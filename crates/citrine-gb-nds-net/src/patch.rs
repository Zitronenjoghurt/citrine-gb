use citrine_gb::gba::wait::Width;
use citrine_gb::nds::machine::Nds;

const MAIN_RAM: u32 = 0x0200_0000;
const HTTPS: &[u8] = b"https://";
const DOMAINS: &[&[u8]] = &[b"nintendowifi.net", b"pkvldtprod.nintendo.co.jp"];
const LONGEST_URL: usize = 256;

pub fn plain_http_spots(ram: &[u8]) -> Vec<(usize, Vec<u8>)> {
    let mut spots = Vec::new();
    let mut at = 0;
    while let Some(found) = find(&ram[at..], HTTPS) {
        let start = at + found;
        at = start + HTTPS.len();
        let rest = &ram[at..ram.len().min(at + LONGEST_URL)];
        let Some(end) = rest.iter().position(|&byte| byte == 0) else {
            continue;
        };
        let url = &rest[..end];
        if url.is_empty() || !DOMAINS.iter().any(|domain| find(url, domain).is_some()) {
            continue;
        }
        let mut plain = b"http://".to_vec();
        plain.extend_from_slice(url);
        plain.extend_from_slice(&[0, 0]);
        spots.push((start, plain));
    }
    spots
}

pub fn plain_http(nds: &mut Nds) -> usize {
    let spots = plain_http_spots(nds.buses().main_ram());
    let mut arm9 = nds.buses_mut().arm9();
    for (offset, bytes) in &spots {
        for (index, &byte) in bytes.iter().enumerate() {
            let addr = MAIN_RAM + u32::try_from(offset + index).unwrap_or_default();
            arm9.write(addr, Width::Byte, u32::from(byte));
        }
    }
    spots.len()
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_nintendo_urls_lose_their_s_and_keep_their_length() {
        let mut ram = vec![0xAA; 64];
        ram.extend_from_slice(b"https://nas.nintendowifi.net/ac\0");
        ram.extend_from_slice(b"https://\0");
        ram.extend_from_slice(b"https://example.com/\0");
        ram.extend_from_slice(b"https://nas.nintendowifi.net/pr\0");
        ram.extend_from_slice(b"https://pkvldtprod.nintendo.co.jp/pokemon/validate\0");
        ram.extend_from_slice(b"https://www.nintendo.co.jp/\0");
        let spots = plain_http_spots(&ram);
        assert_eq!(
            spots,
            vec![
                (64, b"http://nas.nintendowifi.net/ac\0\0".to_vec()),
                (
                    64 + 32 + 9 + 21,
                    b"http://nas.nintendowifi.net/pr\0\0".to_vec()
                ),
                (
                    64 + 32 + 9 + 21 + 32,
                    b"http://pkvldtprod.nintendo.co.jp/pokemon/validate\0\0".to_vec()
                ),
            ]
        );
        assert_eq!(spots[0].1.len(), b"https://nas.nintendowifi.net/ac\0".len());
    }

    #[test]
    fn a_url_with_no_end_is_left_alone() {
        let mut ram = b"https://nas.nintendowifi.net".to_vec();
        ram.extend_from_slice(&[b'a'; 300]);
        assert!(plain_http_spots(&ram).is_empty());
    }
}
