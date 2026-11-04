use citrine_gb_gamedata::RegionMap;
use std::path::Path;

pub fn map_picture(path: &Path, map: &RegionMap, marks: &[(u16, u16)]) {
    let scale = 3;
    let (width, height) = (map.width * scale, map.height * scale);
    let mut rgb = vec![0u8; width * height * 3];
    for y in 0..height {
        for x in 0..width {
            let [r, g, b, _] = map.pixel(x / scale, y / scale);
            rgb[(y * width + x) * 3..][..3].copy_from_slice(&[r, g, b]);
        }
    }
    for &(mx, my) in marks {
        let (cx, cy) = (
            (usize::from(mx) * scale + scale / 2) as i64,
            (usize::from(my) * scale + scale / 2) as i64,
        );
        for y in 0..height as i64 {
            for x in 0..width as i64 {
                let d = (x - cx).pow(2) + (y - cy).pow(2);
                if (64..=121).contains(&d) || d <= 4 {
                    rgb[(y as usize * width + x as usize) * 3..][..3]
                        .copy_from_slice(&[0xFF, 0x00, 0x60]);
                }
            }
        }
    }
    std::fs::write(path, png(width, height, &rgb)).expect("write map");
}

pub fn png(width: usize, height: usize, rgb: &[u8]) -> Vec<u8> {
    let mut raw = Vec::with_capacity((width * 3 + 1) * height);
    for row in rgb.chunks(width * 3) {
        raw.push(0);
        raw.extend_from_slice(row);
    }
    let mut zlib = vec![0x78, 0x01];
    let blocks: Vec<&[u8]> = raw.chunks(0xFFFF).collect();
    for (i, block) in blocks.iter().enumerate() {
        zlib.push(u8::from(i + 1 == blocks.len()));
        let len = block.len() as u16;
        zlib.extend(len.to_le_bytes());
        zlib.extend((!len).to_le_bytes());
        zlib.extend_from_slice(block);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &byte in &raw {
        a = (a + u32::from(byte)) % 65521;
        b = (b + a) % 65521;
    }
    zlib.extend(((b << 16) | a).to_be_bytes());
    let mut header = Vec::new();
    header.extend((width as u32).to_be_bytes());
    header.extend((height as u32).to_be_bytes());
    header.extend([8, 2, 0, 0, 0]);
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    for (kind, body) in [(b"IHDR", header), (b"IDAT", zlib), (b"IEND", Vec::new())] {
        out.extend((body.len() as u32).to_be_bytes());
        let mut chunk = kind.to_vec();
        chunk.extend(&body);
        out.extend(&chunk);
        out.extend(crc32(&chunk).to_be_bytes());
    }
    out
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &byte in bytes {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}
