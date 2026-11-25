pub const CHALLENGE_LEN: usize = 8;

pub struct ListCipher {
    cards: [u8; 256],
    rotor: u8,
    ratchet: u8,
    avalanche: u8,
    last_plain: u8,
    last_cipher: u8,
}

impl ListCipher {
    pub fn for_list(secret: &[u8], challenge: [u8; CHALLENGE_LEN], server_key: &[u8]) -> Self {
        let mut key = challenge;
        if !secret.is_empty() {
            for (i, &byte) in server_key.iter().enumerate() {
                let at = (i * usize::from(secret[i % secret.len()])) % CHALLENGE_LEN;
                let mixed = key[i % CHALLENGE_LEN] ^ byte;
                key[at] ^= mixed;
            }
        }
        Self::keyed(&key)
    }

    fn keyed(key: &[u8]) -> Self {
        let mut cards = [0u8; 256];
        for (i, card) in cards.iter_mut().enumerate() {
            *card = i as u8;
        }
        let mut shuffle = Shuffle { key, sum: 0, at: 0 };
        for i in (0..=255usize).rev() {
            let other = usize::from(shuffle.pick(&cards, i));
            cards.swap(i, other);
        }
        Self {
            rotor: cards[1],
            ratchet: cards[3],
            avalanche: cards[5],
            last_plain: cards[7],
            last_cipher: cards[usize::from(shuffle.sum)],
            cards,
        }
    }

    pub fn encrypt(&mut self, bytes: &mut [u8]) {
        for byte in bytes {
            let plain = *byte;
            let cipher = plain ^ self.turn();
            self.last_cipher = cipher;
            self.last_plain = plain;
            *byte = cipher;
        }
    }

    pub fn decrypt(&mut self, bytes: &mut [u8]) {
        for byte in bytes {
            let cipher = *byte;
            let plain = cipher ^ self.turn();
            self.last_plain = plain;
            self.last_cipher = cipher;
            *byte = plain;
        }
    }

    fn turn(&mut self) -> u8 {
        let card = |cards: &[u8; 256], at: u8| cards[usize::from(at)];
        self.ratchet = self.ratchet.wrapping_add(card(&self.cards, self.rotor));
        self.rotor = self.rotor.wrapping_add(1);
        let held = card(&self.cards, self.last_cipher);
        self.cards[usize::from(self.last_cipher)] = card(&self.cards, self.ratchet);
        self.cards[usize::from(self.ratchet)] = card(&self.cards, self.last_plain);
        self.cards[usize::from(self.last_plain)] = card(&self.cards, self.rotor);
        self.cards[usize::from(self.rotor)] = held;
        self.avalanche = self.avalanche.wrapping_add(card(&self.cards, held));
        let first = card(&self.cards, self.avalanche).wrapping_add(card(&self.cards, self.rotor));
        let second = card(&self.cards, self.last_plain)
            .wrapping_add(card(&self.cards, self.last_cipher))
            .wrapping_add(card(&self.cards, self.ratchet));
        card(&self.cards, first) ^ card(&self.cards, card(&self.cards, second))
    }
}

struct Shuffle<'a> {
    key: &'a [u8],
    sum: u8,
    at: usize,
}

impl Shuffle<'_> {
    fn pick(&mut self, cards: &[u8; 256], limit: usize) -> u8 {
        if limit == 0 {
            return 0;
        }
        let mut mask = 1usize;
        while mask < limit {
            mask = (mask << 1) + 1;
        }
        let mut tries = 0;
        loop {
            self.sum = cards[usize::from(self.sum)].wrapping_add(self.key[self.at]);
            self.at += 1;
            if self.at >= self.key.len() {
                self.at = 0;
                self.sum = self.sum.wrapping_add(self.key.len() as u8);
            }
            let mut picked = mask & usize::from(self.sum);
            tries += 1;
            if tries > 11 {
                picked %= limit;
            }
            if picked <= limit {
                return picked as u8;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SDK_CIPHER: [u8; 40] = [
        0x93, 0x03, 0x04, 0xEB, 0x0B, 0x91, 0xE1, 0x08, 0x31, 0x2D, 0x1B, 0x5E, 0x95, 0x61, 0x70,
        0xB8, 0x44, 0xB6, 0xB6, 0xB8, 0x06, 0x89, 0x4A, 0x77, 0x5D, 0xA0, 0x04, 0x34, 0xA8, 0xD0,
        0x62, 0x0F, 0xE6, 0xFE, 0x23, 0x67, 0xFD, 0x4C, 0xFA, 0x97,
    ];

    fn plain() -> Vec<u8> {
        (0..40u8)
            .map(|i| i.wrapping_mul(7).wrapping_add(3))
            .collect()
    }

    fn cipher() -> ListCipher {
        ListCipher::for_list(b"yeJ3x8", *b"Abc12345", b"0123456789abc")
    }

    #[test]
    fn the_list_cipher_matches_the_gamespy_sdk() {
        let mut bytes = plain();
        cipher().encrypt(&mut bytes);
        assert_eq!(bytes, SDK_CIPHER);
    }

    #[test]
    fn decrypting_gives_back_the_list_across_pieces() {
        let mut bytes = SDK_CIPHER.to_vec();
        let mut reader = cipher();
        let (head, tail) = bytes.split_at_mut(13);
        reader.decrypt(head);
        reader.decrypt(tail);
        assert_eq!(bytes, plain());
    }

    #[test]
    fn another_challenge_gives_another_stream() {
        let mut ours = plain();
        cipher().encrypt(&mut ours);
        let mut theirs = plain();
        ListCipher::for_list(b"yeJ3x8", *b"Abc12346", b"0123456789abc").encrypt(&mut theirs);
        assert_ne!(ours, theirs);
    }
}
