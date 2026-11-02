const BASE32: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

pub fn secret_text(secret: &[u8]) -> String {
    let mut out = String::new();
    let mut buffer = 0u32;
    let mut bits = 0;
    for &byte in secret {
        buffer = (buffer << 8) | u32::from(byte);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(BASE32[((buffer >> bits) & 31) as usize] as char);
        }
    }
    if bits > 0 {
        out.push(BASE32[((buffer << (5 - bits)) & 31) as usize] as char);
    }
    out
}

pub fn grouped(text: &str) -> String {
    text.as_bytes()
        .chunks(4)
        .map(|c| String::from_utf8_lossy(c).into_owned())
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn otpauth_url(secret: &[u8], account: &str) -> String {
    let name: String = account
        .replace(':', " ")
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect();
    format!(
        "otpauth://totp/Citrine:{name}?secret={}&issuer=Citrine",
        secret_text(secret)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_secret_reads_as_rfc_4648_base32() {
        assert_eq!(secret_text(b"foobar"), "MZXW6YTBOI");
        assert_eq!(secret_text(b"12345678901234567890").len(), 32);
        assert_eq!(grouped("JBSWY3DPEHPK3PXP"), "JBSW Y3DP EHPK 3PXP");
    }

    #[test]
    fn the_app_link_names_the_account() {
        assert_eq!(
            otpauth_url(b"foobar", "Clara Z:1"),
            "otpauth://totp/Citrine:Clara%20Z%201?secret=MZXW6YTBOI&issuer=Citrine"
        );
    }
}
