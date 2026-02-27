use std::collections::HashSet;

pub mod e2e;

#[derive(Debug, Default)]
pub struct Debugger {
    pub breakpoints: HashSet<u16>,
    pub total_cycles: u128,
}

impl Debugger {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn soft_reset(&mut self) {
        self.total_cycles = 0;
    }
}

impl DebuggerInterface for Debugger {
    fn break_at(&self, addr: u16) -> bool {
        self.breakpoints.contains(&addr)
    }
}

pub trait DebuggerInterface {
    fn break_at(&self, _addr: u16) -> bool {
        false
    }
}

#[cfg(feature = "debug")]
pub trait DebuggerAccess {
    fn debugger(&self) -> &dyn DebuggerInterface;
    fn debugger_mut(&mut self) -> &mut dyn DebuggerInterface;
}

impl<T: DebuggerAccess> DebuggerInterface for T {
    fn break_at(&self, addr: u16) -> bool {
        self.debugger().break_at(addr)
    }
}
