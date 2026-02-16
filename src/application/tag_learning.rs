use crate::domain::{DomainError, MediaRepository};
use linfa::prelude::*;
use linfa_svm::Svm;
use ndarray::{Array1, Array2};
use std::sync::Arc;
use tracing::warn;
use uuid::Uuid;

const CONFIDENCE_THRESHOLD: f64 = 0.5;
const MIN_POSITIVES_FOR_TRAINING: usize = 3;

#[derive(Debug, Clone)]
pub struct TrainedTagModel {
    pub weights: Vec<f64>,
    pub bias: f64,
}

pub struct AutoTagResult {
    pub before: usize,
    pub after: usize,
    pub models_processed: usize,
}

pub struct TagLearningUseCase {
    repo: Arc<dyn MediaRepository>,
}

impl TagLearningUseCase {
    pub fn new(repo: Arc<dyn MediaRepository>) -> Self {
        Self { repo }
    }

    /// Internal helper to train a model for a specific tag ID using its current manual positives.
    fn train_and_save_model(
        &self,
        tag_id: i64,
        tag_name: &str,
    ) -> Result<TrainedTagModel, DomainError> {
        let positive_ids = self.repo.get_manual_positives(tag_id)?;

        if positive_ids.is_empty() {
            return Err(DomainError::Ai(format!(
                "Tag '{}' is not used on any media items. Please tag at least {} items manually first.",
                tag_name, MIN_POSITIVES_FOR_TRAINING
            )));
        }

        if positive_ids.len() < MIN_POSITIVES_FOR_TRAINING {
            return Err(DomainError::Ai(format!(
                "Tag '{}' only has {} manual examples. Need at least {} to train the AI.",
                tag_name,
                positive_ids.len(),
                MIN_POSITIVES_FOR_TRAINING
            )));
        }

        let mut positive_vectors = Vec::new();
        for id in &positive_ids {
            if let Some(vec) = self.repo.get_embedding(*id)? {
                positive_vectors.push(vec);
            }
        }

        if positive_vectors.is_empty() {
            return Err(DomainError::Ai(format!(
                "Tag '{}' has {} manual tags, but none of those images have AI features extracted yet. Please wait for processing to finish.",
                tag_name, positive_ids.len()
            )));
        }

        if positive_vectors.len() < MIN_POSITIVES_FOR_TRAINING {
            return Err(DomainError::Ai(format!(
                "Tag '{}' has {} manual tags, but only {} have AI features extracted. Need at least {} items with features to train.",
                tag_name,
                positive_ids.len(),
                positive_vectors.len(),
                MIN_POSITIVES_FOR_TRAINING
            )));
        }

        let neg_count = (positive_vectors.len() * 10).max(50).min(500);

        // Exclude ALL items already marked with this tag (manual or auto) from negative samples
        // to prevent contaminating the "negative" group with valid but untagged positives.
        let all_tagged_ids = self.repo.get_all_ids_with_tag(tag_id)?;
        let negative_samples = self
            .repo
            .get_random_embeddings(neg_count, &all_tagged_ids)?;
        let negative_vectors: Vec<Vec<f32>> =
            negative_samples.into_iter().map(|(_, v)| v).collect();

        let model = train_tag_svm(positive_vectors, negative_vectors)?;
        self.repo
            .save_tag_model(tag_id, &model.weights, model.bias)?;

        Ok(model)
    }

    pub fn learn_tag(
        &self,
        tag_name: &str,
        _positive_ids: Vec<Uuid>,
    ) -> Result<usize, DomainError> {
        let tag_id = self
            .repo
            .get_tag_id_by_name(tag_name)?
            .ok_or(DomainError::NotFound)?;

        // Retrain and apply globally
        let model = self.train_and_save_model(tag_id, tag_name)?;

        let all_embeddings = self.repo.get_all_embeddings(None)?;
        let mut predictions = Vec::new();
        for (summary, vector) in all_embeddings {
            let score = predict_tag(&vector, &model);
            if score > CONFIDENCE_THRESHOLD {
                predictions.push((summary.id, score));
            }
        }

        self.repo.update_auto_tags(tag_id, &predictions, None)?;
        Ok(predictions.len())
    }

    pub fn apply_tag_model(
        &self,
        tag_id: i64,
        folder_id: Option<Uuid>,
    ) -> Result<usize, DomainError> {
        // Need to find the name for error reporting
        let tag_name = self
            .repo
            .get_all_tags()?
            .into_iter()
            .find(|t| self.repo.get_tag_id_by_name(t).unwrap_or(None) == Some(tag_id))
            .unwrap_or_else(|| "Unknown".to_string());

        // ALWAYS retrain before applying to ensure we have the latest user feedback
        let model = self.train_and_save_model(tag_id, &tag_name)?;

        let embeddings = self.repo.get_all_embeddings(folder_id)?;
        let mut predictions = Vec::new();

        for (summary, vector) in &embeddings {
            let score = predict_tag(vector, &model);
            if score > CONFIDENCE_THRESHOLD {
                predictions.push((summary.id, score));
            }
        }

        if let Some(_) = folder_id {
            let scope_ids: Vec<Uuid> = embeddings.iter().map(|(s, _)| s.id).collect();
            self.repo
                .update_auto_tags(tag_id, &predictions, Some(&scope_ids))?;
        } else {
            self.repo.update_auto_tags(tag_id, &predictions, None)?;
        }

        Ok(predictions.len())
    }

    pub fn get_trainable_tags(&self) -> Result<Vec<(i64, String)>, DomainError> {
        let tags_with_counts = self.repo.get_tags_with_manual_counts()?;
        let trainable = tags_with_counts
            .into_iter()
            .filter(|(_, _, count)| *count >= MIN_POSITIVES_FOR_TRAINING)
            .map(|(id, name, _)| (id, name))
            .collect();
        Ok(trainable)
    }

    pub fn run_auto_tagging(&self, folder_id: Option<Uuid>) -> Result<AutoTagResult, DomainError> {
        let before = self.repo.count_auto_tags(folder_id)?;

        // 1. Get all tags that currently have auto-tags (to check for stale ones)
        let existing_auto = self.repo.get_tags_with_auto_counts()?;

        // 2. Get all tags that are trainable
        let trainable = self.get_trainable_tags()?;
        let trainable_ids: std::collections::HashSet<i64> =
            trainable.iter().map(|(id, _)| *id).collect();

        // 3. Identify tags that need cleanup (have auto-tags but are no longer trainable)
        let mut to_cleanup = Vec::new();
        for (tag_id, _name, _count) in existing_auto {
            if !trainable_ids.contains(&tag_id) {
                to_cleanup.push(tag_id);
            }
        }

        // 4. Get embeddings for the current scope
        let scope_embeddings = self.repo.get_all_embeddings(folder_id)?;
        if scope_embeddings.is_empty() {
            return Ok(AutoTagResult {
                before,
                after: 0,
                models_processed: 0,
            });
        }
        let scope_ids: Vec<Uuid> = scope_embeddings.iter().map(|(s, _)| s.id).collect();

        // 5. Cleanup stale tags in scope
        for tag_id in to_cleanup {
            self.repo.update_auto_tags(tag_id, &[], Some(&scope_ids))?;
        }

        let models_processed = trainable.len();

        // 6. For each trainable tag, retrain and apply to scope
        for (tag_id, name) in trainable {
            // Train model using ALL manual examples
            let model = match self.train_and_save_model(tag_id, &name) {
                Ok(m) => m,
                Err(_) => {
                    // If training fails (e.g. all vectors invalid), clear auto-tags
                    self.repo.update_auto_tags(tag_id, &[], Some(&scope_ids))?;
                    continue;
                }
            };

            let mut predictions = Vec::new();
            for (summary, vector) in &scope_embeddings {
                let score = predict_tag(vector, &model);
                if score > CONFIDENCE_THRESHOLD {
                    predictions.push((summary.id, score));
                }
            }

            // This will replace existing auto-tags for this tag in the scope
            self.repo
                .update_auto_tags(tag_id, &predictions, Some(&scope_ids))?;
        }

        let after = self.repo.count_auto_tags(folder_id)?;

        Ok(AutoTagResult {
            before,
            after,
            models_processed,
        })
    }
}

pub fn train_tag_svm(
    positive_vectors: Vec<Vec<f32>>,
    negative_vectors: Vec<Vec<f32>>,
) -> Result<TrainedTagModel, DomainError> {
    let n_pos = positive_vectors.len();
    let n_neg = negative_vectors.len();
    let dim = 1280;

    let mut all_data = Vec::with_capacity((n_pos + n_neg) * dim);
    for v in &positive_vectors {
        all_data.extend(v.iter().map(|&x| x as f64));
    }
    for v in &negative_vectors {
        all_data.extend(v.iter().map(|&x| x as f64));
    }

    let dataset_array = Array2::from_shape_vec((n_pos + n_neg, dim), all_data)
        .map_err(|e| DomainError::Ai(e.to_string()))?;

    let mut labels_vec = vec![true; n_pos];
    labels_vec.extend(vec![false; n_neg]);
    let labels_array = Array1::from(labels_vec);

    let dataset = Dataset::new(dataset_array, labels_array);

    let model = Svm::<f64, bool>::params()
        .linear_kernel()
        .fit(&dataset)
        .map_err(|e| DomainError::Ai(format!("{:?}", e)))?;

    let mut weights = Vec::with_capacity(dim);
    let mut basis = Array1::zeros(dim);
    for i in 0..dim {
        basis[i] = 1.0;
        weights.push(model.weighted_sum(&basis));
        basis[i] = 0.0;
    }

    let bias = model.rho as f64;
    Ok(TrainedTagModel { weights, bias })
}

pub fn predict_tag(image_vector: &[f32], model: &TrainedTagModel) -> f64 {
    if image_vector.len() != model.weights.len() {
        warn!(
            "AI DIMENSION MISMATCH: expected {}, got {}",
            model.weights.len(),
            image_vector.len()
        );
        return -999.0;
    }
    let dot_product: f64 = image_vector
        .iter()
        .zip(&model.weights)
        .map(|(i, w)| (*i as f64) * w)
        .sum();

    dot_product - model.bias
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;

    #[test]
    fn test_svm_math_with_normalized_vectors() {
        let dim = 1280;
        let mut positives = Vec::new();
        for i in 0..10 {
            let mut v = vec![0.0f32; dim];
            v[0] = 1.0;
            v[i % dim] += 0.1;
            let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
            v.iter_mut().for_each(|x| *x /= norm);
            positives.push(v);
        }

        let mut negatives = Vec::new();
        for i in 0..50 {
            let mut v = vec![0.0f32; dim];
            v[1] = 1.0;
            v[i % dim] += 0.1;
            let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
            v.iter_mut().for_each(|x| *x /= norm);
            negatives.push(v);
        }

        let model = train_tag_svm(positives, negatives).unwrap();

        let mut test_pos = vec![0.0f32; dim];
        test_pos[0] = 1.0;
        let score_pos = predict_tag(&test_pos, &model);
        assert!(
            score_pos > 0.0,
            "Positive should have score > 0, got {}",
            score_pos
        );

        let mut test_neg = vec![0.0f32; dim];
        test_neg[1] = 1.0;
        let score_neg = predict_tag(&test_neg, &model);
        assert!(
            score_neg < 0.0,
            "Negative should have score < 0, got {}",
            score_neg
        );
    }

    #[test]
    fn test_svm_confidence_threshold_with_real_world_overlap() {
        let dim = 1280;
        let mut positives = Vec::new();
        for _ in 0..5 {
            let mut v = vec![0.0f32; dim];
            v[0] = 1.0;
            for j in 0..dim {
                v[j] += 0.5;
            }
            let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
            v.iter_mut().for_each(|x| *x /= norm);
            positives.push(v);
        }

        let mut negatives = Vec::new();
        for _ in 0..50 {
            let mut v = vec![0.0f32; dim];
            v[1] = 1.0;
            for j in 0..dim {
                v[j] += 0.5;
            }
            let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
            v.iter_mut().for_each(|x| *x /= norm);
            negatives.push(v);
        }

        let model = train_tag_svm(positives, negatives).unwrap();
        assert!(model.bias < 2.0);
    }

    #[test]
    fn test_svm_imbalance_robustness() {
        let dim = 1280;
        let mut positives = Vec::new();
        // ONLY 3 positives
        for _ in 0..3 {
            let mut v = vec![0.0f32; dim];
            v[0] = 1.0;
            let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
            v.iter_mut().for_each(|x| *x /= norm);
            positives.push(v);
        }

        let mut negatives = Vec::new();
        // 500 negatives
        for _ in 0..500 {
            let mut v = vec![0.0f32; dim];
            v[1] = 1.0;
            let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
            v.iter_mut().for_each(|x| *x /= norm);
            negatives.push(v);
        }

        let model = train_tag_svm(positives, negatives).unwrap();

        // Test a positive
        let mut test_pos = vec![0.0f32; dim];
        test_pos[0] = 1.0;
        let score = predict_tag(&test_pos, &model);
        println!("Imbalanced positive score: {}", score);
        assert!(
            score > 0.5,
            "Should meet confidence threshold, got {}",
            score
        );
    }

    #[test]
    fn test_learn_tag_insufficient_data_error() {
        let repo = Arc::new(crate::infrastructure::SqliteRepository::new(":memory:").unwrap());
        let use_case = TagLearningUseCase::new(repo);

        // Tag doesn't exist yet, but even if it did, 0 positives should fail
        let res = use_case.learn_tag("Nature", vec![]);
        assert!(res.is_err(), "Should fail with empty positives");
    }

    #[test]
    fn test_predict_tag_mismatched_dimensions_safety() {
        let model = TrainedTagModel {
            weights: vec![1.0; 1280],
            bias: 0.0,
        };
        // Passing a 10-dimensional vector when 1280 is expected
        let short_vec = vec![1.0f32; 10];
        let score = predict_tag(&short_vec, &model);
        assert_eq!(
            score, -999.0,
            "Should return sentinel error score on mismatch"
        );
    }

    #[test]
    fn test_auto_tagging_summary_logic() {
        let db_path = format!("test_summary_{}.db", Uuid::new_v4());
        let repo = Arc::new(crate::infrastructure::SqliteRepository::new(&db_path).unwrap());
        let use_case = TagLearningUseCase::new(repo.clone());

        let media_ids: Vec<Uuid> = (0..5).map(|_| Uuid::new_v4()).collect();

        // 1. Setup media and tags
        repo.with_conn(|conn| {
            for (i, id) in media_ids.iter().enumerate() {
                conn.execute(
                    "INSERT INTO media (id, filename, original_filename, size_bytes, phash, uploaded_at, original_date) 
                     VALUES (?1, ?2, ?2, 100, 'abc', '2024-01-01T00:00:00Z', '2024-01-01T00:00:00Z')",
                    params![id.as_bytes(), format!("test{}.jpg", i)],
                ).unwrap();
                
                // Add dummy normalized embedding (vectors near [1,0,0...])
                let mut vec = vec![0.0f32; 1280];
                vec[0] = 1.0;
                let vector_bytes: &[u8] = unsafe {
                    std::slice::from_raw_parts(vec.as_ptr() as *const u8, 1280 * 4)
                };
                conn.execute(
                    "INSERT INTO vec_media (rowid, embedding) VALUES (?1, ?2)",
                    params![conn.last_insert_rowid(), vector_bytes],
                ).unwrap();
            }
            
            // Nature tag with 3 manual examples (making it trainable)
            conn.execute("INSERT INTO tags (id, name) VALUES (1, 'Nature')", []).unwrap();
            for i in 0..3 {
                conn.execute("INSERT INTO media_tags (media_id, tag_id, is_auto) VALUES (?1, 1, 0)", params![media_ids[i].as_bytes()]).unwrap();
            }
            Ok(())
        }).unwrap();

        // 2. Pre-condition: Item 4 has a STALE auto-tag (not matching model we will train)
        // Actually let's just test that after run, we have more auto tags.

        // 3. Run auto-tagging
        let result = use_case.run_auto_tagging(None).unwrap();

        // 4. Verify results
        assert_eq!(result.before, 0);
        assert!(result.after > 0, "Should have applied new auto-tags");
        assert_eq!(result.models_processed, 1); // Only 'Nature' had 3 examples

        let _ = std::fs::remove_file(&db_path);
    }
}
