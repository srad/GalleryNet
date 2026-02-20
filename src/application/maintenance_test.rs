#[cfg(test)]
mod tests {
    use crate::application::{FixThumbnailsUseCase, IndexFacesUseCase};
    use crate::domain::{AiProcessor, DomainError, HashGenerator, MediaItem, MediaRepository, DetectedFace};

    use crate::infrastructure::{SqliteRepository, TestDb};
    use std::sync::Arc;
    use uuid::Uuid;
    use tokio::fs;

    // Mock AI Processor
    struct MockAiProcessor;
    impl AiProcessor for MockAiProcessor {
        fn extract_features(&self, _image_data: &[u8]) -> Result<Vec<f32>, DomainError> {
            Ok(vec![0.1; 1280])
        }
        fn detect_and_extract_faces(&self, _image_bytes: &[u8]) -> Result<Vec<DetectedFace>, DomainError> {
            Ok(vec![DetectedFace {
                x1: 0, y1: 0, x2: 10, y2: 10,
                embedding: vec![0.5; 512],
            }])
        }

    }

    // Mock Hash Generator
    struct MockHashGenerator;
    impl HashGenerator for MockHashGenerator {
        fn generate_phash(&self, _image_data: &[u8]) -> Result<String, DomainError> {
            Ok("mock_phash".to_string())
        }
    }

    #[tokio::test]
    async fn test_fix_thumbnails_execution() {
        // Setup
        let db = TestDb::new("maintenance_test");
        // Only verify access to TestDb repo
        let _ = &db.repo; 
        
        let ai = Arc::new(MockAiProcessor);
        let hasher = Arc::new(MockHashGenerator);
        
        let temp_dir = tempfile::tempdir().unwrap();
        let upload_dir = temp_dir.path().join("uploads");
        let thumbnail_dir = temp_dir.path().join("thumbnails");
        fs::create_dir_all(&upload_dir).await.unwrap();
        fs::create_dir_all(&thumbnail_dir).await.unwrap();

        // We need to clone db.repo into Arc, but TestDb owns it.
        // TestDb design in infrastructure seems to hold the repo.
        // Let's create a new repo sharing the same path?
        // SqliteRepository::new(&db.path).unwrap() 
        // But TestDb drops the file on drop.
        // Actually, db.repo is SqliteRepository. We can wrap it in Arc?
        // But we can't move it out of TestDb.
        // So we create a new one pointing to the same file.
        let repo = Arc::new(SqliteRepository::new(&db.path).unwrap());

        let use_case = FixThumbnailsUseCase::new(
            repo.clone(),
            ai,
            hasher,
            upload_dir.clone(),
            thumbnail_dir.clone(),
        );

        // 1. Insert media with 'no_hash'
        let id = Uuid::new_v4();
        let id_str = id.to_string();
        let (p1, p2) = (&id_str[0..2], &id_str[2..4]);
        let filename = format!("{}/{}/{}.jpg", p1, p2, id);
        
        // Create physical file (1x1 PNG)
        let valid_png = vec![
            137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 
            8, 6, 0, 0, 0, 31, 21, 196, 137, 0, 0, 0, 10, 73, 68, 65, 84, 120, 156, 99, 0, 1, 0, 
            0, 5, 0, 1, 13, 10, 45, 180, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130
        ];
        
        let file_path = upload_dir.join(&filename);
        fs::create_dir_all(file_path.parent().unwrap()).await.unwrap();
        fs::write(&file_path, &valid_png).await.unwrap();

        // Insert into DB
        let media = MediaItem {
            id,
            filename: filename.clone(),
            original_filename: "test.png".to_string(), // .png so processor knows
            media_type: "image".to_string(),
            phash: "no_hash".to_string(),
            uploaded_at: chrono::Utc::now(),
            original_date: chrono::Utc::now(),
            width: None,
            height: None,
            size_bytes: valid_png.len() as i64,
            exif_json: None,
            is_favorite: false,
            tags: vec![],
            faces: vec![],
            faces_scanned: true,
        };

        repo.save_metadata_and_vector(&media, None).unwrap();

        // 2. Execute fix
        let fixed = use_case.execute().await.unwrap();

        // 3. Verify
        assert_eq!(fixed.len(), 1);
        assert_eq!(fixed[0].id, id);
        assert_ne!(fixed[0].phash, "no_hash");
        // MockHashGenerator returns "mock_phash", but processor logic might use the real hasher or ignore it?
        // In maintenance.rs:
        // let processed = processor::process_media(..., self.hasher.as_ref()).await
        // processor::process_media uses hasher.generate_phash
        assert_eq!(fixed[0].phash, "mock_phash");

        // Check thumbnail existence
        let thumb_path = thumbnail_dir.join(p1).join(p2).join(format!("{}.jpg", id));
        assert!(thumb_path.exists(), "Thumbnail should be created at {:?}", thumb_path);

        // 4. Run again - should find nothing
        let fixed_again = use_case.execute().await.unwrap();
        assert_eq!(fixed_again.len(), 0);
    }

    #[tokio::test]
    async fn test_index_faces_execution() {
        // Setup
        let db = TestDb::new("index_faces_test");
        let repo = Arc::new(SqliteRepository::new(&db.path).unwrap());
        let ai = Arc::new(MockAiProcessor);
        
        let temp_dir = tempfile::tempdir().unwrap();
        let upload_dir = temp_dir.path().join("uploads");
        fs::create_dir_all(&upload_dir).await.unwrap();

        let use_case = IndexFacesUseCase::new(
            repo.clone(),
            ai,
            upload_dir.clone(),
        );

        // 1. Insert media that is NOT yet scanned
        let id = Uuid::new_v4();
        let id_str = id.to_string();
        let (p1, p2) = (&id_str[0..2], &id_str[2..4]);
        let filename = format!("{}/{}/{}.jpg", p1, p2, id);
        
        let valid_png = vec![
            137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 
            8, 6, 0, 0, 0, 31, 21, 196, 137, 0, 0, 0, 10, 73, 68, 65, 84, 120, 156, 99, 0, 1, 0, 
            0, 5, 0, 1, 13, 10, 45, 180, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130
        ];
        
        let file_path = upload_dir.join(&filename);
        fs::create_dir_all(file_path.parent().unwrap()).await.unwrap();
        fs::write(&file_path, &valid_png).await.unwrap();

        let media = MediaItem {
            id,
            filename: filename.clone(),
            original_filename: "test.png".to_string(),
            media_type: "image".to_string(),
            phash: "existing_phash".to_string(),
            uploaded_at: chrono::Utc::now(),
            original_date: chrono::Utc::now(),
            width: Some(1),
            height: Some(1),
            size_bytes: valid_png.len() as i64,
            exif_json: None,
            is_favorite: false,
            tags: vec![],
            faces: vec![],
            faces_scanned: false, // Important: mark as NOT scanned
        };
        repo.save_metadata_and_vector(&media, None).unwrap();

        // Verify it exists in the "unscanned" list
        let unscanned = repo.find_media_unscanned_faces(10).unwrap();
        assert_eq!(unscanned.len(), 1);
        assert_eq!(unscanned[0].id, id);

        // 2. Execute indexing
        let processed_count = use_case.execute(10).await.unwrap();
        assert_eq!(processed_count, 1);

        // 3. Verify database state
        let item = repo.find_by_id(id).unwrap().unwrap();
        assert!(item.faces_scanned);

        // Check if faces were saved (MockAiProcessor returns 1 face)
        let embeddings = repo.get_all_face_embeddings().unwrap();
        assert_eq!(embeddings.len(), 1);
        assert_eq!(embeddings[0].1, id); // media_id check

        // 4. Run again - should process 0
        let processed_again = use_case.execute(10).await.unwrap();
        assert_eq!(processed_again, 0);
    }
}

