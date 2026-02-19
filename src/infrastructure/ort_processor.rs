use crate::domain::DomainError;
use image::{imageops::FilterType, GenericImageView};
use ndarray017::Array4;
use ort::{inputs, session::Session, value::TensorRef};
use std::sync::{Condvar, Mutex};

const SESSION_POOL_SIZE: usize = 4;

struct SessionBundle {
    mobilenet: Session,
    face_detect: Option<Session>,
    face_embed: Option<Session>,
}

pub struct OrtProcessor {
    pool: Mutex<Vec<SessionBundle>>,
    available: Condvar,
    #[cfg(test)]
    is_mock: bool,
}

impl OrtProcessor {
    pub fn new(model_path: &str) -> Result<Self, DomainError> {
        let mut bundles = Vec::with_capacity(SESSION_POOL_SIZE);
        
        let face_detect_path = "assets/models/version-slim-320.onnx";
        let face_embed_path = "assets/models/w600k_mbf.onnx";

        for _ in 0..SESSION_POOL_SIZE {
            let mobilenet = Session::builder()
                .map_err(|e| DomainError::ModelLoad(e.to_string()))?
                .commit_from_file(model_path)
                .map_err(|e| DomainError::ModelLoad(e.to_string()))?;

            let face_detect = if std::path::Path::new(face_detect_path).exists() {
                Some(Session::builder()
                    .map_err(|e| DomainError::ModelLoad(e.to_string()))?
                    .commit_from_file(face_detect_path)
                    .map_err(|e| DomainError::ModelLoad(e.to_string()))?)
            } else {
                None
            };

            let face_embed = if std::path::Path::new(face_embed_path).exists() {
                Some(Session::builder()
                    .map_err(|e| DomainError::ModelLoad(e.to_string()))?
                    .commit_from_file(face_embed_path)
                    .map_err(|e| DomainError::ModelLoad(e.to_string()))?)
            } else {
                None
            };

            bundles.push(SessionBundle {
                mobilenet,
                face_detect,
                face_embed,
            });
        }

        Ok(Self {
            pool: Mutex::new(bundles),
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
        F: FnOnce(&mut SessionBundle) -> Result<T, DomainError>,
    {
        #[cfg(test)]
        if self.is_mock {
            return Err(DomainError::Ai(
                "Mock processor has no sessions".to_string(),
            ));
        }

        let mut bundle = {
            let mut pool = self
                .pool
                .lock()
                .map_err(|_| DomainError::Ai("Failed to lock session pool".to_string()))?;
            loop {
                if let Some(b) = pool.pop() {
                    break b;
                }
                pool = self
                    .available
                    .wait(pool)
                    .map_err(|_| DomainError::Ai("Session pool wait failed".to_string()))?;
            }
        };

        let result = f(&mut bundle);

        self.pool.lock().unwrap().push(bundle);
        self.available.notify_one();

        result
    }
}

impl crate::domain::AiProcessor for OrtProcessor {
    fn extract_features(&self, image_bytes: &[u8]) -> Result<Vec<f32>, DomainError> {
        // Preprocess outside the session lock
        let img = image::load_from_memory(image_bytes)
            .map_err(|e| DomainError::Ai(format!("Failed to load image: {}", e)))?;

        let (width, height) = img.dimensions();
        let min_dim = width.min(height);
        let crop_x = (width - min_dim) / 2;
        let crop_y = (height - min_dim) / 2;

        let cropped = img.crop_imm(crop_x, crop_y, min_dim, min_dim);
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

        self.with_session(|bundle| {
            let outputs = bundle.mobilenet
                .run(model_inputs)
                .map_err(|e| DomainError::Ai(format!("Inference failed: {}", e)))?;

            let (_shape, output_data) = outputs[0]
                .try_extract_tensor::<f32>()
                .map_err(|e| DomainError::Ai(format!("Failed to extract output: {}", e)))?;

            Ok(output_data.iter().cloned().collect())
        })
    }

    fn detect_and_extract_faces(&self, image_bytes: &[u8]) -> Result<Vec<crate::domain::DetectedFace>, DomainError> {
        let img = image::load_from_memory(image_bytes)
            .map_err(|e| DomainError::Ai(format!("Failed to load image: {}", e)))?;
        
        let (width, height) = img.dimensions();

        // 1. Detect faces using UltraFace-slim (320x240)
        let resized_detect = img.resize_exact(320, 240, FilterType::Triangle);
        let mut input_detect = Array4::<f32>::zeros((1, 3, 240, 320));
        for (x, y, pixel) in resized_detect.pixels() {
            // UltraFace normalization: (x - 127) / 128
            input_detect[[0, 0, y as usize, x as usize]] = (pixel[0] as f32 - 127.0) / 128.0;
            input_detect[[0, 1, y as usize, x as usize]] = (pixel[1] as f32 - 127.0) / 128.0;
            input_detect[[0, 2, y as usize, x as usize]] = (pixel[2] as f32 - 127.0) / 128.0;
        }

        let tensor_detect = TensorRef::from_array_view(&input_detect)
            .map_err(|e| DomainError::Ai(format!("Failed to create detection tensor: {}", e)))?;

        let boxes_and_scores = self.with_session(|bundle| {
            let session = bundle.face_detect.as_mut().ok_or_else(|| DomainError::Ai("Face detection model not loaded".to_string()))?;
            let outputs = session.run(inputs![tensor_detect])
                .map_err(|e| DomainError::Ai(format!("Detection failed: {}", e)))?;
            
            // UltraFace output: [scores, boxes]
            let scores = outputs[0].try_extract_tensor::<f32>().map_err(|e| DomainError::Ai(e.to_string()))?.1.to_vec();
            let boxes = outputs[1].try_extract_tensor::<f32>().map_err(|e| DomainError::Ai(e.to_string()))?.1.to_vec();
            
            Ok((scores, boxes))
        })?;

        let (scores, boxes) = boxes_and_scores;
        // Post-process: filter by score and apply a simple NMS
        let mut candidates = Vec::new();
        for i in 0..(scores.len() / 2) {
            let score = scores[i * 2 + 1];
            if score > 0.8 { 
                let x1 = (boxes[i * 4] * width as f32) as i32;
                let y1 = (boxes[i * 4 + 1] * height as f32) as i32;
                let x2 = (boxes[i * 4 + 2] * width as f32) as i32;
                let y2 = (boxes[i * 4 + 3] * height as f32) as i32;
                candidates.push((score, x1, y1, x2, y2));
            }
        }

        // Simple NMS
        candidates.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
        let mut picked = Vec::new();
        for i in 0..candidates.len() {
            let (s, x1, y1, x2, y2) = candidates[i];
            let mut keep = true;
            for p in &picked {
                let (_, px1, py1, px2, py2) = *p;
                let ix1 = x1.max(px1);
                let iy1 = y1.max(py1);
                let ix2 = x2.min(px2);
                let iy2 = y2.min(py2);
                let iw = (ix2 - ix1).max(0);
                let ih = (iy2 - iy1).max(0);
                let intersection = (iw * ih) as f32;
                let area1 = ((x2 - x1) * (y2 - y1)) as f32;
                let area2 = ((px2 - px1) * (py2 - py1)) as f32;
                let union = area1 + area2 - intersection;
                if union > 0.0 && intersection / union > 0.45 {
                    keep = false;
                    break;
                }
            }
            if keep {
                picked.push((s, x1, y1, x2, y2));
            }
        }

        let mut detected = Vec::new();
        for (_score, x1, y1, x2, y2) in picked {
            // Clamp coordinates to image boundaries
            let x1_clamped = x1.max(0) as u32;
            let y1_clamped = y1.max(0) as u32;
            let x2_clamped = (x2 as u32).min(width);
            let y2_clamped = (y2 as u32).min(height);
            let w = x2_clamped.saturating_sub(x1_clamped);
            let h = y2_clamped.saturating_sub(y1_clamped);

            if w < 2 || h < 2 { continue; }

            // Crop and extract embedding
            let face_img = img.crop_imm(x1_clamped, y1_clamped, w, h);
            let face_resized = face_img.resize_exact(112, 112, FilterType::Triangle);
            let mut input_embed = Array4::<f32>::zeros((1, 3, 112, 112));
            for (fx, fy, pixel) in face_resized.pixels() {
                input_embed[[0, 0, fy as usize, fx as usize]] = (pixel[0] as f32 - 127.5) / 128.0;
                input_embed[[0, 1, fy as usize, fx as usize]] = (pixel[1] as f32 - 127.5) / 128.0;
                input_embed[[0, 2, fy as usize, fx as usize]] = (pixel[2] as f32 - 127.5) / 128.0;
            }

            let tensor_embed = TensorRef::from_array_view(&input_embed)
                .map_err(|e| DomainError::Ai(e.to_string()))?;

            let mut embedding = self.with_session(|bundle| {
                let session = bundle.face_embed.as_mut().ok_or_else(|| DomainError::Ai("Face embedding model not loaded".to_string()))?;
                let outputs = session.run(inputs![tensor_embed])
                    .map_err(|e| DomainError::Ai(format!("Embedding extraction failed: {}", e)))?;
                Ok(outputs[0].try_extract_tensor::<f32>().map_err(|e| DomainError::Ai(e.to_string()))?.1.to_vec())
            })?;

            // L2 Normalize
            let norm = embedding.iter().map(|x| x * x).sum::<f32>().sqrt();
            if norm > 0.0 {
                for x in embedding.iter_mut() { *x /= norm; }
            }

            detected.push(crate::domain::DetectedFace {
                x1, y1, x2, y2,
                embedding,
            });
        }

        Ok(detected)
    }
}
