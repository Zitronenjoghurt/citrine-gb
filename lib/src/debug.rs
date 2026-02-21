#[derive(Debug, Default)]
pub struct Debugger {}

impl Debugger {
    pub fn new() -> Self {
        Self::default()
    }
}

impl DebuggerInterface for Debugger {}

pub trait DebuggerInterface {}

#[cfg(feature = "debug")]
pub trait DebuggerAccess {
    fn debugger(&mut self) -> &mut dyn DebuggerInterface;
}

impl<T: DebuggerAccess> DebuggerInterface for T {}
