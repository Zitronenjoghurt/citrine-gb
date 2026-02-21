use std::collections::HashSet;

#[derive(Debug, Default)]
pub struct Debugger {
    pub breakpoints: HashSet<u16>,
}

impl Debugger {
    pub fn new() -> Self {
        Self::default()
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
