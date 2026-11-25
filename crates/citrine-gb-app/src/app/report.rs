use super::Citrine;
use crate::game_report::Built;
use crate::utils::file_channels::FileChannels;

#[cfg(not(target_os = "android"))]
pub(crate) const SAVE_GAME_REPORT: &str = "Save a bug report";
#[cfg(target_os = "android")]
pub(crate) const SAVE_GAME_REPORT: &str = "Share a bug report";
#[cfg(not(target_os = "android"))]
pub(crate) const SAVE_GAME_REPORT_ICON: &str = crate::icons::DOWNLOAD_SIMPLE;
#[cfg(target_os = "android")]
pub(crate) const SAVE_GAME_REPORT_ICON: &str = crate::icons::SHARE_NETWORK;
pub(crate) const GAME_REPORT_NOTE: &str = "A zip of the last 10 to 20 seconds of play, with screens, the save file and the app log, so a bug can be replayed. Passwords and sign-ins stay out.";

impl Citrine {
    pub(crate) fn save_game_report(&mut self) {
        if !self.emulator.has_game() {
            return;
        }
        let gathered = crate::game_report::gather(&mut self.emulator, &self.ui.settings);
        let hand = Hand::of(&self.files, "report");
        self.files
            .spawner
            .run("game report", move || match gathered.build() {
                Ok(built) => hand.over(Outgoing::report(built)),
                Err(err) => hand.failed(err.to_string()),
            });
    }
}

pub(crate) struct Outgoing {
    pub(crate) file_name: String,
    pub(crate) bytes: Vec<u8>,
    pub(crate) filter: &'static str,
    pub(crate) extension: &'static str,
    pub(crate) mime: &'static str,
}

impl Outgoing {
    fn report(built: Built) -> Self {
        Self {
            file_name: built.file_name,
            bytes: built.zip,
            filter: "Game report",
            extension: "zip",
            mime: "application/zip",
        }
    }
}

pub(crate) struct Hand {
    what: &'static str,
    #[cfg(not(target_os = "android"))]
    spawner: citrine_gb_threads::Spawner,
    #[cfg(not(target_os = "android"))]
    saved: std::sync::mpsc::Sender<crate::utils::file_saver::SaveOutcome>,
    #[cfg(target_os = "android")]
    error: std::sync::mpsc::Sender<String>,
}

impl Hand {
    pub(crate) fn of(files: &FileChannels, what: &'static str) -> Self {
        Self {
            what,
            #[cfg(not(target_os = "android"))]
            spawner: files.spawner.clone(),
            #[cfg(not(target_os = "android"))]
            saved: files.save_tx.clone(),
            #[cfg(target_os = "android")]
            error: files.error_tx.clone(),
        }
    }

    #[cfg(not(target_os = "android"))]
    pub(crate) fn over(self, file: Outgoing) {
        let _ = (self.what, file.mime);
        crate::utils::file_saver::FileSaver::new(&file.file_name)
            .add_filter(file.filter, &[file.extension])
            .dispatch(&self.spawner, file.bytes, self.saved);
    }

    #[cfg(not(target_os = "android"))]
    pub(crate) fn failed(self, err: String) {
        let _ = self
            .saved
            .send(crate::utils::file_saver::SaveOutcome::Failed(err));
    }

    #[cfg(target_os = "android")]
    pub(crate) fn over(self, file: Outgoing) {
        let _ = (file.filter, file.extension);
        let dir = crate::storage::folder(crate::storage::kinds::Folder::Reports);
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join(&file.file_name);
        let written =
            std::fs::create_dir_all(&dir).and_then(|()| std::fs::write(&path, &file.bytes));
        if let Err(err) = written {
            return self.failed(err.to_string());
        }
        if crate::platform::android::clipboard::share_file(&path.to_string_lossy(), file.mime)
            .is_err()
        {
            let _ = self.error.send("Couldn't open the share sheet".into());
        }
    }

    #[cfg(target_os = "android")]
    pub(crate) fn failed(self, err: String) {
        let _ = self
            .error
            .send(format!("Couldn't share the {}: {err}", self.what));
    }
}
