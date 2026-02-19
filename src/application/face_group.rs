use crate::domain::{DomainError, FaceGroup, MediaRepository};
use rayon::prelude::*;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// Maximum number of faces that can be grouped at once.
const MAX_GROUPABLE_FACES: usize = 20_000;

/// Maximum number of edges before aborting to prevent OOM.
const MAX_EDGES: usize = 10_000_000;

struct UnionFind {
    parent: Vec<usize>,
    rank: Vec<usize>,
}

impl UnionFind {
    fn new(n: usize) -> Self {
        Self {
            parent: (0..n).collect(),
            rank: vec![0; n],
        }
    }

    fn find(&mut self, x: usize) -> usize {
        if self.parent[x] != x {
            self.parent[x] = self.parent[self.parent[x]]; // Path halving
            return self.find(self.parent[x]);
        }
        self.parent[x]
    }

    fn union(&mut self, a: usize, b: usize) {
        let ra = self.find(a);
        let rb = self.find(b);
        if ra == rb {
            return;
        }
        match self.rank[ra].cmp(&self.rank[rb]) {
            std::cmp::Ordering::Less => self.parent[ra] = rb,
            std::cmp::Ordering::Greater => self.parent[rb] = ra,
            std::cmp::Ordering::Equal => {
                self.parent[rb] = ra;
                self.rank[ra] += 1;
            }
        }
    }
}

#[inline(always)]
fn dot(a: &[f32], b: &[f32]) -> f32 {
    let mut sum = 0.0f32;
    for i in 0..a.len() {
        sum += a[i] * b[i];
    }
    sum
}

pub struct GroupFacesUseCase {
    repo: Arc<dyn MediaRepository>,
}

impl GroupFacesUseCase {
    pub fn new(repo: Arc<dyn MediaRepository>) -> Self {
        Self { repo }
    }

    pub async fn execute(&self, threshold: f32) -> Result<Vec<FaceGroup>, DomainError> {
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

        for (face_id, _, embedding) in face_data {
            matrix.extend_from_slice(&embedding);
            face_ids.push(face_id);
        }

        // 2. Parallel clustering
        let min_dot = threshold; // For faces, we usually pass similarity 0-1 directly as dot product min
        let edge_count = Arc::new(AtomicUsize::new(0));
        let exceeded = Arc::new(AtomicBool::new(false));

        let edges: Vec<(usize, usize)> = (0..n)
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
                    if dot(vec_i, vec_j) >= min_dot {
                        local_edges.push((i, j));
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

        let mut uf = UnionFind::new(n);
        for &(a, b) in &edges {
            uf.union(a, b);
        }

        // 3. Update clusters in DB
        let mut face_ids_with_clusters = Vec::with_capacity(n);
        for i in 0..n {
            let cluster_id = uf.find(i) as i64;
            face_ids_with_clusters.push((face_ids[i], cluster_id));
        }

        self.repo.update_face_clusters(&face_ids_with_clusters)?;

        // 4. Return grouped items
        self.repo.get_face_groups()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::sqlite_repo::TestDb;
    use crate::domain::MediaItem;
    use crate::domain::ports::MediaRepository;
    use chrono::Utc;
    use uuid::Uuid;

    #[tokio::test]
    async fn test_group_faces_use_case() {
        let db = TestDb::new("test_group_faces_uc");
        let repo = Arc::new(crate::infrastructure::SqliteRepository::new(&db.path).unwrap());
        let use_case = GroupFacesUseCase::new(repo.clone());

        // Setup: 2 media items, each with one face. Faces are similar.
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
                width: Some(100), height: Some(100), size_bytes: 1000,
                exif_json: None, is_favorite: false, tags: vec![],
            };
            repo.save_metadata_and_vector(&media, None).unwrap();

            let face_id = Uuid::new_v4();
            let face = crate::domain::Face {
                id: face_id,
                media_id: *id,
                box_x1: 0, box_y1: 0, box_x2: 10, box_y2: 10,
                cluster_id: None,
            };
            // Exact same embedding for both
            let embedding = vec![1.0f32; 512];
            repo.save_faces(*id, &[face], &[embedding]).unwrap();
        }

        // Execute: group with high similarity threshold
        let groups = use_case.execute(0.9).await.unwrap();

        // Verify: 1 group containing both media items
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].items.len(), 2);
        
        let grouped_ids: Vec<Uuid> = groups[0].items.iter().map(|m| m.id).collect();
        assert!(grouped_ids.contains(&id1));
        assert!(grouped_ids.contains(&id2));
    }

    #[tokio::test]
    async fn test_group_faces_dissimilar() {
        let db = TestDb::new("test_group_faces_dissimilar");
        let repo = Arc::new(crate::infrastructure::SqliteRepository::new(&db.path).unwrap());
        let use_case = GroupFacesUseCase::new(repo.clone());

        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();

        // Media 1: Face with positive embedding
        let media1 = MediaItem {
            id: id1,
            filename: "1.jpg".to_string(),
            original_filename: "1.jpg".to_string(),
            media_type: "image".to_string(),
            phash: "1".to_string(),
            uploaded_at: Utc::now(), original_date: Utc::now(),
            width: Some(100), height: Some(100), size_bytes: 1000,
            exif_json: None, is_favorite: false, tags: vec![],
        };
        repo.save_metadata_and_vector(&media1, None).unwrap();
        repo.save_faces(id1, &[crate::domain::Face {
            id: Uuid::new_v4(), media_id: id1, box_x1: 0, box_y1: 0, box_x2: 10, box_y2: 10, cluster_id: None
        }], &[vec![1.0; 512]]).unwrap();

        // Media 2: Face with negative embedding (completely opposite)
        let media2 = MediaItem {
            id: id2,
            filename: "2.jpg".to_string(),
            original_filename: "2.jpg".to_string(),
            media_type: "image".to_string(),
            phash: "2".to_string(),
            uploaded_at: Utc::now(), original_date: Utc::now(),
            width: Some(100), height: Some(100), size_bytes: 1000,
            exif_json: None, is_favorite: false, tags: vec![],
        };
        repo.save_metadata_and_vector(&media2, None).unwrap();
        repo.save_faces(id2, &[crate::domain::Face {
            id: Uuid::new_v4(), media_id: id2, box_x1: 0, box_y1: 0, box_x2: 10, box_y2: 10, cluster_id: None
        }], &[vec![-1.0; 512]]).unwrap();

        // Execute: group with 0.5 similarity
        let groups = use_case.execute(0.5).await.unwrap();

        // Verify: 0 groups (since each is a singleton cluster)
        assert_eq!(groups.len(), 0);
    }

    #[tokio::test]
    async fn test_group_faces_transitivity() {
        let db = TestDb::new("test_group_faces_transitive");
        let repo = Arc::new(crate::infrastructure::SqliteRepository::new(&db.path).unwrap());
        let use_case = GroupFacesUseCase::new(repo.clone());

        // Setup: 3 media items. 
        // A matches B (similarity 0.9)
        // B matches C (similarity 0.9)
        // A and C are slightly different (similarity 0.7)
        // With threshold 0.8, all three should still end up in the same group.
        
        let ids: Vec<Uuid> = (0..3).map(|_| Uuid::new_v4()).collect();
        let embeddings = vec![
            vec![1.0; 512], // Face A
            vec![1.0; 512], // Face B (identical to A)
            vec![0.9; 512], // Face C (similar to B, but less similar to A)
        ];

        for i in 0..3 {
            let media = MediaItem {
                id: ids[i],
                filename: format!("{}.jpg", i),
                original_filename: "test.jpg".to_string(),
                media_type: "image".to_string(),
                phash: ids[i].to_string(),
                uploaded_at: Utc::now(), original_date: Utc::now(),
                width: Some(100), height: Some(100), size_bytes: 1000,
                exif_json: None, is_favorite: false, tags: vec![],
            };
            repo.save_metadata_and_vector(&media, None).unwrap();
            repo.save_faces(ids[i], &[crate::domain::Face {
                id: Uuid::new_v4(), media_id: ids[i], box_x1: 0, box_y1: 0, box_x2: 10, box_y2: 10, cluster_id: None
            }], &[embeddings[i].clone()]).unwrap();
        }

        // Execute: threshold 0.8
        // A-B will match (1.0 dot), B-C will match (~0.9 dot)
        let groups = use_case.execute(0.8).await.unwrap();

        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].items.len(), 3);
    }
}
