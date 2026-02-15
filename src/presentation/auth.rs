use axum::{
    extract::Request,
    http::{header, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::sync::Arc;

type HmacSha256 = Hmac<Sha256>;

/// Shared auth configuration.
#[derive(Clone)]
pub struct AuthConfig {
    /// The password users must provide to log in.
    pub password: String,
    /// A random secret key used to sign session tokens (generated at startup).
    pub secret: Arc<[u8]>,
}

impl AuthConfig {
    pub fn new(password: String) -> Self {
        use rand::RngCore;
        let mut secret = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut secret);
        Self {
            password,
            secret: Arc::from(secret.as_slice()),
        }
    }

    /// Generate a session token (HMAC of the password with the server secret).
    /// This is deterministic for a given password+secret pair, so all valid sessions
    /// produce the same token. Token is invalidated on server restart (new secret).
    pub fn generate_token(&self) -> String {
        let mut mac = HmacSha256::new_from_slice(&self.secret)
            .expect("HMAC can take key of any size");
        mac.update(self.password.as_bytes());
        hex::encode(mac.finalize().into_bytes())
    }

    /// Verify a session token (constant-time comparison).
    pub fn verify_token(&self, token: &str) -> bool {
        let mut mac = HmacSha256::new_from_slice(&self.secret)
            .expect("HMAC can take key of any size");
        mac.update(self.password.as_bytes());
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
