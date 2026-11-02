use crate::db::{self, Db};
use crate::settings::{hash_password, verify_password};
use qrcode::QrCode;
use qrcode::render::svg;
use totp_rs::{Builder, Secret, Totp};

pub(crate) const RECOVERY_CODES: usize = 10;
const ISSUER: &str = "Citrine";

pub(crate) fn new_secret() -> Vec<u8> {
    let mut secret = vec![0u8; 20];
    getrandom::getrandom(&mut secret).expect("no system random source");
    secret
}

pub(crate) fn totp(secret: &[u8], account: &str) -> Totp {
    Builder::new()
        .with_secret(secret.to_vec())
        .with_issuer(Some(ISSUER))
        .with_account_name(account.replace(':', " "))
        .build_noncompliant()
}

pub(crate) fn key_text(secret: &[u8]) -> String {
    Secret::from(secret)
        .to_base32()
        .as_bytes()
        .chunks(4)
        .map(|c| String::from_utf8_lossy(c).into_owned())
        .collect::<Vec<_>>()
        .join(" ")
}

pub(crate) fn qr_svg(secret: &[u8], account: &str) -> String {
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

pub(crate) fn clean(code: &str) -> Option<String> {
    let digits: String = code
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .collect();
    digits.chars().all(|c| c.is_ascii_digit()).then_some(digits)
}

pub(crate) fn step_of(secret: &[u8], code: &str, now: u64) -> Option<u64> {
    let digits = clean(code)?;
    (digits.len() == 6)
        .then(|| totp(secret, "").check(&digits, now))
        .flatten()
}

pub(crate) fn new_recovery_codes() -> (Vec<String>, Vec<String>) {
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

pub(crate) async fn recovery_code(codes: Vec<(i64, String)>, digits: String) -> Option<i64> {
    tokio::task::spawn_blocking(move || {
        codes
            .into_iter()
            .find(|(_, hash)| verify_password(&digits, hash))
            .map(|(id, _)| id)
    })
    .await
    .ok()
    .flatten()
}

pub(crate) async fn verify_account(
    db: &Db,
    account: i64,
    code: &str,
    now: u64,
) -> db::Result<bool> {
    let Some(secret) = db.account_totp(account).await? else {
        return Ok(false);
    };
    let Some(digits) = clean(code) else {
        return Ok(false);
    };
    match digits.len() {
        6 => match step_of(&secret, &digits, now) {
            Some(step) => db.use_account_step(account, step).await,
            None => Ok(false),
        },
        12 => match recovery_code(db.account_recovery_codes(account).await?, digits).await {
            Some(id) => db.use_account_recovery_code(account, id).await,
            None => Ok(false),
        },
        _ => Ok(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use citrine_gb_net::PublicKey;

    const NOW: u64 = 1_761_782_415;

    fn code(secret: &[u8], at: u64) -> String {
        totp(secret, "").generate(at).to_string()
    }

    async fn account_on() -> (tempfile::TempDir, Db, i64, Vec<u8>, Vec<String>) {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(&dir.path().join("t.db")).await.unwrap();
        let id = db
            .create_account(&[1; 32], &PublicKey([2; 32]), "Clara", 10)
            .await
            .unwrap()
            .id;
        let secret = new_secret();
        let step = step_of(&secret, &code(&secret, NOW - 60), NOW - 60).unwrap();
        let (codes, hashes) = new_recovery_codes();
        db.set_account_totp(id, &secret, step, &hashes)
            .await
            .unwrap();
        (dir, db, id, secret, codes)
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
    async fn an_account_code_is_wrong_replayed_or_right() {
        let (_dir, db, me, secret, _) = account_on().await;
        assert!(!verify_account(&db, me, "000000", NOW).await.unwrap());
        assert!(
            !verify_account(&db, me, &code(&secret, NOW - 60), NOW)
                .await
                .unwrap()
        );
        let now = code(&secret, NOW);
        assert!(verify_account(&db, me, &now, NOW).await.unwrap());
        assert!(!verify_account(&db, me, &now, NOW).await.unwrap());
        assert!(
            !verify_account(&db, me, &code(&secret, NOW - 30), NOW)
                .await
                .unwrap()
        );
        assert!(
            verify_account(&db, me, &code(&secret, NOW + 30), NOW)
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn an_account_recovery_code_works_once() {
        let (_dir, db, me, _, codes) = account_on().await;
        assert!(verify_account(&db, me, &codes[2], NOW).await.unwrap());
        assert!(!verify_account(&db, me, &codes[2], NOW).await.unwrap());
        assert_eq!(
            db.account_codes_left(me).await.unwrap(),
            RECOVERY_CODES as u64 - 1
        );
        assert!(
            verify_account(&db, me, &codes[5].replace('-', " "), NOW)
                .await
                .unwrap()
        );
        assert!(
            !verify_account(&db, me, "1234-5678-9012", NOW)
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn another_accounts_codes_never_work() {
        let (_dir, db, me, secret, codes) = account_on().await;
        let other = db
            .create_account(&[3; 32], &PublicKey([4; 32]), "Ana", 10)
            .await
            .unwrap()
            .id;
        assert!(
            !verify_account(&db, other, &code(&secret, NOW), NOW)
                .await
                .unwrap()
        );
        assert!(!verify_account(&db, other, &codes[0], NOW).await.unwrap());
        assert!(verify_account(&db, me, &codes[0], NOW).await.unwrap());
    }
}
