use crate::domain::{HashGenerator, DomainError};
use image_hasher::{HasherConfig, HashAlg};
use image::load_from_memory;
use base64::{Engine as _, engine::general_purpose};

pub struct PhashGenerator;

impl PhashGenerator {
    pub fn new() -> Self {
        Self
    }
}

impl HashGenerator for PhashGenerator {
    fn generate_phash(&self, image_bytes: &[u8]) -> Result<String, DomainError> {
        let image = load_from_memory(image_bytes)
            .map_err(|e| DomainError::Hashing(format!("Failed to load image: {}", e)))?;

        let hasher = HasherConfig::new()
            .hash_alg(HashAlg::DoubleGradient)
            .to_hasher();

        let hash = hasher.hash_image(&image);

        Ok(general_purpose::STANDARD.encode(hash.as_bytes()))
    }
}
