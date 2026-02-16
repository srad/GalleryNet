use axum::{
    extract::Request,
    http::{header, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

type HmacSha256 = Hmac<Sha256>;

/// Shared auth configuration.
#[derive(Clone)]
pub struct AuthConfig {
    /// The password users must provide to log in.
    pub password: String,
    /// A random secret key used to sign session tokens (generated at startup).
    pub secret: Arc<[u8]>,
    /// Session generation counter — bumped on logout to invalidate all existing tokens.
    pub generation: Arc<AtomicU64>,
}

impl AuthConfig {
    pub fn new(password: String) -> Self {
        use rand::RngCore;
        let mut secret = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut secret);
        Self {
            password,
            secret: Arc::from(secret.as_slice()),
            generation: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Constant-time password verification using XOR fold.
    pub fn verify_password(&self, candidate: &str) -> bool {
        let a = self.password.as_bytes();
        let b = candidate.as_bytes();
        if a.len() != b.len() {
            // Still do work to avoid leaking length via timing,
            // but always return false for different lengths.
            let _ = a.iter().fold(0u8, |acc, &x| acc | x);
            return false;
        }
        a.iter()
            .zip(b.iter())
            .fold(0u8, |acc, (&x, &y)| acc | (x ^ y))
            == 0
    }

    /// Generate a session token (HMAC of the password + generation with the server secret).
    /// Token is invalidated on server restart (new secret) or explicit logout (bumped generation).
    pub fn generate_token(&self) -> String {
        let gen = self.generation.load(Ordering::SeqCst);
        let mut mac = HmacSha256::new_from_slice(&self.secret)
            .expect("HMAC can take key of any size");
        mac.update(self.password.as_bytes());
        mac.update(&gen.to_le_bytes());
        hex::encode(mac.finalize().into_bytes())
    }

    /// Verify a session token (constant-time comparison).
    pub fn verify_token(&self, token: &str) -> bool {
        let gen = self.generation.load(Ordering::SeqCst);
        let mut mac = HmacSha256::new_from_slice(&self.secret)
            .expect("HMAC can take key of any size");
        mac.update(self.password.as_bytes());
        mac.update(&gen.to_le_bytes());
        let expected_bytes = mac.finalize().into_bytes();
        if let Ok(token_bytes) = hex::decode(token) {
            token_bytes.len() == expected_bytes.len()
                && token_bytes
                    .iter()
                    .zip(expected_bytes.iter())
                    .fold(0u8, |acc, (a, b)| acc | (a ^ b))
                    == 0
        } else {
            false
        }
    }

    /// Invalidate all existing sessions by bumping the generation counter.
    pub fn invalidate_sessions(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
    }
}

/// Extract the session token from the `gallery_session` cookie.
fn extract_token(req: &Request) -> Option<String> {
    let cookie_header = req.headers().get(header::COOKIE)?.to_str().ok()?;
    for part in cookie_header.split(';') {
        let part = part.trim();
        if let Some(val) = part.strip_prefix("gallery_session=") {
            return Some(val.to_string());
        }
    }
    None
}

/// Axum middleware that checks for a valid session cookie.
/// Returns 401 if not authenticated.
pub async fn require_auth(
    req: Request,
    next: Next,
) -> Response {
    // AuthConfig is stored as an extension on the request by the layer
    let auth_config = req.extensions().get::<AuthConfig>().cloned();

    match auth_config {
        Some(config) => {
            if let Some(token) = extract_token(&req) {
                if config.verify_token(&token) {
                    return next.run(req).await;
                }
            }
            StatusCode::UNAUTHORIZED.into_response()
        }
        // If no auth config (password not set), allow all requests
        None => next.run(req).await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> AuthConfig {
        AuthConfig {
            password: "test_password".to_string(),
            secret: Arc::from([1u8; 32].as_slice()),
            generation: Arc::new(AtomicU64::new(0)),
        }
    }

    #[test]
    fn verify_password_correct() {
        let config = test_config();
        assert!(config.verify_password("test_password"));
    }

    #[test]
    fn verify_password_wrong() {
        let config = test_config();
        assert!(!config.verify_password("wrong_password"));
    }

    #[test]
    fn verify_password_different_length() {
        let config = test_config();
        assert!(!config.verify_password("short"));
        assert!(!config.verify_password("this_is_a_much_longer_password_than_expected"));
    }

    #[test]
    fn token_generation_and_verification() {
        let config = test_config();
        let token = config.generate_token();
        assert!(config.verify_token(&token));
    }

    #[test]
    fn token_invalidation_after_bump() {
        let config = test_config();
        let token_before = config.generate_token();
        assert!(config.verify_token(&token_before));

        config.invalidate_sessions();

        // Old token no longer valid
        assert!(!config.verify_token(&token_before));

        // New token works
        let token_after = config.generate_token();
        assert!(config.verify_token(&token_after));
    }

    #[test]
    fn invalid_token_rejected() {
        let config = test_config();
        assert!(!config.verify_token("not_hex"));
        assert!(!config.verify_token("deadbeef"));
        assert!(!config.verify_token(""));
    }
}
