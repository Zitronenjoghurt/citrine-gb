use std::collections::VecDeque;

const MAX_LINES: usize = 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrintLevel {
    Fatal,
    Error,
    Warn,
    Info,
    Debug,
}

impl PrintLevel {
    pub fn from_bits(bits: u32) -> Self {
        match bits & 0b111 {
            0 => Self::Fatal,
            1 => Self::Error,
            2 => Self::Warn,
            3 => Self::Info,
            _ => Self::Debug,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Fatal => "Fatal",
            Self::Error => "Error",
            Self::Warn => "Warn",
            Self::Info => "Info",
            Self::Debug => "Debug",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrintLine {
    pub level: PrintLevel,
    pub text: String,
}

#[derive(Debug, Default, Clone)]
pub struct PrintLog {
    lines: VecDeque<PrintLine>,
}

impl PrintLog {
    pub fn push(&mut self, line: PrintLine) {
        if self.lines.len() >= MAX_LINES {
            self.lines.pop_front();
        }
        self.lines.push_back(line);
    }

    pub fn lines(&self) -> &VecDeque<PrintLine> {
        &self.lines
    }

    pub fn clear(&mut self) {
        self.lines.clear();
    }
}
