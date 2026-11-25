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
const REASON_CHARS: usize = 200;

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Sent,
    Gone,
    Failed(String),
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

fn hint(code: &str) -> Option<&'static str> {
    match code {
        "SENDER_ID_MISMATCH" => {
            Some("the app on the phone is in another Firebase project than the server's key")
        }
        "PERMISSION_DENIED" => Some(
            "the key may not send messages, or the Firebase Cloud Messaging API is off in its project",
        ),
        _ => None,
    }
}

fn refusal(status: StatusCode, body: &str) -> String {
    let mut text = status.to_string();
    let Ok(reply) = serde_json::from_str::<Value>(body) else {
        return text;
    };
    let error = &reply["error"];
    let code = error["details"]
        .as_array()
        .and_then(|details| details.iter().find_map(|d| d["errorCode"].as_str()))
        .or_else(|| error["status"].as_str())
        .or_else(|| error.as_str());
    let message = error["message"]
        .as_str()
        .or_else(|| reply["error_description"].as_str());
    if let Some(code) = code {
        text = format!("{text}, {code}");
    }
    if let Some(message) = message {
        let line: String = message
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .chars()
            .take(REASON_CHARS)
            .collect();
        text = format!("{text}: {line}");
    }
    if let Some(hint) = code.and_then(hint) {
        text = format!("{text} ({hint})");
    }
    text
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

    async fn access_token(&mut self) -> Result<String, String> {
        if let Some((token, until)) = &self.access
            && Instant::now() < *until
        {
            return Ok(token.clone());
        }
        let assertion = self
            .assertion(crate::now())
            .ok_or("the key could not sign the sign-in to Firebase")?;
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
                let status = reply.status();
                let body = reply.text().await.unwrap_or_default();
                return Err(format!(
                    "Firebase refused the service account: {}",
                    refusal(status, &body)
                ));
            }
            Err(err) => return Err(format!("Firebase sign-in failed: {err}")),
        };
        let token = reply
            .bytes()
            .await
            .ok()
            .and_then(|bytes| serde_json::from_slice::<AccessToken>(&bytes).ok())
            .ok_or("Firebase sign-in gave no access token")?;
        let lifetime = Duration::from_secs(token.expires_in.saturating_sub(60));
        self.access = Some((token.access_token.clone(), Instant::now() + lifetime));
        Ok(token.access_token)
    }

    pub async fn send(&mut self, body: &Value) -> Outcome {
        for first in [true, false] {
            let token = match self.access_token().await {
                Ok(token) => token,
                Err(reason) => return Outcome::Failed(reason),
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
                    return Outcome::Failed(format!("Firebase could not be reached: {err}"));
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
            return Outcome::Failed(format!("Firebase refused it: {}", refusal(status, &text)));
        }
        Outcome::Failed("Firebase refused the access token twice".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fcm_error(status: &str, code: Option<&str>, message: &str) -> String {
        let details: Vec<Value> = code
            .map(|code| {
                json!({ "@type": "type.googleapis.com/google.firebase.fcm.v1.FcmError", "errorCode": code })
            })
            .into_iter()
            .collect();
        json!({ "error": { "code": 403, "message": message, "status": status, "details": details } })
            .to_string()
    }

    #[test]
    fn a_token_from_another_project_says_so() {
        let body = fcm_error(
            "PERMISSION_DENIED",
            Some("SENDER_ID_MISMATCH"),
            "SenderId mismatch",
        );
        assert_eq!(
            refusal(StatusCode::FORBIDDEN, &body),
            "403 Forbidden, SENDER_ID_MISMATCH: SenderId mismatch (the app on the phone is in another Firebase project than the server's key)"
        );
    }

    #[test]
    fn a_key_that_may_not_send_says_so() {
        let body = fcm_error(
            "PERMISSION_DENIED",
            None,
            "Firebase Cloud Messaging API has not been used in project 1 before or it is disabled.",
        );
        assert_eq!(
            refusal(StatusCode::FORBIDDEN, &body),
            "403 Forbidden, PERMISSION_DENIED: Firebase Cloud Messaging API has not been used in project 1 before or it is disabled. (the key may not send messages, or the Firebase Cloud Messaging API is off in its project)"
        );
    }

    #[test]
    fn a_refused_sign_in_gives_googles_reason() {
        let body =
            json!({ "error": "invalid_grant", "error_description": "Invalid JWT Signature." });
        assert_eq!(
            refusal(StatusCode::BAD_REQUEST, &body.to_string()),
            "400 Bad Request, invalid_grant: Invalid JWT Signature."
        );
    }

    #[test]
    fn a_refusal_without_a_body_is_its_status() {
        assert_eq!(
            refusal(StatusCode::BAD_GATEWAY, "<html>oops</html>"),
            "502 Bad Gateway"
        );
    }

    #[test]
    fn a_long_message_stays_one_short_line() {
        let body = fcm_error(
            "INTERNAL",
            None,
            &format!("first\nsecond {}", "x".repeat(400)),
        );
        let text = refusal(StatusCode::INTERNAL_SERVER_ERROR, &body);
        assert!(!text.contains('\n'), "{text}");
        assert!(text.chars().count() < 260, "{text}");
        assert!(text.starts_with("500 Internal Server Error, INTERNAL: first second x"));
    }
}
