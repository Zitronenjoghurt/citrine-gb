use crate::utils::file_loader::PickedFile;
use crate::utils::file_saver::SaveOutcome;
use std::sync::mpsc::{Receiver, Sender, channel};

pub struct FileChannels {
    pub rom_tx: Sender<PickedFile>,
    pub rom_rx: Receiver<PickedFile>,
    pub boot_rom_tx: Sender<PickedFile>,
    pub boot_rom_rx: Receiver<PickedFile>,
    pub sav_tx: Sender<PickedFile>,
    pub sav_rx: Receiver<PickedFile>,
    #[cfg(not(target_arch = "wasm32"))]
    pub folder_tx: Sender<std::path::PathBuf>,
    #[cfg(not(target_arch = "wasm32"))]
    pub folder_rx: Receiver<std::path::PathBuf>,
    pub save_tx: Sender<SaveOutcome>,
    pub save_rx: Receiver<SaveOutcome>,
}

impl Default for FileChannels {
    fn default() -> Self {
        let (rom_tx, rom_rx) = channel();
        let (boot_rom_tx, boot_rom_rx) = channel();
        let (sav_tx, sav_rx) = channel();
        #[cfg(not(target_arch = "wasm32"))]
        let (folder_tx, folder_rx) = channel();
        let (save_tx, save_rx) = channel();
        Self {
            rom_tx,
            rom_rx,
            boot_rom_tx,
            boot_rom_rx,
            sav_tx,
            sav_rx,
            #[cfg(not(target_arch = "wasm32"))]
            folder_tx,
            #[cfg(not(target_arch = "wasm32"))]
            folder_rx,
            save_tx,
            save_rx,
        }
    }
}
