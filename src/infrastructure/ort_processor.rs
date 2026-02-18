use crate::domain::{AiProcessor, DomainError};
use image::{imageops::FilterType, GenericImageView};
use ndarray017::Array4;
use ort::{inputs, session::Session, value::TensorRef};
use std::sync::{Condvar, Mutex};

const SESSION_POOL_SIZE: usize = 4;

pub struct OrtProcessor {
    pool: Mutex<Vec<Session>>,
    available: Condvar,
    #[cfg(test)]
    is_mock: bool,
}

impl OrtProcessor {
    pub fn new(model_path: &str) -> Result<Self, DomainError> {
        let mut sessions = Vec::with_capacity(SESSION_POOL_SIZE);
        for _ in 0..SESSION_POOL_SIZE {
            let session = Session::builder()
                .map_err(|e| DomainError::ModelLoad(e.to_string()))?
                .commit_from_file(model_path)
                .map_err(|e| DomainError::ModelLoad(e.to_string()))?;
            sessions.push(session);
        }

        Ok(Self {
            pool: Mutex::new(sessions),
            available: Condvar::new(),
            #[cfg(test)]
            is_mock: false,
        })
    }

    #[cfg(test)]
    pub fn new_empty() -> Self {
        Self {
            pool: Mutex::new(vec![]),
            available: Condvar::new(),
            is_mock: true,
        }
    }

    fn with_session<T, F>(&self, f: F) -> Result<T, DomainError>
    where
        F: FnOnce(&mut Session) -> Result<T, DomainError>,
    {
        #[cfg(test)]
        if self.is_mock {
            return Err(DomainError::Ai(
                "Mock processor has no sessions".to_string(),
            ));
        }

        let mut session = {
            let mut pool = self
                .pool
                .lock()
                .map_err(|_| DomainError::Ai("Failed to lock session pool".to_string()))?;
            loop {
                if let Some(s) = pool.pop() {
                    break s;
                }
                pool = self
                    .available
                    .wait(pool)
                    .map_err(|_| DomainError::Ai("Session pool wait failed".to_string()))?;
            }
        };

        let result = f(&mut session);

        self.pool.lock().unwrap().push(session);
        self.available.notify_one();

        result
    }
}

impl AiProcessor for OrtProcessor {
    fn extract_features(&self, image_bytes: &[u8]) -> Result<Vec<f32>, DomainError> {
        // Preprocess outside the session lock
        let img = image::load_from_memory(image_bytes)
            .map_err(|e| DomainError::Ai(format!("Failed to load image: {}", e)))?;

        let (width, height) = img.dimensions();
        let min_dim = width.min(height);
        let crop_x = (width - min_dim) / 2;
        let crop_y = (height - min_dim) / 2;

        let cropped = img.crop_imm(crop_x, crop_y, min_dim, min_dim);
        // Use Triangle (bilinear) for faster resizing, standard for CNN inputs
        let resized = cropped.resize_exact(224, 224, FilterType::Triangle);

        let mean = [0.485, 0.456, 0.406];
        let std = [0.229, 0.224, 0.225];
        let mut input = Array4::<f32>::zeros((1, 3, 224, 224));

        for (x, y, pixel) in resized.pixels() {
            input[[0, 0, y as usize, x as usize]] = (pixel[0] as f32 / 255.0 - mean[0]) / std[0];
            input[[0, 1, y as usize, x as usize]] = (pixel[1] as f32 / 255.0 - mean[1]) / std[1];
            input[[0, 2, y as usize, x as usize]] = (pixel[2] as f32 / 255.0 - mean[2]) / std[2];
        }

        let tensor = TensorRef::from_array_view(&input)
            .map_err(|e| DomainError::Ai(format!("Failed to create tensor inputs: {}", e)))?;

        let model_inputs = inputs![tensor];

        // Only hold a session for the inference itself
        self.with_session(|session| {
            let outputs = session
                .run(model_inputs)
                .map_err(|e| DomainError::Ai(format!("Inference failed: {}", e)))?;

            let (_shape, output_data) = outputs[0]
                .try_extract_tensor::<f32>()
                .map_err(|e| DomainError::Ai(format!("Failed to extract output: {}", e)))?;

            Ok(output_data.iter().cloned().collect())
        })
    }
}
