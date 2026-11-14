#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum FileType {
    Narc,
    Nsbtx,
    Nsbmd,
    Nsbca,
    Nsbtp,
    Nsbta,
    Nsbma,
    Nsbva,
    Ncgr,
    Nclr,
    Nscr,
    Ncer,
    Nanr,
    Sdat,
    Sseq,
    Sbnk,
    Swar,
    Swav,
    Ssar,
    Strm,
    Bmg,
    Text,
    Data,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Group {
    Pictures,
    Models,
    Animations,
    Sound,
    Archives,
    Text,
    Other,
}

impl Group {
    pub const ALL: [Group; 7] = [
        Group::Pictures,
        Group::Models,
        Group::Animations,
        Group::Sound,
        Group::Archives,
        Group::Text,
        Group::Other,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Group::Pictures => "Pictures",
            Group::Models => "Models",
            Group::Animations => "Animations",
            Group::Sound => "Sound",
            Group::Archives => "Archives",
            Group::Text => "Text",
            Group::Other => "Other",
        }
    }
}

const MAGICS: [(&[u8; 4], &[u8; 4], FileType); 21] = [
    (b"NARC", b"CRAN", FileType::Narc),
    (b"BTX0", b"BTX0", FileType::Nsbtx),
    (b"BMD0", b"BMD0", FileType::Nsbmd),
    (b"BCA0", b"BCA0", FileType::Nsbca),
    (b"BTP0", b"BTP0", FileType::Nsbtp),
    (b"BTA0", b"BTA0", FileType::Nsbta),
    (b"BMA0", b"BMA0", FileType::Nsbma),
    (b"BVA0", b"BVA0", FileType::Nsbva),
    (b"RGCN", b"NCGR", FileType::Ncgr),
    (b"RLCN", b"NCLR", FileType::Nclr),
    (b"RCSN", b"NSCR", FileType::Nscr),
    (b"RECN", b"NCER", FileType::Ncer),
    (b"RNAN", b"NANR", FileType::Nanr),
    (b"SDAT", b"SDAT", FileType::Sdat),
    (b"SSEQ", b"SSEQ", FileType::Sseq),
    (b"SBNK", b"SBNK", FileType::Sbnk),
    (b"SWAR", b"SWAR", FileType::Swar),
    (b"SWAV", b"SWAV", FileType::Swav),
    (b"SSAR", b"SSAR", FileType::Ssar),
    (b"STRM", b"STRM", FileType::Strm),
    (b"MESG", b"MESG", FileType::Bmg),
];

impl FileType {
    pub fn detect(bytes: &[u8]) -> FileType {
        if let Some(magic) = bytes.get(..4)
            && let Some(&(_, _, kind)) = MAGICS
                .iter()
                .find(|(a, b, _)| magic == a.as_slice() || magic == b.as_slice())
        {
            return kind;
        }
        if looks_like_text(bytes) {
            FileType::Text
        } else {
            FileType::Data
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            FileType::Narc => "NARC",
            FileType::Nsbtx => "NSBTX",
            FileType::Nsbmd => "NSBMD",
            FileType::Nsbca => "NSBCA",
            FileType::Nsbtp => "NSBTP",
            FileType::Nsbta => "NSBTA",
            FileType::Nsbma => "NSBMA",
            FileType::Nsbva => "NSBVA",
            FileType::Ncgr => "NCGR",
            FileType::Nclr => "NCLR",
            FileType::Nscr => "NSCR",
            FileType::Ncer => "NCER",
            FileType::Nanr => "NANR",
            FileType::Sdat => "SDAT",
            FileType::Sseq => "SSEQ",
            FileType::Sbnk => "SBNK",
            FileType::Swar => "SWAR",
            FileType::Swav => "SWAV",
            FileType::Ssar => "SSAR",
            FileType::Strm => "STRM",
            FileType::Bmg => "BMG",
            FileType::Text => "TEXT",
            FileType::Data => "DATA",
        }
    }

    pub fn group(self) -> Group {
        match self {
            FileType::Nsbtx | FileType::Ncgr | FileType::Nclr | FileType::Nscr | FileType::Ncer => {
                Group::Pictures
            }
            FileType::Nsbmd => Group::Models,
            FileType::Nsbca
            | FileType::Nsbtp
            | FileType::Nsbta
            | FileType::Nsbma
            | FileType::Nsbva
            | FileType::Nanr => Group::Animations,
            FileType::Sdat
            | FileType::Sseq
            | FileType::Sbnk
            | FileType::Swar
            | FileType::Swav
            | FileType::Ssar
            | FileType::Strm => Group::Sound,
            FileType::Narc => Group::Archives,
            FileType::Bmg | FileType::Text => Group::Text,
            FileType::Data => Group::Other,
        }
    }
}

fn looks_like_text(bytes: &[u8]) -> bool {
    let head = &bytes[..bytes.len().min(512)];
    if head.is_empty() {
        return false;
    }
    let text = match std::str::from_utf8(head) {
        Ok(text) => text,
        Err(err) if err.error_len().is_none() && err.valid_up_to() > 0 => {
            std::str::from_utf8(&head[..err.valid_up_to()]).unwrap_or_default()
        }
        Err(_) => return false,
    };
    text.chars()
        .all(|c| !c.is_control() || matches!(c, '\n' | '\r' | '\t'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_type_from_the_magic() {
        assert_eq!(FileType::detect(b"NARC\xFE\xFF"), FileType::Narc);
        assert_eq!(FileType::detect(b"BTX0\xFF\xFE"), FileType::Nsbtx);
        assert_eq!(FileType::detect(b"BMD0\xFF\xFE"), FileType::Nsbmd);
        assert_eq!(FileType::detect(b"RGCN\xFF\xFE"), FileType::Ncgr);
        assert_eq!(FileType::detect(b"NCLR\xFF\xFE"), FileType::Nclr);
        assert_eq!(FileType::detect(b"SDAT\xFF\xFE"), FileType::Sdat);
        assert_eq!(FileType::detect(b"MESGbmg1"), FileType::Bmg);
    }

    #[test]
    fn the_extension_plays_no_part() {
        assert_eq!(
            FileType::detect(&[0x10, 0x20, 0x00, 0x00, 0xFF]),
            FileType::Data
        );
        assert_eq!(FileType::detect(b"hello world\n"), FileType::Text);
        assert_eq!(FileType::detect(b""), FileType::Data);
    }

    #[test]
    fn text_cut_inside_a_character_is_still_text() {
        let text = "Pokémon".repeat(80);
        assert_eq!(FileType::detect(text.as_bytes()), FileType::Text);
    }

    #[test]
    fn every_type_has_one_group() {
        assert_eq!(FileType::Nsbtx.group(), Group::Pictures);
        assert_eq!(FileType::Nsbmd.group(), Group::Models);
        assert_eq!(FileType::Nanr.group(), Group::Animations);
        assert_eq!(FileType::Swar.group(), Group::Sound);
        assert_eq!(FileType::Narc.group(), Group::Archives);
        assert_eq!(FileType::Bmg.group(), Group::Text);
        assert_eq!(FileType::Data.group(), Group::Other);
    }
}
