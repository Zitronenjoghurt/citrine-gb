use super::Emulator;

impl Emulator {
    pub fn hardcore(&self) -> bool {
        self.hardcore
    }

    pub(crate) fn refuse_in_hardcore(&self) -> citrine_gb::error::GbResult<()> {
        if self.hardcore {
            return Err(std::io::Error::other("Off in hardcore").into());
        }
        Ok(())
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn follow_hardcore(&mut self, on: bool) {
        if self.hardcore == on {
            return;
        }
        self.hardcore = on;
        self.cheats_for = None;
        self.pause_due = None;
        if on {
            self.rewind_held = false;
            self.seat.core_mut().set_cheats(Vec::new());
        }
    }

    pub(crate) fn pause_wait(&mut self) -> Option<u32> {
        if !self.hardcore {
            return None;
        }
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(client) = self.seat.core_mut().achievements.as_mut() {
            return client.can_pause().err();
        }
        None
    }

    pub fn try_pause(&mut self) -> bool {
        if self.pause_wait().is_some() {
            return false;
        }
        self.running = false;
        true
    }

    pub fn pause_due(&self) -> Option<u32> {
        self.pause_due
    }

    pub fn take_pause_news(&mut self) -> Option<u32> {
        self.pause_news.take()
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn follow_pause_due(&mut self) {
        if self.pause_due.is_none() {
            return;
        }
        if !self.running {
            self.pause_due = None;
            return;
        }
        if self.pause_wait().is_none() {
            self.pause_due = None;
            self.running = false;
            self.release_all();
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Emulator {
    pub(crate) fn achievements_progress(&mut self) -> Option<Vec<u8>> {
        if !self.seat.is_home() {
            return None;
        }
        self.seat.core_mut().achievements.as_mut()?.progress()
    }

    pub(crate) fn achievements_restore(&mut self, progress: Option<&[u8]>) {
        if !self.seat.is_home() {
            return;
        }
        if let Some(client) = self.seat.core_mut().achievements.as_mut() {
            client.restore_progress(progress);
        }
    }
}

#[cfg(target_arch = "wasm32")]
impl Emulator {
    pub(crate) fn achievements_progress(&mut self) -> Option<Vec<u8>> {
        None
    }

    pub(crate) fn achievements_restore(&mut self, _progress: Option<&[u8]>) {}
}
