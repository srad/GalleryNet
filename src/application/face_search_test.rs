#[cfg(test)]
mod tests {
    use crate::application::{FindSimilarFacesUseCase, ListPeopleUseCase};
    use crate::domain::{AiProcessor, DomainError, MediaItem, MediaRepository, Face, DetectedFace};
    use crate::infrastructure::{SqliteRepository, TestDb};
    use std::sync::Arc;
    use uuid::Uuid;
    use chrono::Utc;

    // Mock AI Processor
    #[allow(dead_code)]
    struct MockAiProcessor;
    impl AiProcessor for MockAiProcessor {
        fn extract_features(&self, _image_data: &[u8]) -> Result<Vec<f32>, DomainError> {
            Ok(vec![0.1; 1280])
        }
        fn detect_and_extract_faces(&self, _image_bytes: &[u8]) -> Result<Vec<DetectedFace>, DomainError> {
            Ok(vec![])
        }
    }

    #[tokio::test]
    async fn test_face_search_and_people_listing() {
        let db = TestDb::new("face_search_test");
        let repo = Arc::new(SqliteRepository::new(&db.path).unwrap());
        
        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();
        
        // Insert 2 media items
        for (i, id) in [id1, id2].iter().enumerate() {
            let media = MediaItem {
                id: *id,
                filename: format!("{}.jpg", i),
                original_filename: "test.jpg".to_string(),
                media_type: "image".to_string(),
                phash: id.to_string(),
                uploaded_at: Utc::now(),
                original_date: Utc::now(),
                width: Some(100), height: Some(100), size_bytes: 1000,
                exif_json: None, is_favorite: false, tags: vec![],
                faces: vec![], faces_scanned: true,
            };
            repo.save_metadata_and_vector(&media, None).unwrap();
        }

        let face_id1 = Uuid::new_v4();
        let face_id2 = Uuid::new_v4();

        // Save similar faces with distinct embeddings to ensure deterministic sort order
        let mut v1 = vec![0.0f32; 512];
        v1[0] = 1.0;
        
        let mut v2 = vec![0.0f32; 512];
        v2[0] = 0.8;
        v2[1] = 0.6; // This ensures v2 is different from v1 even after normalization

        repo.save_faces(id1, &[Face {
            id: face_id1, media_id: id1, box_x1: 0, box_y1: 0, box_x2: 10, box_y2: 10, cluster_id: Some(1)
        }], &[v1]).unwrap();
        
        repo.save_faces(id2, &[Face {
            id: face_id2, media_id: id2, box_x1: 0, box_y1: 0, box_x2: 10, box_y2: 10, cluster_id: Some(1)
        }], &[v2]).unwrap();

        // 1. Test FindSimilarFacesUseCase
        let search_use_case = FindSimilarFacesUseCase::new(repo.clone());
        let results = search_use_case.execute(face_id1, 0.7).await.unwrap();
        
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].id, id1); // Exact match first
        assert_eq!(results[1].id, id2); // Similar match second

        // 2. Test ListPeopleUseCase
        let list_use_case = ListPeopleUseCase::new(repo.clone());
        let people = list_use_case.execute().await.unwrap();
        
        assert_eq!(people.len(), 1);
        assert_eq!(people[0].cluster_id, 1);
        let rep_id = people[0].representative_media.id;
        assert!(rep_id == id1 || rep_id == id2);
    }
}
