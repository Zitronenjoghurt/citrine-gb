use super::Citrine;
use super::mobile::Sheet;
use super::tabs::Tab;
use super::ui_state::UiState;
use super::ui_state::settings::SettingsTab;
use super::widgets::confirm::{self, Answer, Ask};
use super::widgets::settings::your_data::{Exporting, PendingImport, Request};
use crate::utils::file_loader::FileLoader;
use crate::utils::file_saver::{FileSaver, SaveOutcome};
use crate::your_data::{self, Change, Files, Kind, Parts, Tally};
use egui::Context;
use egui_dock::DockState;

const COUNT_EVERY: std::time::Duration = std::time::Duration::from_secs(2);
const FILTER: &str = "Citrine data";

impl Citrine {
    fn data_parts(&mut self) -> Parts<'_> {
        Parts {
            saves: &self.emulator.store,
            media: &mut self.media,
            online: &mut self.online,
            settings: &mut self.ui.settings,
            stats: &mut self.stats,
            #[cfg(not(target_arch = "wasm32"))]
            recent: &mut self.ui.recent,
        }
    }

    fn your_data_open(&self) -> bool {
        self.ui.settings.current_tab == SettingsTab::YourData
            && (self.dock.find_tab(&Tab::Settings).is_some()
                || self.mobile.sheet == Some(Sheet::Panel(Tab::Settings)))
    }

    pub(super) fn count_your_data(&mut self) {
        if !self.your_data_open() {
            self.ui.your_data.counted_at = None;
            return;
        }
        let now = web_time::Instant::now();
        if self
            .ui
            .your_data
            .counted_at
            .is_some_and(|at| now.duration_since(at) < COUNT_EVERY)
        {
            return;
        }
        self.ui.your_data.rows = self.data_parts().tallies();
        self.ui.your_data.counted_at = Some(now);
    }

    fn recount_your_data(&mut self) {
        self.ui.your_data.counted_at = None;
        self.ui.snapshots.invalidate();
    }

    pub(super) fn handle_your_data(&mut self, ctx: &Context) {
        match self.ui.your_data.request.take() {
            Some(Request::Export) => self.start_data_export(),
            Some(Request::Import) => {
                FileLoader::new()
                    .title("Import your data")
                    .add_filter(FILTER, &[your_data::FILE_EXTENSION])
                    .dispatch(self.ui.your_data.picked_tx.clone());
            }
            Some(Request::Delete) => {
                self.ui.your_data.deleting = true;
                if self.ui.your_data.exporting == Exporting::Done {
                    self.ui.your_data.exporting = Exporting::Idle;
                }
            }
            None => {}
        }
        self.poll_data_export();
        while let Ok(file) = self.ui.your_data.picked_rx.try_recv() {
            self.read_data_file(file.name, &file.data);
        }
        self.import_dialog(ctx);
        self.delete_dialog(ctx);
    }

    fn start_data_export(&mut self) {
        if let Err(err) = self.emulator.flush_save() {
            log::error!("failed to flush save before the export: {err:?}");
        }
        let mut files = self.data_parts().export();
        let tx = self.ui.your_data.files_tx.clone();
        self.ui.your_data.exporting = Exporting::Busy;
        self.media.read_files(move |media| {
            your_data::add_media_files(&mut files, media);
            let _ = tx.send(files);
        });
    }

    fn poll_data_export(&mut self) {
        while let Ok(files) = self.ui.your_data.files_rx.try_recv() {
            match your_data::pack(&files) {
                Ok(bytes) => {
                    FileSaver::new(&your_data::file_name(crate::storage::now_unix()))
                        .add_filter(FILTER, &[your_data::FILE_EXTENSION])
                        .dispatch(bytes, self.ui.your_data.saved_tx.clone());
                }
                Err(err) => {
                    self.ui.your_data.exporting = Exporting::Idle;
                    self.toasts.error(format!("Could not export: {err}"));
                }
            }
        }
        while let Ok(outcome) = self.ui.your_data.saved_rx.try_recv() {
            self.ui.your_data.exporting = match outcome {
                SaveOutcome::Saved(name) | SaveOutcome::SavedPair(name, _) => {
                    self.toasts.success(format!("Exported to '{name}'"));
                    Exporting::Done
                }
                SaveOutcome::Failed(err) => {
                    self.toasts.error(format!("Could not export: {err}"));
                    Exporting::Idle
                }
                SaveOutcome::Cancelled => Exporting::Idle,
            };
        }
    }

    fn read_data_file(&mut self, name: String, bytes: &[u8]) {
        let files = match your_data::unpack(bytes) {
            Ok(files) => files,
            Err(err) => {
                self.toasts.error(err.text());
                return;
            }
        };
        if let Err(err) = self.emulator.flush_save() {
            log::error!("failed to flush save before the import: {err:?}");
        }
        let plan = self.data_parts().plan(&files);
        if plan.iter().all(|(_, change)| *change == Change::default()) {
            self.toasts
                .info(format!("This device already has everything in '{name}'"));
            return;
        }
        self.ui.your_data.pending = Some(PendingImport { name, files, plan });
    }

    fn import_dialog(&mut self, ctx: &Context) {
        let Some(pending) = &self.ui.your_data.pending else {
            return;
        };
        let message = format!("Import '{}'?", pending.name);
        let notes = import_notes(&pending.plan);
        let answer = confirm::ask(
            ctx,
            "your_data_import",
            Ask {
                message: &message,
                notes: &notes,
                action: "Import",
                action_enabled: true,
                extra: None,
            },
        );
        match answer {
            Some(Answer::Action) => {
                if let Some(pending) = self.ui.your_data.pending.take() {
                    self.import_data(pending.name, &pending.files);
                }
            }
            Some(_) => self.ui.your_data.pending = None,
            None => {}
        }
    }

    fn import_data(&mut self, name: String, files: &Files) {
        let rom = self.emulator.rom_key().map(str::to_owned);
        let battery = rom
            .as_deref()
            .and_then(|rom| self.emulator.store.battery_saved_at(rom));
        let result = self.data_parts().import(files);
        self.media.name_games(&self.ui.settings.game_names);
        if let Some(rom) = rom
            && self.emulator.store.battery_saved_at(&rom) != battery
            && let Err(err) = self.emulator.reload_battery()
        {
            log::error!("failed to load the imported save: {err:?}");
        }
        self.recount_your_data();
        match result {
            Ok(()) => self.toasts.success(format!("Imported '{name}'")),
            Err(err) => self
                .toasts
                .error(format!("Could not import everything: {err}")),
        };
    }

    fn delete_dialog(&mut self, ctx: &Context) {
        if !self.ui.your_data.deleting {
            return;
        }
        let notes = delete_notes(&self.ui.your_data.rows);
        let exporting = self.ui.your_data.exporting;
        let extra = match exporting {
            Exporting::Idle => "Export first",
            Exporting::Busy => "Exporting…",
            Exporting::Done => "Exported",
        };
        let answer = confirm::ask(
            ctx,
            "your_data_delete",
            Ask {
                message: "Delete everything on this device?",
                notes: &notes,
                action: "Delete everything",
                action_enabled: exporting != Exporting::Busy,
                extra: Some((extra, exporting == Exporting::Idle)),
            },
        );
        match answer {
            Some(Answer::Extra) => self.start_data_export(),
            Some(Answer::Action) => {
                self.ui.your_data.deleting = false;
                self.delete_everything();
            }
            Some(Answer::Cancel) => self.ui.your_data.deleting = false,
            None => {}
        }
    }

    pub(super) fn delete_everything(&mut self) {
        if self.play.session().is_some() {
            self.play.leave(&mut self.online);
        }
        self.emulator.unload();
        let result = self.data_parts().delete_all();
        self.online.cloud_forget_local();
        self.first_launch();
        match result {
            Ok(()) => self.toasts.success("Deleted everything on this device"),
            Err(err) => self
                .toasts
                .error(format!("Could not delete everything: {err}")),
        };
    }

    fn first_launch(&mut self) {
        let settings = std::mem::take(&mut self.ui.settings);
        let recent = std::mem::take(&mut self.ui.recent);
        let mut your_data = std::mem::take(&mut self.ui.your_data);
        your_data.counted_at = None;
        your_data.exporting = Exporting::Idle;
        self.ui = UiState {
            settings,
            recent,
            your_data,
            ..UiState::default()
        };
        self.ui.settings.current_tab = SettingsTab::default();
        self.dock = DockState::new(vec![Tab::GameBoy]);
        self.open_tab(Tab::Info);
        self.mobile.close();
        self.game_info = Default::default();
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.shots = Default::default();
        }
        self.cloud_rom = None;
        self.cloud_hold = None;
        self.pending_import = None;
        self.pending_reset = None;
        self.pending_console = None;
        self.reset_without_asking = false;
        self.quick_load_hold = None;
    }
}

fn listed(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

fn import_notes(plan: &[(Kind, Change)]) -> Vec<String> {
    let settings = |kind: &Kind| kind.name == "Settings";
    let added: Vec<String> = plan
        .iter()
        .filter(|(_, change)| change.added > 0)
        .map(|(kind, change)| kind.noun(change.added))
        .collect();
    let replaced: Vec<String> = plan
        .iter()
        .filter(|(kind, change)| change.replaced > 0 && !settings(kind))
        .map(|(kind, change)| kind.noun(change.replaced))
        .collect();
    let mut notes = Vec::new();
    if !added.is_empty() {
        notes.push(format!("Adds {}.", listed(&added)));
    }
    if !replaced.is_empty() {
        let copies = match plan
            .iter()
            .filter(|(kind, _)| !settings(kind))
            .map(|(_, change)| change.replaced)
            .sum::<usize>()
        {
            1 => "a newer copy",
            _ => "newer copies",
        };
        notes.push(format!(
            "Replaces {} with {copies} from the file.",
            listed(&replaced)
        ));
    }
    if let Some((kind, change)) = plan
        .iter()
        .find(|(kind, change)| settings(kind) && change.replaced > 0)
    {
        notes.push(format!(
            "Changes {} to match the file.",
            kind.noun(change.replaced)
        ));
    }
    if !notes.is_empty() {
        notes.push("Everything else on this device stays.".to_owned());
    }
    notes
}

fn delete_notes(rows: &[(Kind, Tally)]) -> Vec<String> {
    let made = |(kind, tally): &&(Kind, Tally)| tally.count > 0 && kind.name != "Settings";
    let goes: Vec<String> = rows
        .iter()
        .filter(made)
        .map(|(kind, tally)| kind.noun(tally.count))
        .collect();
    let changed = rows
        .iter()
        .find(|(kind, tally)| kind.name == "Settings" && tally.count > 0)
        .map(|(kind, tally)| format!("resets {} to default", kind.noun(tally.count)));
    let first = match (goes.is_empty(), changed) {
        (true, None) => "Nothing you made is stored here yet.".to_owned(),
        (true, Some(changed)) => format!("This {changed}."),
        (false, None) => format!("This deletes {}.", listed(&goes)),
        (false, Some(changed)) => {
            let comma = if goes.len() > 1 { "," } else { "" };
            format!("This deletes {}{comma} and {changed}.", listed(&goes))
        }
    };
    vec![
        first,
        "Your online account and your copies in the cloud stay.".to_owned(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_import_confirm_says_what_it_adds_and_replaces() {
        let kind = |name, units, nouns| Kind {
            name,
            icon: "",
            units,
            nouns,
        };
        let saves = kind("Saves", ("save", "saves"), ("save", "saves"));
        let chats = kind(
            "Chats",
            ("message", "messages"),
            ("chat message", "chat messages"),
        );
        let settings = kind("Settings", ("changed", "changed"), ("setting", "settings"));
        let notes = import_notes(&[
            (
                saves,
                Change {
                    added: 2,
                    replaced: 1,
                },
            ),
            (
                chats,
                Change {
                    added: 1,
                    replaced: 0,
                },
            ),
            (
                settings,
                Change {
                    added: 0,
                    replaced: 4,
                },
            ),
        ]);
        assert_eq!(
            notes,
            [
                "Adds 2 saves and 1 chat message.",
                "Replaces 1 save with a newer copy from the file.",
                "Changes 4 settings to match the file.",
                "Everything else on this device stays.",
            ]
        );
    }

    #[test]
    fn the_delete_confirm_counts_what_goes_and_says_what_stays() {
        let kind = |name, nouns| Kind {
            name,
            icon: "",
            units: ("", ""),
            nouns,
        };
        let tally = |count| Tally { count, bytes: 0 };
        let rows = [
            (kind("Saves", ("save", "saves")), tally(4)),
            (kind("Media", ("file", "files")), tally(0)),
            (kind("Settings", ("setting", "settings")), tally(2)),
            (kind("Game names", ("game name", "game names")), tally(1)),
        ];
        assert_eq!(
            delete_notes(&rows),
            [
                "This deletes 4 saves and 1 game name, and resets 2 settings to default.",
                "Your online account and your copies in the cloud stay.",
            ]
        );
        assert_eq!(
            delete_notes(&rows[..3])[0],
            "This deletes 4 saves and resets 2 settings to default."
        );
        assert_eq!(
            delete_notes(&rows[1..2])[0],
            "Nothing you made is stored here yet."
        );
    }
}
