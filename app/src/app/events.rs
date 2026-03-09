use crate::app::tabs::Tab;

pub enum AppEvent {
    LoadRomData { data: Vec<u8> },
    OpenTab { tab: Tab },
}

#[derive(Default)]
pub struct AppEventQueue {
    queue: Vec<AppEvent>,
}

impl AppEventQueue {
    pub fn take(&mut self) -> Vec<AppEvent> {
        std::mem::take(&mut self.queue)
    }

    pub fn push(&mut self, event: AppEvent) {
        self.queue.push(event);
    }

    pub fn load_rom_data(&mut self, data: Vec<u8>) {
        self.queue.push(AppEvent::LoadRomData { data });
    }

    pub fn open_tab(&mut self, tab: Tab) {
        self.queue.push(AppEvent::OpenTab { tab });
    }
}
