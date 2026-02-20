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
                Some(
                    Session::builder()
                        .map_err(|e| DomainError::ModelLoad(e.to_string()))?
                        .commit_from_file(face_detect_path)
                        .map_err(|e| DomainError::ModelLoad(e.to_string()))?,
                )
            } else {
                None
            };

            let face_embed = if std::path::Path::new(face_embed_path).exists() {
                Some(
                    Session::builder()
                        .map_err(|e| DomainError::ModelLoad(e.to_string()))?
                        .commit_from_file(face_embed_path)
                        .map_err(|e| DomainError::ModelLoad(e.to_string()))?,
                )
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
            let outputs = bundle
                .mobilenet
                .run(model_inputs)
                .map_err(|e| DomainError::Ai(format!("Inference failed: {}", e)))?;

            let (_shape, output_data) = outputs[0]
                .try_extract_tensor::<f32>()
                .map_err(|e| DomainError::Ai(format!("Failed to extract output: {}", e)))?;

            Ok(output_data.iter().cloned().collect())
        })
    }

    fn detect_and_extract_faces(
        &self,
        image_bytes: &[u8],
    ) -> Result<Vec<crate::domain::DetectedFace>, DomainError> {
        let mut img = image::load_from_memory(image_bytes)
            .map_err(|e| DomainError::Ai(format!("Failed to load image: {}", e)))?;

        // Apply EXIF orientation so AI sees the image upright
        let mut orientation = 1u32;
        if let Ok(exif) =
            exif::Reader::new().read_from_container(&mut std::io::Cursor::new(image_bytes))
        {
            if let Some(field) = exif.get_field(exif::Tag::Orientation, exif::In::PRIMARY) {
                if let Some(val) = field.value.get_uint(0) {
                    orientation = val;
                }
            }
        }
        img = crate::application::processor::apply_orientation(img, orientation);

        let (width, height) = img.dimensions();

        // 1. Detect faces using UltraFace-slim (320x240)
        // Stretched resize is often more reliable for this specific model's anchor grid
        let resized_detect = img.resize_exact(320, 240, FilterType::CatmullRom);
        let mut input_detect = Array4::<f32>::zeros((1, 3, 240, 320));
        for (x, y, pixel) in resized_detect.pixels() {
            // UltraFace normalization: (x - 127) / 128
            input_detect[[0, 0, y as usize, x as usize]] = (pixel[0] as f32 - 127.0) / 128.0;
            input_detect[[0, 1, y as usize, x as usize]] = (pixel[1] as f32 - 127.0) / 128.0;
            input_detect[[0, 2, y as usize, x as usize]] = (pixel[2] as f32 - 127.0) / 128.0;
        }

        let tensor_detect = TensorRef::from_array_view(&input_detect)
            .map_err(|e| DomainError::Ai(format!("Failed to create detection tensor: {}", e)))?;

        let (boxes, scores) = self.with_session(|bundle| {
            let session = bundle
                .face_detect
                .as_mut()
                .ok_or_else(|| DomainError::Ai("Face detection model not loaded".to_string()))?;
            let outputs = session
                .run(inputs![tensor_detect])
                .map_err(|e| DomainError::Ai(format!("Detection failed: {}", e)))?;

            let out0 = outputs[0]
                .try_extract_tensor::<f32>()
                .map_err(|e| DomainError::Ai(e.to_string()))?
                .1
                .to_vec();
            let out1 = outputs[1]
                .try_extract_tensor::<f32>()
                .map_err(|e| DomainError::Ai(e.to_string()))?
                .1
                .to_vec();

            // Auto-detect which one is boxes and which one is scores based on length
            if out0.len() == 17680 && out1.len() == 8840 {
                Ok((out0, out1))
            } else if out1.len() == 17680 && out0.len() == 8840 {
                Ok((out1, out0))
            } else {
                Err(DomainError::Ai(format!(
                    "Unexpected output shapes from face detection model: {} and {}",
                    out0.len(),
                    out1.len()
                )))
            }
        })?;

        // Generate anchors for UltraFace-Slim 320
        let mut anchors = Vec::with_capacity(4420);
        let feature_maps = [[40, 30], [20, 15], [10, 8], [5, 4]];
        let strides = [8, 16, 32, 64];
        let min_sizes = [
            vec![10.0, 16.0, 24.0],
            vec![32.0, 48.0],
            vec![64.0, 96.0],
            vec![128.0, 192.0, 256.0],
        ];

        for i in 0..4 {
            let map_w = feature_maps[i][0];
            let map_h = feature_maps[i][1];
            let stride = strides[i];
            for y in 0..map_h {
                for x in 0..map_w {
                    for &min_size in &min_sizes[i] {
                        let anchor_x = (x as f32 + 0.5) * stride as f32 / 320.0;
                        let anchor_y = (y as f32 + 0.5) * stride as f32 / 240.0;
                        let anchor_w = min_size / 320.0;
                        let anchor_h = min_size / 240.0;
                        anchors.push([anchor_x, anchor_y, anchor_w, anchor_h]);
                    }
                }
            }
        }

        let mut candidates = Vec::new();
        for i in 0..anchors.len() {
            let score = scores[i * 2 + 1];
            if score > 0.7 {
                let anchor = anchors[i];

                let dx = boxes[i * 4];
                let dy = boxes[i * 4 + 1];
                let dw = boxes[i * 4 + 2];
                let dh = boxes[i * 4 + 3];

                // Decode box (Standard SSD/UltraFace decoding)
                let center_x = anchor[0] + dx * 0.1 * anchor[2];
                let center_y = anchor[1] + dy * 0.1 * anchor[3];
                let w = anchor[2] * (dw * 0.2).exp();
                let h = anchor[3] * (dh * 0.2).exp();

                let x1 = ((center_x - w / 2.0) * width as f32) as i32;
                let y1 = ((center_y - h / 2.0) * height as f32) as i32;
                let x2 = ((center_x + w / 2.0) * width as f32) as i32;
                let y2 = ((center_y + h / 2.0) * height as f32) as i32;

                candidates.push((score, x1, y1, x2, y2));
            }
        }

        // Improved NMS
        candidates.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        let mut picked = Vec::new();
        for i in 0..candidates.len() {
            let (s, x1, y1, x2, y2) = candidates[i];
            let mut keep = true;

            let w1 = (x2 - x1).max(0);
            let h1 = (y2 - y1).max(0);
            let area1 = (w1 * h1) as f32;

            if area1 <= 0.0 {
                continue;
            }

            for p in &picked {
                let (_, px1, py1, px2, py2) = *p;
                let ix1 = x1.max(px1);
                let iy1 = y1.max(py1);
                let ix2 = x2.min(px2);
                let iy2 = y2.min(py2);
                let iw = (ix2 - ix1).max(0);
                let ih = (iy2 - iy1).max(0);
                let intersection = (iw * ih) as f32;

                if intersection <= 0.0 {
                    continue;
                }

                let pw = (px2 - px1).max(0);
                let ph = (py2 - py1).max(0);
                let area2 = (pw * ph) as f32;

                let union = area1 + area2 - intersection;
                let iou = intersection / union;
                let iom = intersection / area1.min(area2);

                // Stricter merging: if IoU > 0.3 OR IoM > 0.4 (one box inside another)
                if iou > 0.3 || iom > 0.4 {
                    keep = false;
                    break;
                }
            }
            if keep {
                picked.push((s, x1, y1, x2, y2));
            }
        }

        if !candidates.is_empty() {
            tracing::info!(
                "Face detection: {} candidates above threshold (0.7), {} picked after NMS",
                candidates.len(),
                picked.len()
            );
        }

        let mut detected = Vec::new();
        for (_score, x1, y1, x2, y2) in picked {
            // Add a small margin (15%) to capture the whole head, not just the face box.
            // This significantly improves recognition accuracy.
            let face_w = (x2 - x1) as f32;
            let face_h = (y2 - y1) as f32;
            let margin_x = (face_w * 0.15) as i32;
            let margin_y = (face_h * 0.15) as i32;

            let x1_m = x1 - margin_x;
            let y1_m = y1 - margin_y;
            let x2_m = x2 + margin_x;
            let y2_m = y2 + margin_y;

            // Clamp coordinates to image boundaries
            let x1_clamped = x1_m.max(0) as u32;
            let y1_clamped = y1_m.max(0) as u32;
            let x2_clamped = (x2_m as u32).min(width);
            let y2_clamped = (y2_m as u32).min(height);
            let w = x2_clamped.saturating_sub(x1_clamped);
            let h = y2_clamped.saturating_sub(y1_clamped);

            if w < 2 || h < 2 {
                continue;
            }

            // Crop and extract embedding
            let face_img = img.crop_imm(x1_clamped, y1_clamped, w, h);
            let face_resized = face_img.resize_exact(112, 112, FilterType::CatmullRom);
            let mut input_embed = Array4::<f32>::zeros((1, 3, 112, 112));
            for (fx, fy, pixel) in face_resized.pixels() {
                input_embed[[0, 0, fy as usize, fx as usize]] = (pixel[0] as f32 - 127.5) / 128.0;
                input_embed[[0, 1, fy as usize, fx as usize]] = (pixel[1] as f32 - 127.5) / 128.0;
                input_embed[[0, 2, fy as usize, fx as usize]] = (pixel[2] as f32 - 127.5) / 128.0;
            }

            let tensor_embed = TensorRef::from_array_view(&input_embed)
                .map_err(|e| DomainError::Ai(e.to_string()))?;

            let mut embedding = self.with_session(|bundle| {
                let session = bundle.face_embed.as_mut().ok_or_else(|| {
                    DomainError::Ai("Face embedding model not loaded".to_string())
                })?;
                let outputs = session
                    .run(inputs![tensor_embed])
                    .map_err(|e| DomainError::Ai(format!("Embedding extraction failed: {}", e)))?;
                Ok(outputs[0]
                    .try_extract_tensor::<f32>()
                    .map_err(|e| DomainError::Ai(e.to_string()))?
                    .1
                    .to_vec())
            })?;

            // L2 Normalize
            let norm = embedding.iter().map(|x| x * x).sum::<f32>().sqrt();
            if norm > 0.0 {
                let inv = 1.0 / norm;
                for x in embedding.iter_mut() {
                    *x *= inv;
                }
            }

            detected.push(crate::domain::DetectedFace {
                x1: x1_m.clamp(0, width as i32),
                y1: y1_m.clamp(0, height as i32),
                x2: x2_m.clamp(0, width as i32),
                y2: y2_m.clamp(0, height as i32),
                embedding,
            });
        }

        Ok(detected)
    }
}
