use crate::db::{self, Db};
use crate::settings::{hash_password, verify_password};
use qrcode::QrCode;
use qrcode::render::svg;
use totp_rs::{Builder, Secret, Totp};

pub const RECOVERY_CODES: usize = 10;
const ISSUER: &str = "Citrine";

pub fn new_secret() -> Vec<u8> {
    let mut secret = vec![0u8; 20];
    getrandom::getrandom(&mut secret).expect("no system random source");
    secret
}

fn totp(secret: &[u8], account: &str) -> Totp {
    Builder::new()
        .with_secret(secret.to_vec())
        .with_issuer(Some(ISSUER))
        .with_account_name(account.replace(':', " "))
        .build_noncompliant()
}

pub fn key_text(secret: &[u8]) -> String {
    Secret::from(secret)
        .to_base32()
        .as_bytes()
        .chunks(4)
        .map(|c| String::from_utf8_lossy(c).into_owned())
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn qr_svg(secret: &[u8], account: &str) -> String {
    let Ok(url) = totp(secret, account).to_url() else {
        return String::new();
    };
    let Ok(code) = QrCode::new(url.as_bytes()) else {
        return String::new();
    };
    let svg = code
        .render::<svg::Color>()
        .min_dimensions(184, 184)
        .dark_color(svg::Color("#000000"))
        .light_color(svg::Color("#ffffff"))
        .build();
    match svg.find("<svg") {
        Some(start) => svg[start..].to_string(),
        None => svg,
    }
}

fn clean(code: &str) -> Option<String> {
    let digits: String = code
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .collect();
    digits.chars().all(|c| c.is_ascii_digit()).then_some(digits)
}

pub fn step_of(secret: &[u8], code: &str, now: u64) -> Option<u64> {
    let digits = clean(code)?;
    (digits.len() == 6)
        .then(|| totp(secret, "").check(&digits, now))
        .flatten()
}

pub fn new_recovery_codes() -> (Vec<String>, Vec<String>) {
    let codes: Vec<String> = (0..RECOVERY_CODES)
        .map(|_| {
            let mut raw = [0u8; 8];
            getrandom::getrandom(&mut raw).expect("no system random source");
            let n = u64::from_le_bytes(raw) % 1_000_000_000_000;
            let digits = format!("{n:012}");
            format!("{}-{}-{}", &digits[..4], &digits[4..8], &digits[8..])
        })
        .collect();
    let hashes = codes
        .iter()
        .filter_map(|c| clean(c))
        .map(|c| hash_password(&c))
        .collect();
    (codes, hashes)
}

pub async fn is_on(db: &Db) -> bool {
    matches!(db.admin_totp().await, Ok(Some(_)))
}

pub async fn codes_left(db: &Db) -> usize {
    db.admin_recovery_codes()
        .await
        .map(|c| c.len())
        .unwrap_or(0)
}

pub async fn verify(db: &Db, code: &str, now: u64) -> db::Result<bool> {
    let Some(secret) = db.admin_totp().await? else {
        return Ok(false);
    };
    let Some(digits) = clean(code) else {
        return Ok(false);
    };
    match digits.len() {
        6 => match step_of(&secret, &digits, now) {
            Some(step) => db.use_admin_step(step).await,
            None => Ok(false),
        },
        12 => {
            let codes = db.admin_recovery_codes().await?;
            let found = tokio::task::spawn_blocking(move || {
                codes
                    .into_iter()
                    .find(|(_, hash)| verify_password(&digits, hash))
                    .map(|(id, _)| id)
            })
            .await
            .ok()
            .flatten();
            match found {
                Some(id) => db.use_admin_recovery_code(id).await,
                None => Ok(false),
            }
        }
        _ => Ok(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 1_761_782_415;

    async fn db() -> (tempfile::TempDir, Db) {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(&dir.path().join("t.db")).await.unwrap();
        (dir, db)
    }

    fn code(secret: &[u8], at: u64) -> String {
        totp(secret, "").generate(at).to_string()
    }

    async fn turned_on(db: &Db) -> (Vec<u8>, Vec<String>) {
        let secret = new_secret();
        let step = step_of(&secret, &code(&secret, NOW - 60), NOW - 60).unwrap();
        let (codes, hashes) = new_recovery_codes();
        db.set_admin_totp(&secret, step, &hashes).await.unwrap();
        (secret, codes)
    }

    #[test]
    fn rfc_6238_sha1_vector() {
        let secret = b"12345678901234567890";
        assert_eq!(code(secret, 59), "287082");
        assert_eq!(code(secret, 1_111_111_109), "081804");
    }

    #[test]
    fn setup_key_and_qr_carry_the_secret() {
        let secret = new_secret();
        let key = key_text(&secret);
        assert_eq!(
            Secret::try_from_base32(key.replace(' ', ""))
                .unwrap()
                .as_ref(),
            secret.as_slice()
        );
        let svg = qr_svg(&secret, "gb.example.com");
        assert!(svg.starts_with("<svg"));
        assert!(svg.contains("<path"));
    }

    #[tokio::test]
    async fn one_step_of_drift_each_way() {
        let (_dir, db) = db().await;
        let (secret, _) = turned_on(&db).await;
        assert!(!verify(&db, &code(&secret, NOW - 60), NOW).await.unwrap());
        assert!(verify(&db, &code(&secret, NOW - 30), NOW).await.unwrap());
        assert!(verify(&db, &code(&secret, NOW), NOW).await.unwrap());
        assert!(verify(&db, &code(&secret, NOW + 30), NOW).await.unwrap());
        assert!(!verify(&db, &code(&secret, NOW + 60), NOW).await.unwrap());
    }

    #[tokio::test]
    async fn a_used_code_is_refused() {
        let (_dir, db) = db().await;
        let (secret, _) = turned_on(&db).await;
        let now = code(&secret, NOW);
        assert!(verify(&db, &now, NOW).await.unwrap());
        assert!(!verify(&db, &now, NOW).await.unwrap());
        assert!(!verify(&db, &now, NOW + 20).await.unwrap());
        assert!(!verify(&db, &code(&secret, NOW - 30), NOW).await.unwrap());
        let spaced = code(&secret, NOW + 30);
        let spaced = format!("{} {}", &spaced[..3], &spaced[3..]);
        assert!(verify(&db, &spaced, NOW + 30).await.unwrap());
    }

    #[tokio::test]
    async fn a_recovery_code_works_once() {
        let (_dir, db) = db().await;
        let (_, codes) = turned_on(&db).await;
        assert_eq!(codes.len(), RECOVERY_CODES);
        assert_eq!(codes_left(&db).await, RECOVERY_CODES);
        assert!(codes.iter().all(|c| c.len() == 14));
        assert!(verify(&db, &codes[3], NOW).await.unwrap());
        assert!(!verify(&db, &codes[3], NOW).await.unwrap());
        assert_eq!(codes_left(&db).await, RECOVERY_CODES - 1);
        assert!(verify(&db, &codes[4].replace('-', ""), NOW).await.unwrap());
        assert!(!verify(&db, "1234-5678-9012", NOW).await.unwrap());
        assert!(!verify(&db, "abcdef", NOW).await.unwrap());
        assert!(!verify(&db, "", NOW).await.unwrap());
    }

    #[tokio::test]
    async fn off_accepts_nothing_and_clearing_turns_it_off() {
        let (_dir, db) = db().await;
        assert!(!is_on(&db).await);
        assert!(!verify(&db, "123456", NOW).await.unwrap());
        turned_on(&db).await;
        assert!(is_on(&db).await);
        assert!(db.clear_admin_totp().await.unwrap());
        assert!(!is_on(&db).await);
        assert_eq!(codes_left(&db).await, 0);
        assert!(!db.clear_admin_totp().await.unwrap());
    }
}
