use crate::utils::file_loader::PickedFile;
use citrine_gb::persistence::rtc_footer::RtcFooter;

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum FileKind {
    Rom,
    Save,
    Clock,
    Other,
}

pub fn kind_of(name: &str) -> FileKind {
    let extension = std::path::Path::new(name)
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase());
    match extension.as_deref() {
        Some("gb" | "gbc") => FileKind::Rom,
        Some("sav" | "srm") => FileKind::Save,
        Some("rtc") => FileKind::Clock,
        _ => FileKind::Other,
    }
}

pub struct SaveFiles {
    pub save: Option<PickedFile>,
    pub clock: Option<(String, RtcFooter)>,
}

pub fn sort(files: Vec<PickedFile>) -> Result<SaveFiles, String> {
    let mut save = None;
    let mut clock = None;
    for file in files {
        let is_clock = kind_of(&file.name) == FileKind::Clock;
        let taken = if is_clock {
            clock.is_some()
        } else {
            save.is_some()
        };
        if taken {
            return Err("Pick one save file and at most one .rtc file".to_string());
        }
        if is_clock {
            let footer = RtcFooter::decode_rtc_file(&file.data)
                .ok_or_else(|| format!("'{}' is not a SameBoy .rtc file", file.name))?;
            clock = Some((file.name, footer));
        } else {
            save = Some(file);
        }
    }
    Ok(SaveFiles { save, clock })
}

#[cfg(test)]
mod tests {
    use super::*;
    use citrine_gb::gb::cartridge::RtcRegisters;

    fn file(name: &str, data: &[u8]) -> PickedFile {
        PickedFile {
            name: name.to_string(),
            data: data.to_vec(),
            #[cfg(not(target_arch = "wasm32"))]
            path: None,
        }
    }

    fn rtc_bytes() -> Vec<u8> {
        RtcFooter {
            real: RtcRegisters::from_bytes([26, 19, 5, 1, 0]),
            latched: RtcRegisters::from_bytes([26, 19, 5, 1, 0]),
            timestamp: 1_792_890_553,
        }
        .encode_rtc_file()
        .to_vec()
    }

    #[test]
    fn kinds_follow_the_extension_in_any_case() {
        assert_eq!(kind_of("Pokemon Silver.GBC"), FileKind::Rom);
        assert_eq!(kind_of("silver.srm"), FileKind::Save);
        assert_eq!(kind_of("silver.SAV"), FileKind::Save);
        assert_eq!(kind_of("silver.rtc"), FileKind::Clock);
        assert_eq!(kind_of("notes.txt"), FileKind::Other);
        assert_eq!(kind_of("rtc"), FileKind::Other);
    }

    #[test]
    fn pairs_a_save_with_its_clock_in_either_order() {
        let sorted = sort(vec![
            file("silver.rtc", &rtc_bytes()),
            file("silver.srm", b"ram"),
        ])
        .expect("sorted");
        assert_eq!(sorted.save.expect("save").data, b"ram");
        let (name, footer) = sorted.clock.expect("clock");
        assert_eq!(name, "silver.rtc");
        assert_eq!(footer.real.to_bytes(), [26, 19, 5, 1, 0]);
        assert_eq!(footer.timestamp, 1_792_890_553);
    }

    #[test]
    fn takes_a_save_or_a_clock_alone() {
        let save = sort(vec![file("game.sav", b"ram")]).expect("save");
        assert!(save.save.is_some() && save.clock.is_none());
        let clock = sort(vec![file("game.rtc", &rtc_bytes())]).expect("clock");
        assert!(clock.save.is_none() && clock.clock.is_some());
    }

    #[test]
    fn refuses_two_saves_or_two_clocks() {
        assert!(sort(vec![file("a.sav", b""), file("b.srm", b"")]).is_err());
        assert!(
            sort(vec![
                file("a.rtc", &rtc_bytes()),
                file("b.rtc", &rtc_bytes())
            ])
            .is_err()
        );
    }

    #[test]
    fn refuses_an_rtc_file_of_the_wrong_size() {
        let err = sort(vec![file("game.rtc", &[0; 48])]).err().expect("error");
        assert_eq!(err, "'game.rtc' is not a SameBoy .rtc file");
    }
}
