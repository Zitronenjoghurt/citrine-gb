use crate::{ErrorCode, MAX_SCREEN_BYTES};

pub const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";

pub const MAX_SCREEN_SIDE: u32 = 1024;

pub fn screen_size(png: &[u8]) -> Option<(u32, u32)> {
    let header = png.strip_prefix(PNG_SIGNATURE)?.get(..16)?;
    if &header[4..8] != b"IHDR" {
        return None;
    }
    let width = u32::from_be_bytes([header[8], header[9], header[10], header[11]]);
    let height = u32::from_be_bytes([header[12], header[13], header[14], header[15]]);
    Some((width, height))
}

pub fn check_screen(png: &[u8]) -> Result<(), ErrorCode> {
    if png.len() > MAX_SCREEN_BYTES {
        return Err(ErrorCode::TooLarge);
    }
    let sides = 1..=MAX_SCREEN_SIDE;
    match screen_size(png) {
        Some((width, height)) if sides.contains(&width) && sides.contains(&height) => Ok(()),
        _ => Err(ErrorCode::BadMessage),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(width: u32, height: u32, len: usize) -> Vec<u8> {
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
        png.extend_from_slice(&width.to_be_bytes());
        png.extend_from_slice(&height.to_be_bytes());
        png.resize(len, 0);
        png
    }

    #[test]
    fn every_console_shape_goes_through() {
        for (width, height) in [
            (160, 144),
            (320, 288),
            (640, 576),
            (240, 240),
            (384, 384),
            (256, 384),
        ] {
            assert_eq!(
                check_screen(&png(width, height, 200)),
                Ok(()),
                "{width}x{height}"
            );
        }
    }

    #[test]
    fn a_screen_outside_the_limits_is_refused() {
        assert_eq!(
            check_screen(&png(384, 384, MAX_SCREEN_BYTES + 1)),
            Err(ErrorCode::TooLarge)
        );
        for (width, height) in [(0, 144), (160, 0), (1025, 1024), (1024, 1025)] {
            assert_eq!(
                check_screen(&png(width, height, 200)),
                Err(ErrorCode::BadMessage),
                "{width}x{height}"
            );
        }
        assert_eq!(check_screen(&[2; 100]), Err(ErrorCode::BadMessage));
        assert_eq!(check_screen(&png(1024, 1024, 200)), Ok(()));
    }
}
