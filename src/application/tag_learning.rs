use crate::domain::{DomainError, MediaRepository, MediaSummary, TrainedTagModel};
use linfa::composing::platt_scaling::{platt_newton_method, PlattParams};
use linfa::prelude::*;
use linfa_svm::Svm;
use ndarray::{Array1, Array2};
use std::sync::Arc;
use tracing::warn;
use uuid::Uuid;

const CONFIDENCE_THRESHOLD: f64 = 0.5;
const MIN_POSITIVES_FOR_TRAINING: usize = 3;

fn normalize(vec: &mut [f32]) {
    let norm: f32 = vec.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        let inv = 1.0 / norm;
        for v in vec.iter_mut() {
            *v *= inv;
        }
    }
}

fn calculate_centroid(vectors: &[Vec<f32>]) -> Vec<f32> {
    if vectors.is_empty() {
        return Vec::new();
    }
    let dim = vectors[0].len();
    let mut centroid = vec![0.0f32; dim];
    for v in vectors {
        for i in 0..dim {
            centroid[i] += v[i];
        }
    }
    let count = vectors.len() as f32;
    for i in 0..dim {
        centroid[i] /= count;
    }
    normalize(&mut centroid);
    centroid
}

#[allow(dead_code)]
pub fn predict_probability(image_vector: &[f32], model: &TrainedTagModel) -> f64 {
    let score = predict_tag(image_vector, model);
    if score <= -999.0 {
        return 0.0;
    }
    platt_probability(score, model.platt_a, model.platt_b)
}

fn platt_probability(score: f64, platt_a: f64, platt_b: f64) -> f64 {
    let f_apb = platt_a * score + platt_b;
    if f_apb >= 0.0 {
        let e = (-f_apb).exp();
        e / (1.0 + e)
    } else {
        1.0 / (1.0 + f_apb.exp())
    }
}

#[allow(dead_code)]
pub fn predict_tag(image_vector: &[f32], model: &TrainedTagModel) -> f64 {
    if image_vector.len() != model.weights.len() {
        warn!(
            "AI DIMENSION MISMATCH: expected {}, got {}",
            model.weights.len(),
            image_vector.len()
        );
        return -999.0;
    }
    let mut vec = image_vector.to_vec();
    normalize(&mut vec);
    let dot_product: f64 = vec
        .iter()
        .zip(&model.weights)
        .map(|(i, w)| (*i as f64) * w)
        .sum();
    dot_product - model.bias
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

    fn train_and_save_model(
        &self,
        tag_id: i64,
        tag_name: &str,
    ) -> Result<TrainedTagModel, DomainError> {
        let positive_ids = self.repo.get_manual_positives(tag_id)?;
        let trained_at_count = positive_ids.len();
        let mut positive_vectors = Vec::new();
        for id in &positive_ids {
            if let Some(vec) = self.repo.get_embedding(*id)? {
                positive_vectors.push(vec);
            }
        }
        if positive_vectors.len() < MIN_POSITIVES_FOR_TRAINING {
            return Err(DomainError::Ai(format!(
                "Insufficient data for '{}'",
                tag_name
            )));
        }

        let centroid = calculate_centroid(&positive_vectors);
        if positive_vectors.len() > 5 {
            let mut distances: Vec<(usize, f32)> = positive_vectors
                .iter()
                .enumerate()
                .map(|(i, v)| {
                    (
                        i,
                        1.0 - v.iter().zip(&centroid).map(|(a, b)| a * b).sum::<f32>(),
                    )
                })
                .collect();
            distances.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
            let avg_dist = distances.iter().map(|(_, d)| d).sum::<f32>() / distances.len() as f32;
            let mut to_remove = Vec::new();
            for (i, d) in distances {
                if d > avg_dist * 3.0
                    && positive_vectors.len() - to_remove.len() > MIN_POSITIVES_FOR_TRAINING
                {
                    to_remove.push(i);
                }
            }
            to_remove.sort_unstable_by(|a, b| b.cmp(a));
            for i in to_remove {
                positive_vectors.remove(i);
            }
        }

        let all_tagged_ids = self.repo.get_all_ids_with_tag(tag_id)?;
        let raw_hard_negatives =
            self.repo
                .get_nearest_embeddings(&centroid, 50, &all_tagged_ids)?;
        let hard_negatives: Vec<Vec<f32>> = raw_hard_negatives
            .into_iter()
            .map(|(_, v)| v)
            .filter(|v| (1.0 - v.iter().zip(&centroid).map(|(a, b)| a * b).sum::<f32>()) > 0.05)
            .collect();

        let random_count = (positive_vectors.len() * 10).max(50).min(500);
        let mut negative_vectors: Vec<Vec<f32>> = hard_negatives;
        negative_vectors.extend(
            self.repo
                .get_random_embeddings(random_count, &all_tagged_ids)?
                .into_iter()
                .map(|(_, v)| v),
        );

        let model = train_tag_svm(positive_vectors, negative_vectors)?;
        self.repo.save_tag_model(
            tag_id,
            &model.weights,
            model.bias,
            model.platt_a,
            model.platt_b,
            trained_at_count,
        )?;
        Ok(model)
    }

    pub fn learn_tag(&self, tag_name: &str) -> Result<usize, DomainError> {
        let tag_id = self
            .repo
            .get_tag_id_by_name(tag_name)?
            .ok_or(DomainError::NotFound)?;
        let model = self.train_and_save_model(tag_id, tag_name)?;
        let all_embeddings = self.repo.get_all_embeddings(None)?;
        let predictions: Vec<_> = batch_predict(&all_embeddings, &model)
            .into_iter()
            .filter(|(_, score)| *score > CONFIDENCE_THRESHOLD)
            .collect();
        self.repo.update_auto_tags(tag_id, &predictions, None)?;
        Ok(predictions.len())
    }

    pub fn apply_tag_model(
        &self,
        tag_id: i64,
        folder_id: Option<Uuid>,
    ) -> Result<usize, DomainError> {
        let tag_name = self.repo.get_tag_name_by_id(tag_id)?.unwrap_or_default();
        let model = self.train_and_save_model(tag_id, &tag_name)?;
        let embeddings = self.repo.get_all_embeddings(folder_id)?;
        let predictions: Vec<_> = batch_predict(&embeddings, &model)
            .into_iter()
            .filter(|(_, score)| *score > CONFIDENCE_THRESHOLD)
            .collect();
        let scope_ids: Vec<Uuid> = embeddings.iter().map(|(s, _)| s.id).collect();
        self.repo.update_auto_tags(
            tag_id,
            &predictions,
            folder_id.map(|_| scope_ids.as_slice()),
        )?;
        Ok(predictions.len())
    }

    pub fn get_trainable_tags(&self) -> Result<Vec<(i64, String)>, DomainError> {
        Ok(self
            .repo
            .get_tags_with_manual_counts()?
            .into_iter()
            .filter(|(_, _, count)| *count >= MIN_POSITIVES_FOR_TRAINING)
            .map(|(id, name, _)| (id, name))
            .collect())
    }

    pub fn run_auto_tagging(&self, folder_id: Option<Uuid>) -> Result<AutoTagResult, DomainError> {
        let before = self.repo.count_auto_tags(folder_id)?;
        let existing_auto = self.repo.get_tags_with_auto_counts()?;
        let trainable = self
            .repo
            .get_tags_with_manual_counts()?
            .into_iter()
            .filter(|(_, _, count)| *count >= MIN_POSITIVES_FOR_TRAINING)
            .collect::<Vec<_>>();
        let trainable_ids: std::collections::HashSet<_> =
            trainable.iter().map(|(id, _, _)| *id).collect();

        let scope_embeddings = self.repo.get_all_embeddings(folder_id)?;
        if scope_embeddings.is_empty() {
            return Ok(AutoTagResult {
                before,
                after: 0,
                models_processed: 0,
            });
        }
        let scope_ids: Vec<Uuid> = scope_embeddings.iter().map(|(s, _)| s.id).collect();

        for (tag_id, _, _) in existing_auto {
            if !trainable_ids.contains(&tag_id) {
                self.repo.update_auto_tags(tag_id, &[], Some(&scope_ids))?;
            }
        }

        for (tag_id, name, manual_count) in &trainable {
            let last_trained = self.repo.get_last_trained_count(*tag_id)?;
            let model = if *manual_count != last_trained {
                match self.train_and_save_model(*tag_id, name) {
                    Ok(m) => Some(m),
                    Err(e) => {
                        warn!("Failed to train model for tag '{}' (id={}): {}", name, tag_id, e);
                        None
                    }
                }
            } else {
                self.repo.get_tag_model(*tag_id)?
            };
            if let Some(ref model) = model {
                let predictions: Vec<_> = batch_predict(&scope_embeddings, model)
                    .into_iter()
                    .filter(|(_, score)| *score > CONFIDENCE_THRESHOLD)
                    .collect();
                warn!(
                    "Tag '{}': {} predictions above threshold out of {} embeddings",
                    name, predictions.len(), scope_embeddings.len()
                );
                self.repo
                    .update_auto_tags(*tag_id, &predictions, Some(&scope_ids))?;
            }
        }
        Ok(AutoTagResult {
            before,
            after: self.repo.count_auto_tags(folder_id)?,
            models_processed: trainable.len(),
        })
    }
}

pub fn train_tag_svm(
    mut pos: Vec<Vec<f32>>,
    mut neg: Vec<Vec<f32>>,
) -> Result<TrainedTagModel, DomainError> {
    if pos.is_empty() || neg.is_empty() {
        return Err(DomainError::Ai("Empty vectors".to_string()));
    }
    for v in pos.iter_mut() {
        normalize(v);
    }
    for v in neg.iter_mut() {
        normalize(v);
    }
    let dim = pos[0].len();
    let n_pos = pos.len();
    let n_neg = neg.len();
    let ratio = n_neg as f64 / n_pos as f64;
    let mut data = Vec::with_capacity((n_pos + n_neg) * dim);
    for v in &pos {
        data.extend(v.iter().map(|&x| x as f64));
    }
    for v in &neg {
        data.extend(v.iter().map(|&x| x as f64));
    }
    let labels: Vec<bool> = std::iter::repeat(true)
        .take(n_pos)
        .chain(std::iter::repeat(false).take(n_neg))
        .collect();
    let dataset = Dataset::new(
        Array2::from_shape_vec((n_pos + n_neg, dim), data)
            .map_err(|e| DomainError::Ai(e.to_string()))?,
        Array1::from(labels.clone()),
    );
    let model = Svm::<f64, bool>::params()
        .pos_neg_weights(ratio, 1.0)
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
    let raw_scores = dataset.records().dot(&Array1::from(weights.clone())) - bias;
    let (platt_a, platt_b) = platt_newton_method(
        raw_scores.view(),
        Array1::from(labels).view(),
        PlattParams::<f64, ()>::default()
            .check_ref()
            .map_err(|e| DomainError::Ai(format!("{:?}", e)))?,
    )
    .map_err(|e| DomainError::Ai(format!("{:?}", e)))?;
    Ok(TrainedTagModel {
        weights,
        bias,
        platt_a: platt_a as f64,
        platt_b: platt_b as f64,
    })
}

pub fn batch_predict(
    embeddings: &[(MediaSummary, Vec<f32>)],
    model: &TrainedTagModel,
) -> Vec<(Uuid, f64)> {
    if embeddings.is_empty() {
        return Vec::new();
    }
    let dim = model.weights.len();
    let n = embeddings.len();
    let mut data = Vec::with_capacity(n * dim);
    for (_, v) in embeddings {
        let mut normalized = v.clone();
        normalize(&mut normalized);
        data.extend(normalized.iter().map(|&x| x as f64));
    }
    let array = Array2::from_shape_vec((n, dim), data).unwrap_or_else(|_| Array2::zeros((n, dim)));
    let scores = array.dot(&Array1::from(model.weights.clone())) - model.bias;
    embeddings
        .iter()
        .zip(scores.iter())
        .map(|((s, _), &score)| (s.id, platt_probability(score, model.platt_a, model.platt_b)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;

    #[test]
    fn test_svm_math_with_normalized_vectors() {
        let dim = 1280;
        let mut pos = Vec::new();
        for i in 0..10 {
            let mut v = vec![0.0f32; dim];
            v[0] = 1.0;
            v[i % dim] += 0.1;
            pos.push(v);
        }
        let mut neg = Vec::new();
        for i in 0..10 {
            let mut v = vec![0.0f32; dim];
            v[0] = -1.0;
            v[i % dim] += 0.1;
            neg.push(v);
        }
        let model = train_tag_svm(pos, neg).unwrap();
        let mut t_pos = vec![0.0f32; dim];
        t_pos[0] = 1.0;
        assert!(predict_tag(&t_pos, &model) > 0.0);
        let mut t_neg = vec![0.0f32; dim];
        t_neg[0] = -1.0;
        assert!(predict_tag(&t_neg, &model) < 0.0);
    }

    #[test]
    fn test_svm_imbalance_robustness() {
        let dim = 1280;
        let mut pos = Vec::new();
        for i in 0..3 {
            let mut v = vec![0.0f32; dim];
            v[0] = 1.0;
            v[i] += 0.1;
            pos.push(v);
        }
        let mut neg = Vec::new();
        for i in 0..500 {
            let mut v = vec![0.0f32; dim];
            v[0] = -1.0;
            v[i % dim] += 0.1;
            neg.push(v);
        }
        let model = train_tag_svm(pos, neg).unwrap();
        let mut t_pos = vec![0.0f32; dim];
        t_pos[0] = 1.0;
        assert!(predict_probability(&t_pos, &model) > 0.5);
    }

    #[test]
    fn test_learn_tag_insufficient_data_error() {
        let repo = Arc::new(crate::infrastructure::SqliteRepository::new(":memory:").unwrap());
        let use_case = TagLearningUseCase::new(repo);
        assert!(use_case.learn_tag("Nature").is_err());
    }

    #[test]
    fn test_predict_tag_mismatched_dimensions_safety() {
        let model = TrainedTagModel {
            weights: vec![1.0; 1280],
            bias: 0.0,
            platt_a: -2.0,
            platt_b: 0.0,
        };
        assert_eq!(predict_tag(&vec![1.0f32; 10], &model), -999.0);
    }

    #[test]
    fn test_probability_mapping() {
        let model = TrainedTagModel {
            weights: vec![1.0; 10],
            bias: 0.0,
            platt_a: -2.0,
            platt_b: 0.0,
        };
        let mut v = vec![0.0f32; 10];
        v[0] = 1.0;
        assert!(predict_probability(&v, &model) > 0.5);
        v[0] = -1.0;
        assert!(predict_probability(&v, &model) < 0.5);
    }

    #[test]
    fn test_auto_tagging_summary_logic() {
        let db_path = format!("test_summary_{}.db", Uuid::new_v4());
        let repo = Arc::new(crate::infrastructure::SqliteRepository::new(&db_path).unwrap());
        let use_case = TagLearningUseCase::new(repo.clone());
        // 100 items: 15 manual positives, 80 negatives, 5 unlabeled positives
        let ids: Vec<Uuid> = (0..100).map(|_| Uuid::new_v4()).collect();
        repo.with_conn(|conn| {
            for (i, id) in ids.iter().enumerate() {
                conn.execute("INSERT INTO media (id, filename, original_filename, size_bytes, phash, uploaded_at, original_date) VALUES (?1, ?2, ?2, 100, 'abc', '2024-01-01T00:00:00Z', '2024-01-01T00:00:00Z')", params![id.as_bytes(), format!("{}.jpg", i)]).unwrap();
                let mut v = vec![0.0f32; 1280];
                if i < 15 {
                    // Manual positives: strong positive signal
                    v[0] = 1.0; v[1] = 0.5; v[(i + 2) % 1280] = 0.1;
                } else if i >= 95 {
                    // Unlabeled positives: should be auto-tagged
                    v[0] = 1.0; v[1] = 0.5; v[(i + 2) % 1280] = 0.1;
                } else {
                    // Negatives: opposite direction
                    v[0] = -1.0; v[1] = -0.5; v[(i + 2) % 1280] = 0.1;
                }
                let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt(); v.iter_mut().for_each(|x| *x /= norm);
                let bytes: &[u8] = unsafe { std::slice::from_raw_parts(v.as_ptr() as *const u8, 1280 * 4) };
                conn.execute("INSERT INTO vec_media (rowid, embedding) VALUES (?1, ?2)", params![conn.last_insert_rowid(), bytes]).unwrap();
            }
            conn.execute("INSERT INTO tags (id, name) VALUES (1, 'Nature')", []).unwrap();
            for i in 0..15 { conn.execute("INSERT INTO media_tags (media_id, tag_id, is_auto) VALUES (?1, 1, 0)", params![ids[i].as_bytes()]).unwrap(); }
            Ok(())
        }).unwrap();
        let result = use_case.run_auto_tagging(None).unwrap();
        assert!(result.after > 0, "Before: {}, After: {}, Models: {}", result.before, result.after, result.models_processed);
        let _ = std::fs::remove_file(&db_path);
    }
}
