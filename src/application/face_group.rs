use crate::domain::{DomainError, FaceGroup, MediaRepository};
use rayon::prelude::*;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use uuid::Uuid;


/// Maximum number of faces that can be grouped at once.
const MAX_GROUPABLE_FACES: usize = 20_000;

/// Maximum number of edges before aborting to prevent OOM.
const MAX_EDGES: usize = 10_000_000;

#[inline(always)]
fn l2_normalize(v: &mut [f32]) {
    let sq_sum: f32 = v.iter().map(|x| x * x).sum();
    let magnitude = sq_sum.sqrt();
    if magnitude > 1e-6 {
        for x in v.iter_mut() {
            *x /= magnitude;
        }
    }
}

#[inline(always)]
fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}

pub struct GroupFacesUseCase {
    repo: Arc<dyn MediaRepository>,
}

impl GroupFacesUseCase {
    pub fn new(repo: Arc<dyn MediaRepository>) -> Self {
        Self { repo }
    }

    pub async fn execute(&self, threshold: f32) -> Result<Vec<crate::domain::FaceGroup>, DomainError> {
        self.execute_sync(threshold)
    }

    pub fn execute_sync(&self, threshold: f32) -> Result<Vec<crate::domain::FaceGroup>, DomainError> {
        // 1. Load all face embeddings
        let face_data = self.repo.get_all_face_embeddings()?;

        if face_data.is_empty() {
            return Ok(Vec::new());
        }

        let n = face_data.len();
        if n > MAX_GROUPABLE_FACES {
            return Err(DomainError::Io(format!(
                "Too many faces to group: {} (max {})",
                n, MAX_GROUPABLE_FACES
            )));
        }

        let dim = face_data[0].2.len();
        let mut matrix = Vec::with_capacity(n * dim);
        let mut face_ids = Vec::with_capacity(n);

        // L2 Normalize embeddings before flattening (Critical for ArcFace/MobileFaceNet)
        for (face_id, _, mut embedding) in face_data {
            l2_normalize(&mut embedding);
            matrix.extend_from_slice(&embedding);
            face_ids.push(face_id);
        }

        // 2. Parallel Edge Calculation (Cosine Similarity)
        let edge_count = Arc::new(AtomicUsize::new(0));
        let exceeded = Arc::new(AtomicBool::new(false));

        let edges: Vec<(usize, usize, f32)> = (0..n)
            .into_par_iter()
            .flat_map_iter(|i| {
                if exceeded.load(Ordering::Relaxed) {
                    return Vec::new();
                }
                let vec_i = &matrix[i * dim..(i + 1) * dim];
                let mut local_edges = Vec::new();

                for j in (i + 1)..n {
                    if exceeded.load(Ordering::Relaxed) {
                        break;
                    }
                    let vec_j = &matrix[j * dim..(j + 1) * dim];
                    let similarity = dot(vec_i, vec_j);

                    if similarity >= threshold {
                        // Store the similarity weight for Chinese Whispers
                        local_edges.push((i, j, similarity));
                        if edge_count.fetch_add(1, Ordering::Relaxed) + 1 > MAX_EDGES {
                            exceeded.store(true, Ordering::Relaxed);
                            break;
                        }
                    }
                }
                local_edges
            })
            .collect();

        if exceeded.load(Ordering::Relaxed) {
            return Err(DomainError::Io(format!(
                "Too many similar face pairs (>{})",
                MAX_EDGES
            )));
        }

        // 3. Build Adjacency List for Graph Clustering
        let mut adj_list = vec![Vec::new(); n];
        for &(u, v, weight) in &edges {
            adj_list[u].push((v, weight));
            adj_list[v].push((u, weight));
        }

        // 4. Chinese Whispers Clustering Algorithm
        let mut labels: Vec<usize> = (0..n).collect(); // Initially, each face is its own cluster
        let max_iterations = 10; // Usually converges in 2-5 iterations

        for _ in 0..max_iterations {
            let mut changed = false;

            // In a production system, you might want to shuffle the iteration order here using `rand`,
            // but sequential processing works perfectly fine for most face datasets.
            for i in 0..n {
                if adj_list[i].is_empty() {
                    continue;
                }

                // Calculate the total similarity weight for each neighboring label
                let mut label_weights: HashMap<usize, f32> =
                    HashMap::with_capacity(adj_list[i].len());
                for &(neighbor, weight) in &adj_list[i] {
                    *label_weights.entry(labels[neighbor]).or_insert(0.0) += weight;
                }

                // Find the label with the highest accumulated weight
                if let Some((&best_label, _)) = label_weights
                    .iter()
                    .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
                {
                    if labels[i] != best_label {
                        labels[i] = best_label;
                        changed = true;
                    }
                }
            }

            if !changed {
                break; // Converged
            }
        }

        // 5. Update clusters in DB
        let mut face_ids_with_clusters = Vec::with_capacity(n);
        for i in 0..n {
            face_ids_with_clusters.push((face_ids[i], labels[i] as i64));
        }

        self.repo.update_face_clusters(&face_ids_with_clusters)?;

        // 6. Return grouped items
        self.repo.get_face_groups()
    }
}

pub struct FindSimilarFacesUseCase {
    repo: Arc<dyn MediaRepository>,
}

impl FindSimilarFacesUseCase {
    pub fn new(repo: Arc<dyn MediaRepository>) -> Self {
        Self { repo }
    }

    pub async fn execute(
        &self,
        reference_face_id: Uuid,
        threshold: f32,
    ) -> Result<Vec<crate::domain::MediaItem>, DomainError> {
        // 1. Fetch the embedding for the face the user clicked on
        let ref_embedding = self.repo.get_face_embedding(reference_face_id)?;

        // 2. Use KNN search via sqlite-vec for high performance
        // Threshold check: cosine distance = 1 - similarity
        // So similarity >= threshold  =>  1 - distance >= threshold  =>  distance <= 1 - threshold
        let max_distance = 1.0 - threshold;
        
        let nearest = self.repo.get_nearest_face_embeddings(&ref_embedding, 1000)?;
        
        // 3. Filter by distance and deduplicate media IDs
        let mut matched_media_ids = Vec::new();
        let mut seen_media = std::collections::HashSet::new();

        for (_face_id, media_id, distance) in nearest {
            if distance <= max_distance {
                if seen_media.insert(media_id) {
                    matched_media_ids.push(media_id);
                }
            }
        }

        if matched_media_ids.is_empty() {
            return Ok(Vec::new());
        }

        // 4. Fetch the actual media metadata
        self.repo.get_media_items_by_ids(&matched_media_ids)
    }
}

pub struct ListPeopleUseCase {
    repo: Arc<dyn MediaRepository>,
}

impl ListPeopleUseCase {
    pub fn new(repo: Arc<dyn MediaRepository>) -> Self {
        Self { repo }
    }

    pub async fn execute(&self) -> Result<Vec<crate::domain::PersonSummary>, DomainError> {
        let reps = self.repo.get_cluster_representatives()?;
        
        Ok(reps.into_iter().map(|(cluster_id, media, face)| {
            crate::domain::PersonSummary {
                cluster_id,
                representative_media: media,
                representative_face: face,
            }
        }).collect())
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ports::MediaRepository;
    use crate::domain::MediaItem;
    use crate::infrastructure::sqlite_repo::TestDb;
    use chrono::Utc;
    use uuid::Uuid;

    #[tokio::test]
    async fn test_group_faces_use_case() {
        let db = TestDb::new("test_group_faces_uc");
        let repo = Arc::new(crate::infrastructure::SqliteRepository::new(&db.path).unwrap());
        let use_case = GroupFacesUseCase::new(repo.clone());

        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();

        for id in &[id1, id2] {
            let media = MediaItem {
                id: *id,
                filename: format!("{}.jpg", id),
                original_filename: "test.jpg".to_string(),
                media_type: "image".to_string(),
                phash: id.to_string(),
                uploaded_at: Utc::now(),
                original_date: Utc::now(),
                width: Some(100),
                height: Some(100),
                size_bytes: 1000,
                exif_json: None,
                is_favorite: false,
                tags: vec![],
                faces: vec![],
                faces_scanned: true,
            };

            repo.save_metadata_and_vector(&media, None).unwrap();

            let face = crate::domain::Face {
                id: Uuid::new_v4(),
                media_id: *id,
                box_x1: 0,
                box_y1: 0,
                box_x2: 10,
                box_y2: 10,
                cluster_id: None,
            };
            let embedding = vec![1.0f32; 512];
            repo.save_faces(*id, &[face], &[embedding]).unwrap();
        }

        let groups = use_case.execute(0.9).await.unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].items.len(), 2);
    }

    #[tokio::test]
    async fn test_chinese_whispers_prevents_chaining() {
        let db = TestDb::new("test_cw_prevents_chaining");
        let repo = Arc::new(crate::infrastructure::SqliteRepository::new(&db.path).unwrap());
        let use_case = GroupFacesUseCase::new(repo.clone());

        // Setup: Two distinct clusters of faces (A and B), with one weak link bridging them.
        // Node 0, 1, 2 are Cluster A.
        // Node 3, 4, 5 are Cluster B.
        // Node 2 and Node 3 are weakly connected.
        // Under Union-Find, they would merge into 1 group. Under Chinese Whispers, they stay 2 groups.

        let ids: Vec<Uuid> = (0..6).map(|_| Uuid::new_v4()).collect();
        let mut embeddings = vec![vec![0.0f32; 512]; 6];

        // Cluster A (Strongly connected to each other)
        embeddings[0][0] = 1.0;
        embeddings[0][1] = 1.0; // Normalizes to ~0.707
        embeddings[1][0] = 1.0;
        embeddings[1][1] = 0.9;
        embeddings[2][0] = 1.0;
        embeddings[2][1] = 0.8;

        // The Weak Link connecting Cluster A and B
        embeddings[2][2] = 0.5;
        embeddings[3][2] = 0.5;

        // Cluster B (Strongly connected to each other)
        embeddings[3][3] = 1.0;
        embeddings[3][4] = 0.8;
        embeddings[4][3] = 1.0;
        embeddings[4][4] = 0.9;
        embeddings[5][3] = 1.0;
        embeddings[5][4] = 1.0;

        for i in 0..6 {
            let media = MediaItem {
                id: ids[i],
                filename: format!("{}.jpg", i),
                original_filename: "test.jpg".to_string(),
                media_type: "image".to_string(),
                phash: ids[i].to_string(),
                uploaded_at: Utc::now(),
                original_date: Utc::now(),
                width: Some(100),
                height: Some(100),
                size_bytes: 1000,
                exif_json: None,
                is_favorite: false,
                tags: vec![],
                faces: vec![],
                faces_scanned: true,
            };

            repo.save_metadata_and_vector(&media, None).unwrap();
            repo.save_faces(
                ids[i],
                &[crate::domain::Face {
                    id: Uuid::new_v4(),
                    media_id: ids[i],
                    box_x1: 0,
                    box_y1: 0,
                    box_x2: 10,
                    box_y2: 10,
                    cluster_id: None,
                }],
                &[embeddings[i].clone()],
            )
            .unwrap();
        }

        // Execute: Lower threshold to allow the weak link to register as an edge
        let groups = use_case.execute(0.3).await.unwrap();

        // Verify: Chinese Whispers should correctly sever the weak link and return 2 distinct clusters
        assert_eq!(
            groups.len(),
            2,
            "Chinese Whispers failed to sever the chain!"
        );
    }

    #[tokio::test]
    async fn test_group_faces_dissimilar() {
        let db = TestDb::new("test_faces_dissimilar");
        let repo = Arc::new(crate::infrastructure::SqliteRepository::new(&db.path).unwrap());
        let use_case = GroupFacesUseCase::new(repo.clone());

        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();

        // Face 1
        let media1 = MediaItem {
            id: id1,
            filename: "1.jpg".to_string(),
            original_filename: "1.jpg".to_string(),
            media_type: "image".to_string(),
            phash: "1".to_string(),
            uploaded_at: Utc::now(),
            original_date: Utc::now(),
            width: Some(100), height: Some(100), size_bytes: 1000,
            exif_json: None, is_favorite: false, tags: vec![], faces: vec![], faces_scanned: true,
        };
        repo.save_metadata_and_vector(&media1, None).unwrap();
        let mut v1 = vec![0.0f32; 512];
        v1[0] = 1.0;
        repo.save_faces(id1, &[crate::domain::Face {
            id: Uuid::new_v4(), media_id: id1, box_x1: 0, box_y1: 0, box_x2: 10, box_y2: 10, cluster_id: None,
        }], &[v1]).unwrap();

        // Face 2 (Orthogonal/Dissimilar)
        let media2 = MediaItem {
            id: id2,
            filename: "2.jpg".to_string(),
            original_filename: "2.jpg".to_string(),
            media_type: "image".to_string(),
            phash: "2".to_string(),
            uploaded_at: Utc::now(),
            original_date: Utc::now(),
            width: Some(100), height: Some(100), size_bytes: 1000,
            exif_json: None, is_favorite: false, tags: vec![], faces: vec![], faces_scanned: true,
        };
        repo.save_metadata_and_vector(&media2, None).unwrap();
        
        let mut v2 = vec![0.0f32; 512];
        v2[1] = 1.0;
        repo.save_faces(id2, &[crate::domain::Face {
            id: Uuid::new_v4(), media_id: id2, box_x1: 0, box_y1: 0, box_x2: 10, box_y2: 10, cluster_id: None,
        }], &[v2]).unwrap();

        // Execute: group with 0.5 similarity
        let groups = use_case.execute(0.5).await.unwrap();

        // Verify: 2 groups (each is a singleton cluster)
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].items.len(), 1);
        assert_eq!(groups[1].items.len(), 1);
    }
}

    