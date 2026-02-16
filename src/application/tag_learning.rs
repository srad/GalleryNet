use crate::domain::{DomainError, MediaRepository, MediaSummary, TrainedTagModel};
use linfa::composing::platt_scaling::{platt_newton_method, PlattParams};
use linfa::prelude::*;
use linfa_svm::Svm;
use ndarray::{Array1, Array2};
use std::sync::Arc;
use tracing::{info, warn};
use uuid::Uuid;

/// Minimum raw SVM score to consider an item as positive.
/// 0.0 = the natural SVM decision boundary (already compensated for class imbalance
/// via pos_neg_weights). A small positive margin reduces borderline false positives.
const RAW_SCORE_THRESHOLD: f64 = 0.0;
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
                "Insufficient data for '{}': {} positives with embeddings (need {})",
                tag_name,
                positive_vectors.len(),
                MIN_POSITIVES_FOR_TRAINING,
            )));
        }

        let centroid = calculate_centroid(&positive_vectors);
        let outliers_removed = if positive_vectors.len() > 5 {
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
            let count = to_remove.len();
            to_remove.sort_unstable_by(|a, b| b.cmp(a));
            for i in to_remove {
                positive_vectors.remove(i);
            }
            count
        } else {
            0
        };

        let all_tagged_ids = self.repo.get_all_ids_with_tag(tag_id)?;
        let raw_hard_negatives =
            self.repo
                .get_nearest_embeddings(&centroid, 20, &all_tagged_ids)?;
        let hard_negatives: Vec<Vec<f32>> = raw_hard_negatives
            .into_iter()
            .map(|(_, v)| v)
            .filter(|v| (1.0 - v.iter().zip(&centroid).map(|(a, b)| a * b).sum::<f32>()) > 0.05)
            .collect();
        let n_hard = hard_negatives.len();

        let random_count = (positive_vectors.len() * 3).max(30).min(100);
        let mut negative_vectors: Vec<Vec<f32>> = hard_negatives;
        negative_vectors.extend(
            self.repo
                .get_random_embeddings(random_count, &all_tagged_ids)?
                .into_iter()
                .map(|(_, v)| v),
        );

        info!(
            tag = tag_name,
            pos = positive_vectors.len(),
            neg = negative_vectors.len(),
            hard = n_hard,
            outliers = outliers_removed,
            "Training SVM"
        );

        let t0 = std::time::Instant::now();
        let model = train_tag_svm(positive_vectors, negative_vectors)?;
        info!(tag = tag_name, elapsed_ms = t0.elapsed().as_millis() as u64, "SVM trained");

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
        let scored = batch_predict_raw(&all_embeddings, &model);
        let predictions: Vec<_> = scored
            .into_iter()
            .filter(|(_, raw, _)| *raw > RAW_SCORE_THRESHOLD)
            .map(|(id, _, confidence)| (id, confidence))
            .collect();
        info!(tag = tag_name, tagged = predictions.len(), total = all_embeddings.len(), "Learn complete");
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
        let scored = batch_predict_raw(&embeddings, &model);
        let predictions: Vec<_> = scored
            .into_iter()
            .filter(|(_, raw, _)| *raw > RAW_SCORE_THRESHOLD)
            .map(|(id, _, confidence)| (id, confidence))
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

        info!(
            tags = trainable.len(),
            scope = ?folder_id,
            "Auto-tagging started"
        );

        let scope_embeddings = self.repo.get_all_embeddings(folder_id)?;
        if scope_embeddings.is_empty() {
            info!("No embeddings found, skipping");
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
            let needs_retrain = *manual_count != last_trained;
            if needs_retrain {
                info!(tag = name.as_str(), manual = manual_count, prev = last_trained, "Retraining");
            } else {
                info!(tag = name.as_str(), "Using cached model");
            }
            let model = if needs_retrain {
                match self.train_and_save_model(*tag_id, name) {
                    Ok(m) => Some(m),
                    Err(e) => {
                        warn!(tag = name.as_str(), error = %e, "Training failed");
                        None
                    }
                }
            } else {
                self.repo.get_tag_model(*tag_id)?
            };
            if let Some(ref model) = model {
                let scored = batch_predict_raw(&scope_embeddings, model);
                let predictions: Vec<_> = scored
                    .into_iter()
                    .filter(|(_, raw, _)| *raw > RAW_SCORE_THRESHOLD)
                    .map(|(id, _, confidence)| (id, confidence))
                    .collect();
                info!(
                    tag = name.as_str(),
                    tagged = predictions.len(),
                    total = scope_embeddings.len(),
                    "Predictions applied"
                );
                self.repo
                    .update_auto_tags(*tag_id, &predictions, Some(&scope_ids))?;
            }
        }
        let after = self.repo.count_auto_tags(folder_id)?;
        info!(before, after, change = (after as i64 - before as i64), "Auto-tagging complete");
        Ok(AutoTagResult {
            before,
            after,
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
    let n_total = n_pos + n_neg;
    let ratio = n_neg as f64 / n_pos as f64;

    // Build full-dimensional data matrix (n × dim)
    let mut full_data = Vec::with_capacity(n_total * dim);
    for v in &pos {
        full_data.extend(v.iter().map(|&x| x as f64));
    }
    for v in &neg {
        full_data.extend(v.iter().map(|&x| x as f64));
    }
    let full_matrix = Array2::from_shape_vec((n_total, dim), full_data)
        .map_err(|e| DomainError::Ai(e.to_string()))?;

    let labels: Vec<bool> = std::iter::repeat(true)
        .take(n_pos)
        .chain(std::iter::repeat(false).take(n_neg))
        .collect();

    let dataset = Dataset::new(full_matrix.clone(), Array1::from(labels.clone()));
    let svm_model = Svm::<f64, bool>::params()
        .pos_neg_weights(ratio, 1.0)
        .linear_kernel()
        .eps(1e-2)
        .fit(&dataset)
        .map_err(|e| DomainError::Ai(format!("{:?}", e)))?;

    let mut weights = Vec::with_capacity(dim);
    let mut basis = Array1::zeros(dim);
    for i in 0..dim {
        basis[i] = 1.0;
        weights.push(svm_model.weighted_sum(&basis));
        basis[i] = 0.0;
    }
    let bias = svm_model.rho as f64;

    // Platt calibration on full-space scores, balanced subsample
    let raw_scores = full_matrix.dot(&Array1::from(weights.clone())) - bias;
    let platt_n = n_pos.min(n_neg);
    let mut platt_scores = Vec::with_capacity(platt_n * 2);
    let mut platt_labels = Vec::with_capacity(platt_n * 2);
    for i in 0..n_pos {
        platt_scores.push(raw_scores[i]);
        platt_labels.push(true);
    }
    for i in n_pos..(n_pos + platt_n) {
        platt_scores.push(raw_scores[i]);
        platt_labels.push(false);
    }

    let (platt_a, platt_b) = platt_newton_method(
        Array1::from(platt_scores).view(),
        Array1::from(platt_labels).view(),
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

/// Returns (media_id, raw_svm_score, platt_confidence) for each embedding.
pub fn batch_predict_raw(
    embeddings: &[(MediaSummary, Vec<f32>)],
    model: &TrainedTagModel,
) -> Vec<(Uuid, f64, f64)> {
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
        .map(|((s, _), &score)| {
            let confidence = platt_probability(score, model.platt_a, model.platt_b);
            (s.id, score, confidence)
        })
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
        // Raw score should be positive (SVM classifies correctly)
        assert!(predict_tag(&t_pos, &model) > 0.0);
        // Platt probability should also be above 0.5 with balanced calibration
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

        // Run again without changes — should use cached model and produce same result
        let result2 = use_case.run_auto_tagging(None).unwrap();
        assert_eq!(result.after, result2.after, "Second run should produce identical results (cached model)");

        let _ = std::fs::remove_file(&db_path);
    }

    #[test]
    fn test_raw_score_threshold_catches_positives() {
        let dim = 1280;
        let mut pos = Vec::new();
        for i in 0..5 {
            let mut v = vec![0.0f32; dim];
            v[0] = 1.0;
            v[1] = 0.5;
            v[(i + 2) % dim] = 0.1;
            pos.push(v);
        }
        let mut neg = Vec::new();
        for i in 0..500 {
            let mut v = vec![0.0f32; dim];
            v[0] = -1.0;
            v[1] = -0.5;
            v[i % dim] += 0.1;
            neg.push(v);
        }
        let model = train_tag_svm(pos, neg).unwrap();

        let mut test_pos = vec![0.0f32; dim];
        test_pos[0] = 1.0;
        test_pos[1] = 0.5;
        test_pos[5] = 0.1;
        let raw_score = predict_tag(&test_pos, &model);
        let prob = predict_probability(&test_pos, &model);

        assert!(raw_score > 0.0, "Raw score should be positive: {:.4}", raw_score);
        assert!(prob > 0.3, "Platt probability should be reasonable: {:.4}", prob);
    }

    /// HYPOTHESIS 1: With imbalanced Platt calibration (old approach), items the SVM
    /// correctly classifies as positive get Platt probabilities BELOW 0.5, causing
    /// the old CONFIDENCE_THRESHOLD of 0.5 to reject them.
    #[test]
    fn test_hypothesis_imbalanced_platt_rejects_true_positives() {
        let dim = 1280;
        let n_pos = 10;
        let n_neg = 500;

        let mut pos = Vec::new();
        for i in 0..n_pos {
            let mut v = vec![0.0f32; dim];
            v[0] = 1.0;
            v[1] = 0.5;
            v[(i + 2) % dim] = 0.1;
            pos.push(v);
        }
        let mut neg = Vec::new();
        for i in 0..n_neg {
            let mut v = vec![0.0f32; dim];
            v[0] = -1.0;
            v[1] = -0.5;
            v[i % dim] += 0.1;
            neg.push(v);
        }

        let model = train_tag_svm(pos.clone(), neg.clone()).unwrap();

        let mut test = vec![0.0f32; dim];
        test[0] = 0.9;
        test[1] = 0.4;
        test[3] = 0.2;
        let raw_score = predict_tag(&test, &model);
        assert!(raw_score > 0.0, "SVM should classify this as positive: raw={:.4}", raw_score);

        // Simulate OLD imbalanced Platt: fit on ALL training data (10 pos vs 500 neg)
        let all_labels: Vec<bool> = std::iter::repeat(true)
            .take(n_pos)
            .chain(std::iter::repeat(false).take(n_neg))
            .collect();
        let mut all_scores = Vec::new();
        for v in &pos {
            all_scores.push(predict_tag(v, &model));
        }
        for v in &neg {
            all_scores.push(predict_tag(v, &model));
        }
        let (old_a, old_b) = platt_newton_method(
            Array1::from(all_scores).view(),
            Array1::from(all_labels).view(),
            PlattParams::<f64, ()>::default().check_ref().unwrap(),
        )
        .unwrap();

        let old_prob = platt_probability(raw_score, old_a as f64, old_b as f64);
        let new_prob = platt_probability(raw_score, model.platt_a, model.platt_b);

        assert!(
            old_prob < 0.5 || new_prob > old_prob,
            "Old Platt should give lower probability than balanced. old={:.4} new={:.4}",
            old_prob, new_prob
        );
        assert!(new_prob > 0.4, "Balanced Platt should give reasonable probability: {:.4}", new_prob);
    }

    /// HYPOTHESIS 2: Random negative sampling causes different models each run.
    #[test]
    fn test_hypothesis_random_negatives_cause_instability() {
        let dim = 1280;
        let pos: Vec<Vec<f32>> = (0..10)
            .map(|i| {
                let mut v = vec![0.0f32; dim];
                v[0] = 1.0;
                v[1] = 0.5;
                v[(i + 2) % dim] = 0.1;
                v
            })
            .collect();

        let neg_set_a: Vec<Vec<f32>> = (0..200)
            .map(|i| {
                let mut v = vec![0.0f32; dim];
                v[0] = -1.0;
                v[(i * 3) % dim] = 0.2;
                v
            })
            .collect();
        let neg_set_b: Vec<Vec<f32>> = (0..200)
            .map(|i| {
                let mut v = vec![0.0f32; dim];
                v[0] = -1.0;
                v[(i * 7 + 100) % dim] = 0.2;
                v
            })
            .collect();

        let model_a = train_tag_svm(pos.clone(), neg_set_a).unwrap();
        let model_b = train_tag_svm(pos.clone(), neg_set_b).unwrap();

        let mut test = vec![0.0f32; dim];
        test[0] = 0.6;
        test[1] = 0.3;
        test[5] = 0.2;
        let score_a = predict_tag(&test, &model_a);
        let score_b = predict_tag(&test, &model_b);

        let diff = (score_a - score_b).abs();
        assert!(diff > 0.0, "Models trained with different negatives should differ");
    }

    /// HYPOTHESIS 3: Once a model is cached, repeated runs produce identical results.
    #[test]
    fn test_hypothesis_cached_model_is_deterministic() {
        let dim = 1280;
        let pos: Vec<Vec<f32>> = (0..10)
            .map(|i| {
                let mut v = vec![0.0f32; dim];
                v[0] = 1.0;
                v[(i + 1) % dim] = 0.1;
                v
            })
            .collect();
        let neg: Vec<Vec<f32>> = (0..100)
            .map(|i| {
                let mut v = vec![0.0f32; dim];
                v[0] = -1.0;
                v[i % dim] += 0.1;
                v
            })
            .collect();
        let model = train_tag_svm(pos, neg).unwrap();

        let test_vectors: Vec<Vec<f32>> = (0..50)
            .map(|i| {
                let mut v = vec![0.0f32; dim];
                v[0] = if i < 25 { 0.8 } else { -0.8 };
                v[(i + 3) % dim] = 0.2;
                v
            })
            .collect();

        let scores_run1: Vec<f64> = test_vectors.iter().map(|v| predict_tag(v, &model)).collect();
        let scores_run2: Vec<f64> = test_vectors.iter().map(|v| predict_tag(v, &model)).collect();

        assert_eq!(scores_run1, scores_run2, "Same model + same data must produce identical scores");

        let count_run1 = scores_run1.iter().filter(|s| **s > RAW_SCORE_THRESHOLD).count();
        let count_run2 = scores_run2.iter().filter(|s| **s > RAW_SCORE_THRESHOLD).count();
        assert_eq!(count_run1, count_run2, "Prediction count must be deterministic");
    }

    /// Full-dimensional SVM training must complete in reasonable time.
    /// The speed comes from eps(1e-2) and no shrinking, not from projection.
    #[test]
    fn test_full_dimensional_training_speed() {
        let dim = 1280;
        let pos: Vec<Vec<f32>> = (0..20)
            .map(|i| {
                let mut v = vec![0.0f32; dim];
                v[0] = 1.0;
                v[(i + 1) % dim] = 0.2;
                v
            })
            .collect();
        let neg: Vec<Vec<f32>> = (0..200)
            .map(|i| {
                let mut v = vec![0.0f32; dim];
                v[0] = -1.0;
                v[i % dim] += 0.1;
                v
            })
            .collect();

        let start = std::time::Instant::now();
        let model = train_tag_svm(pos, neg).unwrap();
        let elapsed = start.elapsed();

        assert_eq!(model.weights.len(), dim);
        assert!(
            elapsed.as_secs() < 30,
            "Training took too long: {:?}",
            elapsed
        );
        eprintln!("Training 20 pos + 200 neg in 1280-d took {:?}", elapsed);
    }
}
