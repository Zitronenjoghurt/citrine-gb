use super::autosaves::autosave_thumbnail;
use super::battery::battery_meta;
use super::{BATTERY, BATTERY_META, SaveStore};
#[cfg(any(target_arch = "wasm32", test))]
use super::{decode_meta, encode_meta};
#[cfg(any(target_arch = "wasm32", test))]
use base64::Engine;
use citrine_gb::console::ConsoleKind;
use citrine_gb::persistence::battery_save::BatterySave;

#[cfg(any(target_arch = "wasm32", test))]
const VERSION: u8 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrashSave {
    pub rom_key: String,
    pub saved_at: u64,
    pub console: ConsoleKind,
    pub state: Vec<u8>,
    pub thumb: Vec<u8>,
    pub battery: Option<(Vec<u8>, Vec<u8>)>,
}

impl CrashSave {
    pub fn new(
        rom_key: String,
        saved_at: u64,
        console: ConsoleKind,
        state: Vec<u8>,
        frame_rgba: &[u8],
        battery: Option<&BatterySave>,
    ) -> Self {
        Self {
            rom_key,
            saved_at,
            console,
            state,
            thumb: autosave_thumbnail(frame_rgba, console),
            battery: battery.map(|save| (save.to_file(), battery_meta(save, saved_at))),
        }
    }

    #[cfg(any(target_arch = "wasm32", test))]
    pub fn encode(&self) -> String {
        let mut out = vec![VERSION];
        out.extend(self.saved_at.to_le_bytes());
        out.push(encode_meta(0, self.console)[8]);
        for part in [self.rom_key.as_bytes(), &self.state, &self.thumb] {
            push_part(&mut out, part);
        }
        match &self.battery {
            Some((file, meta)) => {
                out.push(1);
                push_part(&mut out, file);
                push_part(&mut out, meta);
            }
            None => out.push(0),
        }
        base64::engine::general_purpose::STANDARD.encode(out)
    }

    #[cfg(any(target_arch = "wasm32", test))]
    pub fn decode(raw: &str) -> Option<Self> {
        let data = base64::engine::general_purpose::STANDARD.decode(raw).ok()?;
        let mut reader = Reader(&data);
        if reader.take(1)? != [VERSION] {
            return None;
        }
        let saved_at = u64::from_le_bytes(reader.take(8)?.try_into().ok()?);
        let mut meta = [0; 9];
        meta[8] = reader.take(1)?[0];
        let console = decode_meta(&meta).1?;
        let rom_key = String::from_utf8(reader.part()?.to_vec()).ok()?;
        let state = reader.part()?.to_vec();
        let thumb = reader.part()?.to_vec();
        let battery = match reader.take(1)? {
            [1] => Some((reader.part()?.to_vec(), reader.part()?.to_vec())),
            [0] => None,
            _ => return None,
        };
        reader.0.is_empty().then_some(Self {
            rom_key,
            saved_at,
            console,
            state,
            thumb,
            battery,
        })
    }
}

#[cfg(any(target_arch = "wasm32", test))]
fn push_part(out: &mut Vec<u8>, part: &[u8]) {
    out.extend(u32::try_from(part.len()).unwrap_or(u32::MAX).to_le_bytes());
    out.extend(part);
}

#[cfg(any(target_arch = "wasm32", test))]
struct Reader<'a>(&'a [u8]);

#[cfg(any(target_arch = "wasm32", test))]
impl<'a> Reader<'a> {
    fn take(&mut self, count: usize) -> Option<&'a [u8]> {
        if self.0.len() < count {
            return None;
        }
        let (head, rest) = self.0.split_at(count);
        self.0 = rest;
        Some(head)
    }

    fn part(&mut self) -> Option<&'a [u8]> {
        let len = u32::from_le_bytes(self.take(4)?.try_into().ok()?);
        self.take(usize::try_from(len).ok()?)
    }
}

impl SaveStore {
    pub fn store_crash_save(&self, crash: &CrashSave, keep: usize) -> std::io::Result<usize> {
        let battery = match &crash.battery {
            Some((file, meta)) => self.write_battery_files(&crash.rom_key, file, meta),
            None => Ok(()),
        };
        let id = self.store_autosave_at(
            &crash.rom_key,
            &crash.state,
            &crash.thumb,
            crash.console,
            crash.saved_at,
            keep,
        )?;
        battery.map(|()| id)
    }

    #[cfg(any(target_arch = "wasm32", test))]
    pub fn recover_crash_save(&self, crash: &CrashSave) -> std::io::Result<bool> {
        let rom_key = &crash.rom_key;
        let mut taken = false;
        if let Some((file, meta)) = &crash.battery
            && self
                .battery_saved_at(rom_key)
                .is_none_or(|at| at < crash.saved_at)
        {
            self.write_battery_files(rom_key, file, meta)?;
            taken = true;
        }
        if self
            .newest_autosave_at(rom_key)
            .is_none_or(|at| at < crash.saved_at)
        {
            let id = self.next_autosave_id(rom_key);
            self.write_autosave(
                rom_key,
                id,
                &crash.state,
                &crash.thumb,
                crash.console,
                crash.saved_at,
            )?;
            taken = true;
        }
        Ok(taken)
    }

    fn write_battery_files(&self, rom_key: &str, file: &[u8], meta: &[u8]) -> std::io::Result<()> {
        self.write(rom_key, BATTERY, file)?;
        self.write(rom_key, BATTERY_META, meta)
    }
}

#[cfg(test)]
mod tests;
