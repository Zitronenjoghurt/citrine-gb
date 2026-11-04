use crate::debug::print::{PrintLevel, PrintLine, PrintLog};
use crate::gba::wait::Width;

const STRING: u32 = 0x04FF_F600;
const STRING_LEN: usize = 0x100;
const FLAGS: u32 = 0x04FF_F700;
const ENABLE: u32 = 0x04FF_F780;
const ENABLE_KEY: u32 = 0xC0DE;
const ENABLED: u32 = 0x1DEA;
const FLUSH: u32 = 0x100;

#[derive(Clone)]
pub struct DebugPrint {
    enabled: bool,
    text: [u8; STRING_LEN],
    log: PrintLog,
}

impl Default for DebugPrint {
    fn default() -> Self {
        Self {
            enabled: false,
            text: [0; STRING_LEN],
            log: PrintLog::default(),
        }
    }
}

impl DebugPrint {
    pub fn covers(addr: u32) -> bool {
        (STRING..ENABLE + 2).contains(&addr)
    }

    pub fn read(&self, addr: u32) -> Option<u32> {
        (self.enabled && addr & !1 == ENABLE).then_some(ENABLED)
    }

    pub fn write(&mut self, addr: u32, width: Width, value: u32) {
        let aligned = addr & !(width.bytes() as u32 - 1);
        if aligned & !1 == ENABLE {
            self.enabled = value & 0xFFFF == ENABLE_KEY;
            return;
        }
        if !self.enabled {
            return;
        }
        if aligned & !1 == FLAGS {
            if value & FLUSH != 0 {
                self.flush(PrintLevel::from_bits(value));
            }
            return;
        }
        let start = (aligned - STRING) as usize;
        for (index, byte) in value.to_le_bytes()[..width.bytes()].iter().enumerate() {
            if let Some(slot) = self.text.get_mut(start + index) {
                *slot = *byte;
            }
        }
    }

    fn flush(&mut self, level: PrintLevel) {
        let end = self
            .text
            .iter()
            .position(|&byte| byte == 0)
            .unwrap_or(STRING_LEN);
        let text = String::from_utf8_lossy(&self.text[..end]).into_owned();
        self.log.push(PrintLine { level, text });
        self.text.fill(0);
    }

    pub fn log(&self) -> &PrintLog {
        &self.log
    }

    pub fn log_mut(&mut self) -> &mut PrintLog {
        &mut self.log
    }
}
