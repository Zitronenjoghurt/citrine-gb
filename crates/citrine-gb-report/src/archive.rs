use crate::inputs::{self, FrameInput};
use crate::manifest::{FORMAT, Manifest, ScreenInfo};
use crate::screen::Screen;
use std::io::{Cursor, Read, Write};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

const MANIFEST: &str = "report.json";
const STATE: &str = "state.bin";
const INPUTS: &str = "inputs.txt";
const SAVE: &str = "save.sav";
const SETTINGS: &str = "settings.json";
const LOG: &str = "log.txt";
const TRAFFIC: &str = "traffic.txt";

#[derive(Debug, thiserror::Error)]
pub enum ReportError {
    #[error("zip: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("report.json: {0}")]
    Manifest(#[from] serde_json::Error),
    #[error("png: {0}")]
    Png(#[from] png::EncodingError),
    #[error("console: {0}")]
    Console(#[from] citrine_gb::error::GbError),
    #[error("the report has no report.json")]
    NoManifest,
    #[error("report format {0} is newer than this reader")]
    NewerFormat(u32),
    #[error("inputs.txt has a line this reader can't read")]
    BadInputs,
    #[error("the report has no state to replay from")]
    NoReplay,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Report {
    pub manifest: Manifest,
    pub state: Option<Vec<u8>>,
    pub inputs: Vec<FrameInput>,
    pub save: Option<Vec<u8>>,
    pub screens: Vec<Screen>,
    pub settings: Option<String>,
    pub log: String,
    pub traffic: String,
}

impl Report {
    pub fn to_zip(&self) -> Result<Vec<u8>, ReportError> {
        let mut manifest = self.manifest.clone();
        manifest.format = FORMAT;
        let mut files: Vec<(String, &[u8], bool)> = Vec::new();
        let inputs = inputs::encode(&self.inputs);
        if let Some(state) = &self.state {
            files.push((STATE.into(), state, false));
            files.push((INPUTS.into(), inputs.as_bytes(), true));
        }
        if let Some(save) = &self.save {
            files.push((SAVE.into(), save, true));
        }
        manifest.screens = Vec::new();
        for (index, screen) in self.screens.iter().enumerate() {
            let file = format!("screens/{index}.png");
            manifest.screens.push(ScreenInfo {
                file: file.clone(),
                seconds_before: screen.seconds_before,
            });
            files.push((file, &screen.png, false));
        }
        if let Some(settings) = &self.settings {
            files.push((SETTINGS.into(), settings.as_bytes(), true));
        }
        files.push((LOG.into(), self.log.as_bytes(), true));
        files.push((TRAFFIC.into(), self.traffic.as_bytes(), true));
        manifest.files = files.iter().map(|(name, _, _)| name.clone()).collect();
        let json = serde_json::to_vec_pretty(&manifest)?;

        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        zip.start_file(MANIFEST, deflated)?;
        zip.write_all(&json)?;
        for (name, bytes, deflate) in files {
            zip.start_file(name, if deflate { deflated } else { stored })?;
            zip.write_all(bytes)?;
        }
        Ok(zip.finish()?.into_inner())
    }

    pub fn from_zip(bytes: &[u8]) -> Result<Self, ReportError> {
        let mut zip = ZipArchive::new(Cursor::new(bytes))?;
        let manifest: Manifest = match read(&mut zip, MANIFEST)? {
            Some(json) => serde_json::from_slice(&json)?,
            None => return Err(ReportError::NoManifest),
        };
        if manifest.format > FORMAT {
            return Err(ReportError::NewerFormat(manifest.format));
        }
        let inputs = match read_text(&mut zip, INPUTS)? {
            Some(text) => inputs::decode(&text).ok_or(ReportError::BadInputs)?,
            None => Vec::new(),
        };
        let mut screens = Vec::new();
        for info in &manifest.screens {
            if let Some(png) = read(&mut zip, &info.file)? {
                screens.push(Screen {
                    seconds_before: info.seconds_before,
                    png,
                });
            }
        }
        Ok(Self {
            state: read(&mut zip, STATE)?,
            inputs,
            save: read(&mut zip, SAVE)?,
            screens,
            settings: read_text(&mut zip, SETTINGS)?,
            log: read_text(&mut zip, LOG)?.unwrap_or_default(),
            traffic: read_text(&mut zip, TRAFFIC)?.unwrap_or_default(),
            manifest,
        })
    }
}

fn read(zip: &mut ZipArchive<Cursor<&[u8]>>, name: &str) -> Result<Option<Vec<u8>>, ReportError> {
    let mut file = match zip.by_name(name) {
        Ok(file) => file,
        Err(zip::result::ZipError::FileNotFound) => return Ok(None),
        Err(err) => return Err(err.into()),
    };
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    Ok(Some(bytes))
}

fn read_text(
    zip: &mut ZipArchive<Cursor<&[u8]>>,
    name: &str,
) -> Result<Option<String>, ReportError> {
    Ok(read(zip, name)?.map(|bytes| String::from_utf8_lossy(&bytes).into_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{GameInfo, ReplayInfo};
    use citrine_gb::console::{Keys, Touch};

    fn report() -> Report {
        Report {
            manifest: Manifest {
                created_unix: 1_790_000_000,
                game: GameInfo {
                    console: "nds".into(),
                    title: "POKEMON HG".into(),
                    code: Some("IPKE".into()),
                    crc32: "0123abcd".into(),
                    sha1: "ff".repeat(20),
                    frames_played: 4200,
                },
                replay: Some(ReplayInfo {
                    state_unix: 1_789_999_990,
                    frames: 2,
                    end_digest: "00000000deadbeef".into(),
                }),
                ..Manifest::default()
            },
            state: Some(vec![1, 2, 3]),
            inputs: vec![
                FrameInput {
                    keys: Keys::A,
                    touch: None,
                    reset: false,
                },
                FrameInput {
                    keys: Keys::empty(),
                    touch: Some(Touch { x: 3, y: 4 }),
                    reset: true,
                },
            ],
            save: Some(vec![0xFF; 64]),
            screens: vec![Screen::from_rgba(&[255; 4 * 4 * 2], 4, 2, 1).unwrap()],
            settings: Some("{\"volume\":1}".into()),
            log: "INFO loaded\n".into(),
            traffic: "frame  device  event\n".into(),
        }
    }

    #[test]
    fn a_report_round_trips_through_its_zip() {
        let report = report();
        let read = Report::from_zip(&report.to_zip().unwrap()).unwrap();
        assert_eq!(read.state, report.state);
        assert_eq!(read.inputs, report.inputs);
        assert_eq!(read.save, report.save);
        assert_eq!(read.screens, report.screens);
        assert_eq!(read.settings, report.settings);
        assert_eq!(read.log, report.log);
        assert_eq!(read.traffic, report.traffic);
        assert_eq!(read.manifest.game, report.manifest.game);
        assert_eq!(read.manifest.replay, report.manifest.replay);
        assert_eq!(read.manifest.format, FORMAT);
        assert_eq!(
            read.manifest.files,
            [
                "state.bin",
                "inputs.txt",
                "save.sav",
                "screens/0.png",
                "settings.json",
                "log.txt",
                "traffic.txt"
            ]
        );
    }

    #[test]
    fn a_newer_format_is_refused() {
        let mut manifest = Manifest {
            format: FORMAT + 1,
            ..Manifest::default()
        };
        manifest.files.clear();
        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        zip.start_file(MANIFEST, SimpleFileOptions::default())
            .unwrap();
        zip.write_all(&serde_json::to_vec(&manifest).unwrap())
            .unwrap();
        let bytes = zip.finish().unwrap().into_inner();
        assert!(matches!(
            Report::from_zip(&bytes),
            Err(ReportError::NewerFormat(_))
        ));
    }
}
