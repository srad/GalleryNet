use crate::domain::{DomainError, MediaRepository};
use std::sync::Arc;
use std::path::PathBuf;
use reqwest::{Client, multipart, header};
use uuid::Uuid;
use serde::Deserialize;

#[derive(Deserialize)]
struct YandexResponse {
    blocks: Vec<YandexBlock>,
}

#[derive(Deserialize)]
struct YandexBlock {
    params: YandexParams,
}

#[derive(Deserialize)]
struct YandexParams {
    url: Option<String>,
}

pub struct ExternalSearchUseCase {
    repo: Arc<dyn MediaRepository>,
    upload_dir: PathBuf,
    client: Client,
    yandex_base_url: String,
}

impl ExternalSearchUseCase {
    pub fn new(repo: Arc<dyn MediaRepository>, upload_dir: PathBuf) -> Self {
        let mut headers = header::HeaderMap::new();
        headers.insert(header::ORIGIN, header::HeaderValue::from_static("https://yandex.com"));
        headers.insert(header::REFERER, header::HeaderValue::from_static("https://yandex.com/images/"));

        let client = Client::builder()
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
            .default_headers(headers)
            .build()
            .unwrap_or_else(|_| Client::new());

        Self {
            repo,
            upload_dir,
            client,
            yandex_base_url: "https://yandex.com".to_string(),
        }
    }

    #[cfg(test)]
    pub fn with_base_url(mut self, url: String) -> Self {
        self.yandex_base_url = url;
        self
    }

    pub async fn execute(&self, id: Uuid) -> Result<String, DomainError> {
        let item = self.repo.find_by_id(id)?
            .ok_or(DomainError::NotFound)?;

        let file_path = self.upload_dir.join(&item.filename);
        let bytes = tokio::fs::read(&file_path).await
            .map_err(|e| DomainError::Io(format!("Failed to read file: {}", e)))?;

        let part = multipart::Part::bytes(bytes)
            .file_name(item.original_filename)
            .mime_str("image/jpeg")
            .map_err(|e| DomainError::Network(format!("Failed to create multipart: {}", e)))?;

        // Yandex uses 'upfile'
        let form = multipart::Form::new().part("upfile", part);

        // Append &format=json to get a reliable JSON response
        let url = format!("{}/images/search?rpt=imageview&format=json", self.yandex_base_url);
        let res = self.client.post(url)
            .multipart(form)
            .send()
            .await
            .map_err(|e| DomainError::Network(format!("Request failed: {}", e)))?;

        if !res.status().is_success() {
             let status = res.status();
            return Err(DomainError::Network(format!("Yandex upload failed with status {}.", status)));
        }

        // Parse JSON response
        // Structure: {"blocks": [{"name": "...", "params": {"url": "https://yandex.com/images/search?..."}}]}
        let body_text = res.text().await
            .map_err(|e| DomainError::Network(format!("Failed to read response body: {}", e)))?;

        let response: YandexResponse = serde_json::from_str(&body_text)
            .map_err(|e| DomainError::Network(format!("Failed to parse Yandex JSON: {}", e)))?;

        if let Some(block) = response.blocks.first() {
            if let Some(url) = &block.params.url {
                return Ok(format!("{}/images/search?{}", self.yandex_base_url, url));
            }
        }
        
        // Fallback: sometimes the URL is just in the "url" param of the object if structure varies
        // But the above is standard for &format=json
        
        Err(DomainError::Network("Could not extract search URL from Yandex response.".to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{routing::post, Router, Json};
    use crate::domain::MediaItem;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_yandex_upload_success() {
        use crate::domain::ports::MediaRepository;
        // 1. Setup mock Yandex server
        let mock_yandex = Router::new().route("/images/search", post(|| async {
            Json(serde_json::json!({
                "blocks": [{
                    "params": { "url": "test_params=1" }
                }]
            }))
        }));
        
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let base_url = format!("http://{}", addr);
        
        tokio::spawn(async move {
            axum::serve(listener, mock_yandex).await.unwrap();
        });

        // 2. Setup mock environment
        let temp = tempdir().unwrap();
        let upload_dir = temp.path().to_path_buf();
        let db = crate::infrastructure::sqlite_repo::TestDb::new("external_search");
        let repo = Arc::new(crate::infrastructure::SqliteRepository::new(&db.path).unwrap());
        
        let id = Uuid::new_v4();
        let filename = "test.jpg".to_string();
        let item = MediaItem {
            id,
            filename: filename.clone(),
            original_filename: "orig.jpg".to_string(),
            media_type: "image".to_string(),
            uploaded_at: chrono::Utc::now(),
            original_date: chrono::Utc::now(),
            size_bytes: 10,
            width: Some(10),
            height: Some(10),
            exif_json: None,
            phash: "hash".to_string(),
            is_favorite: false,
            tags: vec![],
            faces: vec![],
            faces_scanned: true,
        };

        repo.save_metadata_and_vector(&item, None).unwrap();
        
        tokio::fs::write(upload_dir.join(&filename), b"fake image data").await.unwrap();

        // 3. Run Use Case
        let use_case = ExternalSearchUseCase::new(repo, upload_dir)
            .with_base_url(base_url.clone());
            
        let result = use_case.execute(id).await.unwrap();
        
        assert_eq!(result, format!("{}/images/search?test_params=1", base_url));
    }
}
