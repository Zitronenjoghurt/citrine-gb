use std::path::{Path, PathBuf};

const SHARED: &[(&str, &str)] = &[
    (
        "app/big_screen.rs",
        "the big screen's hold for menu tip on the TV; it takes the kit card fill token as the TV keyboard does",
    ),
    (
        "app/big_screen/keyboard.rs",
        "the big screen TV keyboard, not a desktop widget; it takes the kit card fill and hairline colour tokens",
    ),
    (
        "app/big_screen/move_screen.rs",
        "the big screen's move hints card on the TV; it takes the kit card fill token as the TV keyboard does",
    ),
    (
        "app/push.rs",
        "ask_card shows only on the touch friends page",
    ),
    (
        "app/second_screens.rs",
        "the joined second-screen view is a full-window touch surface on any device",
    ),
    (
        "app/tabs/chat.rs",
        "the kit header button is the touch branch of the chat header",
    ),
    (
        "app/tabs/friends.rs",
        "every kit call sits behind the touch flag; desktop draws person_row and plain buttons",
    ),
    (
        "app/tabs/gts.rs",
        "the GTS pane draws kit cards, rows and the search field only in its touch branch",
    ),
    (
        "app/tabs/media.rs",
        "kit section labels head the touch grid only",
    ),
    (
        "app/tabs/playing/room_bar.rs",
        "kit pills, rows, switches and card fill are the phone branches; desktop draws buttons and menu rows",
    ),
    (
        "app/tabs/second_screens.rs",
        "the phone branches (kit rows, code form, device sheet) run only when touch_square is set",
    ),
    (
        "app/tabs/stats/phone.rs",
        "phone stats pages, called only when the layout is touch",
    ),
    (
        "app/tabs/walker.rs",
        "the kit switch is the phone branch of the steps row, drawn only when phone is set",
    ),
    (
        "app/ui_state/ui_theme.rs",
        "theme tests check the kit colours",
    ),
    (
        "app/widgets/confirm.rs",
        "kit rows, switches and buttons are the phone branch of each dialog",
    ),
    (
        "app/widgets/game_tile.rs",
        "the big screen library tile takes the kit card fill so it matches the Recent covers",
    ),
    (
        "app/widgets/section.rs",
        "phone_section runs only when touch_square is set",
    ),
    (
        "app/widgets/settings/online.rs",
        "name_editor, two_factor_phone, accounts_body and add_account_body are called only from mobile/you.rs",
    ),
    (
        "app/widgets/settings/online/accounts.rs",
        "ROW_HEIGHT sizes only Look::CARD",
    ),
    (
        "app/widgets/settings/online/devices.rs",
        "kit pills and the kit field draw only when touch_square is set",
    ),
    (
        "app/widgets/settings/online/phone_join.rs",
        "the signed-out rows, join sheet and dialogs are called only from mobile/you.rs",
    ),
    (
        "app/widgets/settings/online/storage.rs",
        "storage_list is called only from the phone online settings page",
    ),
    (
        "app/widgets/settings/online/two_factor.rs",
        "kit calls sit in the phone setup and codes steps, reached only from mobile/you.rs",
    ),
    (
        "app/widgets/settings/page.rs",
        "kit section titles and cards are the phone branch of Page",
    ),
];

fn src_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

fn calls_kit(line: &str) -> bool {
    line.contains("mobile::kit")
        || line.contains("mobile::{kit")
        || line.match_indices("kit::").any(|(at, _)| {
            line[..at]
                .chars()
                .next_back()
                .is_none_or(|c| !(c.is_alphanumeric() || c == '_'))
        })
}

fn kit_lines(source: &str) -> Vec<usize> {
    source
        .lines()
        .enumerate()
        .filter(|(_, line)| calls_kit(line))
        .map(|(index, _)| index + 1)
        .collect()
}

fn phone_module(name: &str) -> bool {
    name == "app/mobile.rs" || name.starts_with("app/mobile/")
}

#[test]
fn desktop_code_never_draws_the_phone_kit() {
    let src = src_dir();
    let mut files = Vec::new();
    rust_files(&src, &mut files);
    files.sort();
    let mut slips = Vec::new();
    for file in files {
        let name = file
            .strip_prefix(&src)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        if phone_module(&name) || SHARED.iter().any(|(path, _)| *path == name) {
            continue;
        }
        let source = std::fs::read_to_string(&file).unwrap();
        for line in kit_lines(&source) {
            slips.push(format!("{name}:{line}"));
        }
    }
    assert!(
        slips.is_empty(),
        "desktop code calls mobile::kit; draw the desktop look there, move the phone part under app/mobile, or add the file to SHARED with its reason:\n{}",
        slips.join("\n")
    );
}

#[test]
fn every_shared_file_still_calls_the_kit() {
    let src = src_dir();
    for (path, reason) in SHARED {
        assert!(!reason.is_empty(), "{path} has no reason");
        let source = std::fs::read_to_string(src.join(path)).unwrap();
        assert!(
            !kit_lines(&source).is_empty(),
            "{path} no longer calls the kit; drop it from SHARED"
        );
    }
}

#[test]
fn the_scan_reads_kit_paths_and_skips_lookalikes() {
    assert!(calls_kit("    kit::pill(ui, \"Undo\", false)"));
    assert!(calls_kit("use crate::app::mobile::kit;"));
    assert!(calls_kit("use crate::app::mobile::{kit, touch_square};"));
    assert!(calls_kit("super::mobile::kit::card(ui, |ui| {"));
    assert!(!calls_kit("use egui_kittest::Harness;"));
    assert!(!calls_kit("toolkit::run()"));
}

#[test]
fn the_scan_reaches_ui_code_outside_the_app_folder() {
    let src = src_dir();
    let mut files = Vec::new();
    rust_files(&src, &mut files);
    for wanted in ["app.rs", "covers.rs", "app/tabs/walker.rs"] {
        assert!(files.contains(&src.join(wanted)), "{wanted} is not scanned");
    }
}
