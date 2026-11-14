use crate::utils::file_loader::PickedFile;
use crate::utils::file_saver::SaveOutcome;
use citrine_gb_threads::Spawner;
use std::sync::mpsc::{Receiver, Sender, channel};

pub struct FileChannels {
    pub rom_tx: Sender<PickedFile>,
    pub rom_rx: Receiver<PickedFile>,
    pub second_rom_tx: Sender<PickedFile>,
    pub second_rom_rx: Receiver<PickedFile>,
    pub boot_rom_tx: Sender<PickedFile>,
    pub boot_rom_rx: Receiver<PickedFile>,
    pub gba_bios_tx: Sender<PickedFile>,
    pub gba_bios_rx: Receiver<PickedFile>,
    #[cfg(feature = "nds")]
    pub ds_file_tx: Sender<(crate::app::ds_firmware::DsFile, PickedFile)>,
    #[cfg(feature = "nds")]
    pub ds_file_rx: Receiver<(crate::app::ds_firmware::DsFile, PickedFile)>,
    pub play_rom_tx: Sender<PickedFile>,
    pub play_rom_rx: Receiver<PickedFile>,
    pub picture_tx: Sender<PickedFile>,
    pub picture_rx: Receiver<PickedFile>,
    pub sav_tx: Sender<Vec<PickedFile>>,
    pub sav_rx: Receiver<Vec<PickedFile>>,
    #[cfg(not(target_arch = "wasm32"))]
    pub folder_tx: Sender<std::path::PathBuf>,
    #[cfg(not(target_arch = "wasm32"))]
    pub folder_rx: Receiver<std::path::PathBuf>,
    pub save_tx: Sender<SaveOutcome>,
    pub save_rx: Receiver<SaveOutcome>,
    pub copy_tx: Sender<Result<String, String>>,
    pub copy_rx: Receiver<Result<String, String>>,
    pub error_tx: Sender<String>,
    pub error_rx: Receiver<String>,
    pub rom_copies: crate::app::ui_state::recent::copies::RomCopies,
    pub spawner: Spawner,
}

impl FileChannels {
    pub fn new(spawner: Spawner) -> Self {
        let (rom_tx, rom_rx) = channel();
        let (second_rom_tx, second_rom_rx) = channel();
        let (boot_rom_tx, boot_rom_rx) = channel();
        let (gba_bios_tx, gba_bios_rx) = channel();
        #[cfg(feature = "nds")]
        let (ds_file_tx, ds_file_rx) = channel();
        let (play_rom_tx, play_rom_rx) = channel();
        let (picture_tx, picture_rx) = channel();
        let (sav_tx, sav_rx) = channel();
        #[cfg(not(target_arch = "wasm32"))]
        let (folder_tx, folder_rx) = channel();
        let (save_tx, save_rx) = channel();
        let (copy_tx, copy_rx) = channel();
        let (error_tx, error_rx) = channel();
        Self {
            rom_tx,
            rom_rx,
            second_rom_tx,
            second_rom_rx,
            boot_rom_tx,
            boot_rom_rx,
            gba_bios_tx,
            gba_bios_rx,
            #[cfg(feature = "nds")]
            ds_file_tx,
            #[cfg(feature = "nds")]
            ds_file_rx,
            play_rom_tx,
            play_rom_rx,
            picture_tx,
            picture_rx,
            sav_tx,
            sav_rx,
            #[cfg(not(target_arch = "wasm32"))]
            folder_tx,
            #[cfg(not(target_arch = "wasm32"))]
            folder_rx,
            save_tx,
            save_rx,
            copy_tx,
            copy_rx,
            error_tx,
            error_rx,
            rom_copies: Default::default(),
            spawner,
        }
    }
}
