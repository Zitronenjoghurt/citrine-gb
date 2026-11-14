use base64::Engine;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use reqwest::StatusCode;
use reqwest::header::CONTENT_TYPE;
use ring::rand::SystemRandom;
use ring::signature::{RSA_PKCS1_SHA256, RsaKeyPair};
use serde::Deserialize;
use serde_json::{Value, json};
use std::path::Path;
use std::time::{Duration, Instant};

pub const DEFAULT_ENDPOINT: &str = "https://fcm.googleapis.com";
const SCOPE: &str = "https://www.googleapis.com/auth/firebase.messaging";
const ASSERTION_SECS: u64 = 3600;
const TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Deserialize)]
struct ServiceAccount {
    project_id: String,
    client_email: String,
    private_key: String,
    token_uri: String,
}

#[derive(Deserialize)]
struct AccessToken {
    access_token: String,
    expires_in: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum KeyError {
    #[error("{0}")]
    Read(#[from] std::io::Error),
    #[error("it is not a service account key: {0}")]
    Json(#[from] serde_json::Error),
    #[error("its private_key is not an RSA key in PKCS#8 PEM")]
    Key,
    #[error("{0}")]
    Client(#[from] reqwest::Error),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Sent,
    Gone,
    Failed,
}

pub struct Fcm {
    http: reqwest::Client,
    account: ServiceAccount,
    key: RsaKeyPair,
    rng: SystemRandom,
    send_url: String,
    access: Option<(String, Instant)>,
}

fn pem_der(pem: &str) -> Option<Vec<u8>> {
    let body: String = pem
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("-----"))
        .collect();
    STANDARD.decode(body).ok()
}

impl Fcm {
    pub fn from_file(path: &Path, endpoint: &str) -> Result<Self, KeyError> {
        Self::from_json(&std::fs::read_to_string(path)?, endpoint)
    }

    pub fn from_json(text: &str, endpoint: &str) -> Result<Self, KeyError> {
        let account: ServiceAccount = serde_json::from_str(text)?;
        let der = pem_der(&account.private_key).ok_or(KeyError::Key)?;
        let key = RsaKeyPair::from_pkcs8(&der).map_err(|_| KeyError::Key)?;
        let send_url = format!(
            "{}/v1/projects/{}/messages:send",
            endpoint.trim_end_matches('/'),
            account.project_id
        );
        Ok(Self {
            http: reqwest::Client::builder().timeout(TIMEOUT).build()?,
            account,
            key,
            rng: SystemRandom::new(),
            send_url,
            access: None,
        })
    }

    pub fn project(&self) -> &str {
        &self.account.project_id
    }

    fn assertion(&self, now: u64) -> Option<String> {
        let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"RS256","typ":"JWT"}"#);
        let claims = json!({
            "iss": self.account.client_email,
            "scope": SCOPE,
            "aud": self.account.token_uri,
            "iat": now,
            "exp": now + ASSERTION_SECS,
        });
        let claims = URL_SAFE_NO_PAD.encode(claims.to_string());
        let signed = format!("{header}.{claims}");
        let mut signature = vec![0; self.key.public().modulus_len()];
        self.key
            .sign(
                &RSA_PKCS1_SHA256,
                &self.rng,
                signed.as_bytes(),
                &mut signature,
            )
            .ok()?;
        Some(format!("{signed}.{}", URL_SAFE_NO_PAD.encode(signature)))
    }

    async fn access_token(&mut self) -> Option<String> {
        if let Some((token, until)) = &self.access
            && Instant::now() < *until
        {
            return Some(token.clone());
        }
        let assertion = self.assertion(crate::now())?;
        let reply = self
            .http
            .post(&self.account.token_uri)
            .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(format!(
                "grant_type=urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Ajwt-bearer&assertion={assertion}"
            ))
            .send()
            .await;
        let reply = match reply {
            Ok(reply) if reply.status().is_success() => reply,
            Ok(reply) => {
                log::warn!("Firebase refused the service account: {}", reply.status());
                return None;
            }
            Err(err) => {
                log::warn!("Firebase sign-in failed: {err}");
                return None;
            }
        };
        let token: AccessToken = serde_json::from_slice(&reply.bytes().await.ok()?).ok()?;
        let lifetime = Duration::from_secs(token.expires_in.saturating_sub(60));
        self.access = Some((token.access_token.clone(), Instant::now() + lifetime));
        Some(token.access_token)
    }

    pub async fn send(&mut self, body: &Value) -> Outcome {
        for first in [true, false] {
            let Some(token) = self.access_token().await else {
                return Outcome::Failed;
            };
            let reply = self
                .http
                .post(&self.send_url)
                .bearer_auth(token)
                .header(CONTENT_TYPE, "application/json")
                .body(body.to_string())
                .send()
                .await;
            let reply = match reply {
                Ok(reply) => reply,
                Err(err) => {
                    log::warn!("push notification not sent: {err}");
                    return Outcome::Failed;
                }
            };
            let status = reply.status();
            if status.is_success() {
                return Outcome::Sent;
            }
            if status == StatusCode::UNAUTHORIZED && first {
                self.access = None;
                continue;
            }
            if status == StatusCode::NOT_FOUND {
                return Outcome::Gone;
            }
            let text = reply.text().await.unwrap_or_default();
            if status == StatusCode::BAD_REQUEST && text.contains("message.token") {
                return Outcome::Gone;
            }
            log::warn!("Firebase refused a push notification: {status}");
            return Outcome::Failed;
        }
        Outcome::Failed
    }
}
