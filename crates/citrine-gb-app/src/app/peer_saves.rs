use super::Citrine;
use super::events::AppEvent;
use super::tabs;
use crate::app::widgets::confirm::confirm;
use crate::app::widgets::snapshots::{FailedStep, failed_message};
use crate::emulator::peer_saves::{KeptSaves, PeerNotice};

impl Citrine {
    pub(super) fn tell_peer_saves(&mut self) {
        while let Some(notice) = self.emulator.take_peer_notice() {
            self.show_peer_notice(notice);
        }
    }

    fn friend_name(&self, notice: &PeerNotice) -> String {
        notice
            .name
            .clone()
            .unwrap_or_else(|| tabs::playing::member_name(&self.play, &self.online, notice.friend))
    }

    pub(super) fn show_peer_notice(&mut self, mut notice: PeerNotice) {
        let name = self.friend_name(&notice);
        if notice.name.is_none() {
            notice.name = Some(name.clone());
            self.emulator.name_peer_saves(&notice);
        }
        self.toasts
            .info(format!("{name}'s saves didn't replace yours."))
            .button(
                format!("Use {name}'s, keep mine in Saves"),
                AppEvent::KeepPeerSaves(notice.clone()),
            )
            .or(
                format!("Delete {name}'s saves"),
                AppEvent::AskDiscardPeerSaves(notice.clone()),
            )
            .on_close(AppEvent::SetPeerSavesAside(notice))
            .needs_action();
    }

    pub(super) fn keep_peer_saves(&mut self, notice: &PeerNotice) {
        let name = self.friend_name(notice);
        match self.emulator.keep_peer_saves(&notice.divert, &name) {
            Ok(kept) => {
                let text = if kept.set_mine_aside() {
                    format!("You're playing {name}'s save. Yours is kept in Saves.")
                } else {
                    format!("You're playing {name}'s save.")
                };
                self.toasts
                    .success(text)
                    .button("Undo", AppEvent::UndoKeptSaves(Box::new(kept)));
            }
            Err(err) => {
                self.toasts
                    .error(failed_message(FailedStep::Switch, "the synced saves", &err));
            }
        }
        self.ui.snapshots.invalidate();
    }

    pub(super) fn peer_discard_confirm(&mut self, ctx: &egui::Context) {
        let Some(notice) = &self.pending_peer_discard else {
            return;
        };
        let choice = confirm(
            ctx,
            "peer_discard_confirm",
            &self.discard_question(notice),
            "Delete",
        );
        if let Some(confirmed) = choice {
            self.finish_peer_discard(confirmed);
        }
    }

    fn discard_question(&self, notice: &PeerNotice) -> String {
        let name = self.friend_name(notice);
        let question = format!("Delete {name}'s saves? Your own save is untouched.");
        if self.emulator.diverting(&notice.divert) {
            format!("{question} If {name} saves again, Citrine asks which save to keep.")
        } else {
            question
        }
    }

    pub(super) fn finish_peer_discard(&mut self, confirmed: bool) {
        let Some(notice) = self.pending_peer_discard.take() else {
            return;
        };
        let name = self.friend_name(&notice);
        if !confirmed {
            self.show_peer_notice(notice);
            return;
        }
        match self.emulator.discard_peer_saves(&notice.divert) {
            Ok(()) => {
                self.toasts.info(format!("{name}'s saves deleted"));
            }
            Err(err) => {
                self.toasts
                    .error(failed_message(FailedStep::Delete, "the synced saves", &err));
            }
        }
        self.ui.snapshots.invalidate();
    }

    pub(super) fn set_peer_saves_aside(&mut self, notice: &PeerNotice) {
        let name = self.friend_name(notice);
        if let Err(err) = self.emulator.set_peer_saves_aside(&notice.divert, &name) {
            self.toasts
                .error(failed_message(FailedStep::Switch, "the synced saves", &err));
        }
        self.ui.snapshots.invalidate();
    }

    pub(super) fn undo_kept_saves(&mut self, kept: &KeptSaves) {
        if let Err(err) = self.emulator.undo_kept_saves(kept) {
            self.toasts
                .error(failed_message(FailedStep::Switch, "your old saves", &err));
        }
        self.tell_peer_saves();
        self.ui.snapshots.invalidate();
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use citrine_gb::WriteMemory;
    use citrine_gb::gb::GbModel;
    use citrine_gb::machine::Machine;
    use citrine_gb::rom::Rom;

    fn battery_rom() -> Rom {
        let mut data = vec![0u8; 0x8000];
        data[0x0147] = 0x03;
        data[0x0149] = 0x02;
        Rom::new(&data)
    }

    fn write_sram(app: &mut crate::app::Citrine, value: u8) {
        let gb = app.emulator.gb_mut();
        gb.cartridge.write_naive(0x0000, 0x0A);
        gb.cartridge.write_naive(0xA000, value);
    }

    fn saved(app: &mut crate::app::Citrine, key: &str) -> Option<u8> {
        app.emulator.flush_save().expect("flush");
        app.emulator.writes.settle();
        app.emulator.store.load_battery(key).map(|ram| ram[0])
    }

    fn after_a_synced_session(name: &str) -> (crate::app::Citrine, String) {
        let mut app = crate::app::tests::phone_app(name);
        app.emulator
            .load_rom(&battery_rom(), GbModel::Dmg, None)
            .expect("load");
        write_sram(&mut app, 0x11);
        let own = app.emulator.rom_key().expect("key").to_owned();
        assert_eq!(saved(&mut app, &own), Some(0x11));
        let console = Machine::load(&battery_rom(), GbModel::Dmg.into(), &app.emulator.firmware)
            .expect("console");
        app.emulator.keep_returned(crate::play::Returned {
            console,
            borrowed: None,
            paused: false,
            friend: citrine_gb_net::OnlineId::new(7).unwrap(),
            history: None,
            downloaded: false,
            peer_touched: true,
        });
        write_sram(&mut app, 0x22);
        assert_eq!(saved(&mut app, &own), Some(0x11));
        app.tell_peer_saves();
        (app, own)
    }

    fn delete_label(app: &crate::app::Citrine) -> String {
        app.toasts
            .button_labels()
            .into_iter()
            .find(|label| label.starts_with("Delete ") && label.ends_with("'s saves"))
            .expect("a delete button")
            .to_owned()
    }

    fn keep_label(app: &crate::app::Citrine) -> String {
        app.toasts
            .button_labels()
            .into_iter()
            .find(|label| label.ends_with("'s, keep mine in Saves"))
            .expect("a keep button")
            .to_owned()
    }

    #[test]
    fn the_synced_game_notice_keeps_its_saves_and_undo_puts_mine_back() {
        let (mut app, own) = after_a_synced_session("peer-saves-notice");
        let notice = app.toasts.needing_action();
        assert_eq!(notice.len(), 1);
        assert!(
            notice[0].ends_with("'s saves didn't replace yours."),
            "{}",
            notice[0]
        );
        let keep = keep_label(&app);
        let keep = app.toasts.press(&keep).expect("the keep button");
        app.handle_event(keep, false);
        assert_eq!(saved(&mut app, &own), Some(0x22));
        assert!(app.toasts.needing_action().is_empty());
        assert!(
            app.toasts.texts().iter().any(|text| {
                text.starts_with("You're playing ")
                    && text.ends_with("'s save. Yours is kept in Saves.")
            }),
            "{:?}",
            app.toasts.texts()
        );

        let undo = app.toasts.press("Undo").expect("an Undo button");
        app.handle_event(undo, false);
        app.emulator.writes.settle();
        assert_eq!(
            app.emulator.store.load_battery(&own).map(|ram| ram[0]),
            Some(0x11)
        );
        assert_eq!(saved(&mut app, &own), Some(0x11));
        assert_eq!(app.toasts.needing_action().len(), 1);
    }

    fn relaunched(app: crate::app::Citrine) -> crate::app::Citrine {
        let store = app.emulator.store.clone();
        drop(app);
        let mut again = crate::app::tests::test_app();
        again.emulator.store = store;
        again.emulator.reopen_peer_notices();
        again.tell_peer_saves();
        again
    }

    #[test]
    fn closing_the_notice_keeps_my_save_and_files_theirs_in_saves() {
        let (mut app, own) = after_a_synced_session("peer-saves-close");
        let borrowed = app.emulator.divert().expect("the divert").borrowed.clone();
        app.emulator.unload();
        let frame: Vec<u8> = vec![9; 160 * 144 * 4];
        let gb = citrine_gb::console::ConsoleKind::Gb;
        let store = app.emulator.store.clone();
        store.store_snapshot(&own, 0, b"mine", &frame, gb).unwrap();
        store
            .store_autosave(&own, b"my auto", &frame, gb, 3)
            .unwrap();
        store
            .store_snapshot(&borrowed, 0, b"gym", &frame, gb)
            .unwrap();
        store.set_snapshot_name(&borrowed, 0, "Gym").unwrap();
        store
            .store_snapshot(&borrowed, 4, b"theirs", &frame, gb)
            .unwrap();
        store
            .store_autosave(&borrowed, b"their auto", &frame, gb, 3)
            .unwrap();
        let autosaves = store.autosave_ids(&borrowed).len();
        assert!(autosaves >= 1);
        let text = app.toasts.needing_action()[0].to_owned();
        let close = app.toasts.press_close(&text).expect("a close event");
        app.handle_event(close, false);
        app.emulator.writes.settle();
        assert!(app.toasts.needing_action().is_empty());
        assert_eq!(
            app.emulator.store.load_battery(&own).map(|ram| ram[0]),
            Some(0x11)
        );
        assert_eq!(app.emulator.store.load_battery(&borrowed), None);
        let others = app.emulator.store.other_batteries(&own);
        assert_eq!(others.len(), 1);
        assert!(
            others[0]
                .name
                .as_deref()
                .is_some_and(|name| name.ends_with("'s save")),
            "{:?}",
            others[0].name
        );
        assert_eq!(store.load_snapshot(&own, 0).as_deref(), Some(&b"mine"[..]));
        assert!(store.snapshot_name(&own, 0).is_none());
        assert_eq!(store.autosave_ids(&own).len(), 1);
        let mut moved: Vec<String> = store
            .used_slots(&own)
            .into_iter()
            .filter(|&slot| slot != 0)
            .map(|slot| store.snapshot_name(&own, slot).expect("a name"))
            .collect();
        moved.sort();
        let friend = moved[0].split("'s ").next().expect("a friend").to_owned();
        let named = |kind: &str| moved.iter().filter(|name| name.starts_with(kind)).count();
        assert_eq!(named(&format!("{friend}'s Gym")), 1, "{moved:?}");
        assert_eq!(named(&format!("{friend}'s snapshot")), 1, "{moved:?}");
        assert_eq!(
            moved
                .iter()
                .filter(|name| **name == format!("{friend}'s autosave"))
                .count(),
            autosaves,
            "{moved:?}"
        );
        assert_eq!(moved.len(), 2 + autosaves, "{moved:?}");
        assert!(store.used_slots(&borrowed).is_empty());
        assert!(store.autosave_ids(&borrowed).is_empty());
        let again = relaunched(app);
        assert!(again.toasts.needing_action().is_empty());
    }

    #[test]
    fn friend_autosaves_filed_by_the_close_show_under_autosaves_and_stay_theirs() {
        use crate::storage::cloud::{LocalSave, snapshot_slot};
        let (mut app, own) = after_a_synced_session("peer-saves-filed");
        let borrowed = app.emulator.divert().expect("the divert").borrowed.clone();
        app.emulator.unload();
        let frame: Vec<u8> = vec![9; 160 * 144 * 4];
        let gb = citrine_gb::console::ConsoleKind::Gb;
        let store = app.emulator.store.clone();
        store
            .store_autosave(&own, b"my auto", &frame, gb, 3)
            .unwrap();
        store
            .store_snapshot(&borrowed, 0, b"gym", &frame, gb)
            .unwrap();
        for _ in 0..2 {
            store
                .store_autosave(&borrowed, b"their auto", &frame, gb, 3)
                .unwrap();
        }
        let mut before: Vec<Vec<u8>> = store
            .autosave_ids(&borrowed)
            .into_iter()
            .map(|id| store.load_autosave(&borrowed, id).expect("their autosave"))
            .collect();
        before.sort();
        let theirs = before.len();
        let text = app.toasts.needing_action()[0].to_owned();
        let close = app.toasts.press_close(&text).expect("a close event");
        app.handle_event(close, false);
        app.emulator.writes.settle();

        let filed: Vec<usize> = store
            .used_slots(&own)
            .into_iter()
            .filter(|&slot| store.is_filed_autosave(&own, slot))
            .collect();
        assert_eq!(filed.len(), theirs);
        assert_eq!(store.used_slots(&own).len(), theirs + 1);
        let spoken: Vec<String> = filed
            .iter()
            .map(|&slot| store.spoken_snapshot_name(&own, slot).expect("a name"))
            .collect();
        assert!(
            spoken.iter().all(|name| name.contains("'s autosave from ")),
            "{spoken:?}"
        );
        let ctx = egui::Context::default();
        app.ui.snapshots.reload(&ctx, &store, &own);
        assert_eq!(app.ui.snapshots.filed_autosaves().len(), theirs);
        assert_eq!(app.ui.snapshots.slots().len(), 1);
        assert!(
            app.ui
                .snapshots
                .filed_autosaves()
                .iter()
                .all(|view| view.display_name().ends_with("'s autosave"))
        );

        for _ in 0..5 {
            store
                .store_autosave(&own, b"my next auto", &frame, gb, 3)
                .unwrap();
        }
        assert_eq!(store.autosave_ids(&own).len(), 3);
        assert!(
            filed
                .iter()
                .all(|&slot| store.is_filed_autosave(&own, slot))
        );
        let mut after: Vec<Vec<u8>> = filed
            .iter()
            .map(|&slot| store.load_snapshot(&own, slot).expect("the filed autosave"))
            .collect();
        after.sort();
        assert_eq!(after, before);

        let items = store.local_items(&own, "device");
        let autosaves = items
            .iter()
            .filter(|item| matches!(item.save, LocalSave::Autosave(_)))
            .count();
        assert_eq!(autosaves, 3);
        for &slot in &filed {
            let item = items
                .iter()
                .find(|item| item.save == LocalSave::Snapshot(slot))
                .expect("the filed slot syncs as a snapshot");
            assert_eq!(item.slot, snapshot_slot(slot));
            let name = item.payload.file("name").expect("it syncs with its name");
            assert!(
                String::from_utf8_lossy(name).ends_with("'s autosave"),
                "{name:?}"
            );
            assert!(item.payload.file("filed").is_some(), "{:?}", item.save);
        }

        let other_dir = tempfile::tempdir().unwrap();
        let other = crate::storage::SaveStore::at(other_dir.path().to_path_buf());
        for item in &items {
            if let LocalSave::Snapshot(_) = item.save {
                other
                    .write_payload(&own, &item.save, &item.payload)
                    .unwrap();
            }
        }
        app.ui.snapshots.reload(&ctx, &other, &own);
        assert_eq!(app.ui.snapshots.filed_autosaves().len(), theirs);
        assert_eq!(app.ui.snapshots.slots().len(), 1);
        let first = items
            .iter()
            .find(|item| item.save == LocalSave::Snapshot(filed[0]))
            .expect("the first filed slot");
        let mut plain = first.payload.clone();
        plain.files.retain(|(role, _)| role != "filed");
        other.write_payload(&own, &first.save, &plain).unwrap();
        assert!(!other.is_filed_autosave(&own, filed[0]));

        store
            .store_snapshot(&own, filed[0], b"mine now", &frame, gb)
            .unwrap();
        assert!(!store.is_filed_autosave(&own, filed[0]));
        if let Some(&slot) = filed.get(1) {
            store.delete_save(&own, &LocalSave::Snapshot(slot)).unwrap();
            assert!(!store.is_filed_autosave(&own, slot));
        }
    }

    fn tall_frame(
        app: &mut crate::app::Citrine,
        ctx: &egui::Context,
        events: Vec<egui::Event>,
    ) -> Vec<(String, egui::Rect)> {
        frame_at(app, ctx, 2400.0, events)
    }

    fn frame_at(
        app: &mut crate::app::Citrine,
        ctx: &egui::Context,
        height: f32,
        events: Vec<egui::Event>,
    ) -> Vec<(String, egui::Rect)> {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(412.0, height),
            )),
            events,
            ..Default::default()
        };
        let output = ctx.run(input, |ctx| {
            let touch = app.is_mobile(ctx);
            crate::app::mobile::touch_spacing(ctx, touch);
            app.render(ctx);
            app.peer_discard_confirm(ctx);
            app.show_toasts(ctx);
            app.handle_event_queue(touch);
        });
        output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Text(text) => {
                    Some((text.galley.text().to_owned(), text.visual_bounding_rect()))
                }
                _ => None,
            })
            .collect()
    }

    fn friend_autosaves_on_the_saves_page(name: &str) -> crate::app::Citrine {
        let (mut app, own) = after_a_synced_session(name);
        let borrowed = app.emulator.divert().expect("the divert").borrowed.clone();
        app.emulator.unload();
        let frame: Vec<u8> = vec![9; 160 * 144 * 4];
        let gb = citrine_gb::console::ConsoleKind::Gb;
        let store = app.emulator.store.clone();
        for _ in 0..2 {
            store
                .store_autosave(&borrowed, b"their auto", &frame, gb, 3)
                .unwrap();
        }
        store
            .store_autosave(&own, b"my auto", &frame, gb, 3)
            .unwrap();
        let text = app.toasts.needing_action()[0].to_owned();
        let close = app.toasts.press_close(&text).expect("a close event");
        app.handle_event(close, false);
        app.emulator.writes.settle();
        app.emulator
            .load_rom(&battery_rom(), GbModel::Dmg, None)
            .expect("my game again");
        app.ui.snapshots.invalidate();
        app.mobile.open(crate::app::tabs::Tab::Saves);
        app
    }

    #[test]
    fn a_desktop_delete_ask_centres_its_question_and_buttons_in_the_popup() {
        fn flat(shape: egui::Shape) -> Vec<egui::Shape> {
            match shape {
                egui::Shape::Vec(shapes) => shapes.into_iter().flat_map(flat).collect(),
                shape => vec![shape],
            }
        }
        let mut app = friend_autosaves_on_the_saves_page("peer-saves-centre");
        app.ui.settings.layout = crate::app::ui_state::settings::LayoutMode::Desktop;
        app.handle_event(
            crate::app::events::AppEvent::OpenTab {
                tab: crate::app::tabs::Tab::Saves,
            },
            false,
        );
        let ctx = egui::Context::default();
        let mut run = |frame: u32, events: Vec<egui::Event>| {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1280.0, 800.0),
                )),
                time: Some(f64::from(frame) * 0.1),
                events,
                max_texture_side: Some(8192),
                ..Default::default()
            };
            let output = ctx.run(input, |ctx| {
                let touch = app.is_mobile(ctx);
                crate::app::mobile::touch_spacing(ctx, touch);
                app.render(ctx);
                app.peer_discard_confirm(ctx);
                app.show_toasts(ctx);
                app.handle_event_queue(touch);
            });
            output
                .shapes
                .into_iter()
                .flat_map(|clipped| flat(clipped.shape))
                .collect::<Vec<egui::Shape>>()
        };
        let text = |shapes: &[egui::Shape], wanted: &dyn Fn(&str) -> bool| {
            shapes.iter().find_map(|shape| match shape {
                egui::Shape::Text(text) if wanted(text.galley.text()) => {
                    Some(text.visual_bounding_rect())
                }
                _ => None,
            })
        };
        let mut shapes = Vec::new();
        for frame in 0..30 {
            shapes = run(frame, Vec::new());
        }
        let at = shapes
            .iter()
            .filter_map(|shape| match shape {
                egui::Shape::Text(text) if text.galley.text().ends_with(" Delete") => {
                    Some(text.visual_bounding_rect())
                }
                _ => None,
            })
            .max_by(|a, b| a.top().total_cmp(&b.top()))
            .expect("a row Delete")
            .center();
        for (frame, pressed) in [(30, true), (31, false)] {
            run(
                frame,
                vec![
                    egui::Event::PointerMoved(at),
                    egui::Event::PointerButton {
                        pos: at,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
        let fill = ctx.style().visuals.window_fill;
        for frame in 33..45 {
            let shapes = run(frame, Vec::new());
            let question = text(&shapes, &|t| t.starts_with("Delete ") && t.ends_with('?'))
                .expect("the delete question");
            let delete = text(&shapes, &|t| t == "Delete").expect("the Delete button");
            let cancel = text(&shapes, &|t| t == "Cancel").expect("the Cancel button");
            let popup = shapes
                .iter()
                .filter_map(|shape| match shape {
                    egui::Shape::Rect(rect)
                        if rect.fill == fill && rect.rect.contains(question.center()) =>
                    {
                        Some(rect.rect)
                    }
                    _ => None,
                })
                .min_by(|a, b| a.area().total_cmp(&b.area()))
                .expect("the popup");
            let centre = popup.center().x;
            let buttons = (delete.left() + cancel.right()) / 2.0;
            for (what, x) in [("question", question.center().x), ("buttons", buttons)] {
                assert!(
                    (x - centre).abs() < 1.5,
                    "frame {frame}: the {what} at {x}, the popup {popup:?} at {centre}"
                );
            }
        }
    }

    fn row_deletes(texts: &[(String, egui::Rect)]) -> Vec<egui::Rect> {
        texts
            .iter()
            .filter(|(text, _)| text.ends_with(" Delete"))
            .map(|(_, rect)| *rect)
            .collect()
    }

    #[test]
    fn a_phone_delete_ask_lifts_the_asked_row_above_the_sheet_and_holds_it_there() {
        let mut app = friend_autosaves_on_the_saves_page("peer-saves-lift");
        let ctx = egui::Context::default();
        let height = 700.0;
        let mut texts = Vec::new();
        for _ in 0..4 {
            frame_at(&mut app, &ctx, height, Vec::new());
        }
        let middle = egui::pos2(206.0, height / 2.0);
        frame_at(
            &mut app,
            &ctx,
            height,
            vec![
                egui::Event::PointerMoved(middle),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, -4000.0),
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        for _ in 0..30 {
            texts = frame_at(&mut app, &ctx, height, Vec::new());
        }
        let deletes = row_deletes(&texts);
        let pressed = deletes
            .iter()
            .copied()
            .filter(|rect| rect.bottom() < height - 80.0)
            .max_by(|a, b| a.top().total_cmp(&b.top()))
            .expect("a Delete in view");
        assert!(
            deletes.iter().all(|rect| rect.top() <= pressed.top()),
            "the last row's Delete {pressed:?} is not in view: {deletes:?}"
        );
        let at = pressed.center();
        let click = |pressed: bool| {
            vec![
                egui::Event::PointerMoved(at),
                egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ]
        };
        frame_at(&mut app, &ctx, height, click(true));
        frame_at(&mut app, &ctx, height, click(false));
        frame_at(&mut app, &ctx, height, Vec::new());
        let sheet = crate::app::mobile::sheet::above(&ctx).expect("the ask's sheet");
        assert!(
            pressed.bottom() > sheet.bottom(),
            "the row {pressed:?} starts clear of the sheet {sheet:?}, nothing to prove"
        );
        let asked = app
            .ui
            .snapshots
            .delete_question()
            .or(app.ui.snapshots.delete_autosave_question())
            .expect("the ask")
            .1;
        let mut lifted = None;
        for frame in 0..40 {
            texts = frame_at(&mut app, &ctx, height, Vec::new());
            let sheet = crate::app::mobile::sheet::above(&ctx).expect("the ask's sheet");
            let row = row_deletes(&texts)
                .into_iter()
                .max_by(|a, b| a.top().total_cmp(&b.top()))
                .expect("the asked row's Delete");
            if frame >= 20 {
                assert!(
                    row.bottom() <= sheet.bottom() - crate::app::mobile::sheet::KEEP_GAP + 0.5,
                    "frame {frame}: the row {row:?} under the sheet {sheet:?}"
                );
                assert_eq!(
                    lifted.get_or_insert(row),
                    &row,
                    "the row moved at frame {frame}"
                );
                let lit = crate::app::widgets::confirm::lit(&ctx).expect("the asked row lit");
                assert!(
                    lit.contains_rect(row),
                    "frame {frame}: the row {row:?} outside the lit {lit:?}"
                );
            }
        }
        assert!(
            texts.iter().any(|(text, _)| *text == asked),
            "{asked} in {texts:?}"
        );
    }

    #[test]
    fn the_saves_page_shows_one_time_style_and_the_delete_question_repeats_the_row_time() {
        let mut app = friend_autosaves_on_the_saves_page("peer-saves-one-style");
        let ctx = egui::Context::default();
        let mut texts = Vec::new();
        for _ in 0..4 {
            texts = tall_frame(&mut app, &ctx, Vec::new());
        }
        let now = crate::storage::now_unix();
        let day = crate::media::local_stamp(now, "%b %-d, ");
        let stamps: Vec<&(String, egui::Rect)> = texts
            .iter()
            .filter(|(text, _)| text.contains(day.as_str()))
            .collect();
        let words: Vec<&String> = texts
            .iter()
            .map(|(text, _)| text)
            .filter(|text| text.len() > 3)
            .collect();
        assert!(stamps.len() >= 4, "{words:?}");
        assert!(
            stamps
                .iter()
                .all(|(text, _)| text.matches(':').count() == 2),
            "{stamps:?}"
        );
        assert!(
            stamps
                .iter()
                .any(|(text, _)| text.starts_with(&format!("Saved {day}"))),
            "{stamps:?}"
        );
        assert!(!texts.iter().any(|(text, _)| text.contains("just now")));

        let (title, title_rect) = texts
            .iter()
            .find(|(text, _)| text.ends_with("'s autosave"))
            .expect("a friend's autosave row")
            .clone();
        let below = |wanted: &dyn Fn(&str) -> bool| {
            texts
                .iter()
                .filter(|(text, rect)| wanted(text) && rect.top() > title_rect.top())
                .min_by(|a, b| a.1.top().total_cmp(&b.1.top()))
                .expect("a text under the title")
                .clone()
        };
        let (stamp, _) = below(&|text| text.contains(day.as_str()));
        let (_, delete) = below(&|text| text.ends_with(" Delete"));
        let at = delete.center();
        tall_frame(
            &mut app,
            &ctx,
            vec![
                egui::Event::PointerMoved(at),
                egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        tall_frame(
            &mut app,
            &ctx,
            vec![egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        for _ in 0..3 {
            texts = tall_frame(&mut app, &ctx, Vec::new());
        }
        let question = format!("Delete {title} from {stamp}?");
        assert!(
            texts.iter().any(|(text, _)| *text == question),
            "{question} in {texts:?}"
        );
    }

    #[test]
    fn the_page_strip_holds_while_delete_asks_and_follows_the_toasts_once_it_closes() {
        let (mut app, _) = after_a_synced_session("peer-saves-strip-ask");
        let ctx = egui::Context::default();
        let settle = |app: &mut crate::app::Citrine| {
            let mut last = f32::NAN;
            for _ in 0..200 {
                tall_frame(app, &ctx, Vec::new());
                let now = crate::app::toasts::strip_room(&ctx);
                if now == last {
                    break;
                }
                last = now;
            }
            last
        };
        let notice = settle(&mut app);
        assert!(notice > 0.0, "no strip under the notice");
        let delete = app
            .toasts
            .press(&delete_label(&app))
            .expect("a Delete button");
        app.handle_event(delete, false);
        let mut texts = Vec::new();
        for _ in 0..30 {
            texts = tall_frame(&mut app, &ctx, Vec::new());
            assert_eq!(crate::app::toasts::strip_room(&ctx), notice);
        }
        let (_, button) = texts
            .iter()
            .filter(|(text, _)| text == "Delete")
            .max_by(|a, b| a.1.top().total_cmp(&b.1.top()))
            .expect("the Delete of the ask")
            .clone();
        let at = button.center();
        let press = |pressed: bool| {
            vec![
                egui::Event::PointerMoved(at),
                egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ]
        };
        tall_frame(&mut app, &ctx, press(true));
        assert_eq!(crate::app::toasts::strip_room(&ctx), notice);
        tall_frame(&mut app, &ctx, press(false));
        assert!(app.pending_peer_discard.is_none(), "the ask is still open");
        assert_eq!(crate::app::toasts::strip_room(&ctx), notice);
        let after = settle(&mut app);
        assert!(
            after > 0.0 && after < notice - 10.0,
            "{after} after {notice}"
        );
    }

    #[test]
    fn closing_the_notice_during_the_session_asks_again_only_if_it_saves_again() {
        let (mut app, own) = after_a_synced_session("peer-saves-close-running");
        let text = app.toasts.needing_action()[0].to_owned();
        let close = app.toasts.press_close(&text).expect("a close event");
        app.handle_event(close, false);
        assert!(app.toasts.needing_action().is_empty());

        write_sram(&mut app, 0x33);
        assert_eq!(saved(&mut app, &own), Some(0x11));
        app.emulator.unload();
        app.tell_peer_saves();
        assert_eq!(app.toasts.needing_action().len(), 1);
    }

    #[test]
    fn a_notice_left_unanswered_comes_back_after_a_relaunch() {
        let (mut app, own) = after_a_synced_session("peer-saves-relaunch");
        let borrowed = app.emulator.divert().expect("the divert").borrowed.clone();
        assert_eq!(saved(&mut app, &borrowed), Some(0x22));
        let keep_before = keep_label(&app);
        let mut again = relaunched(app);
        assert_eq!(again.toasts.needing_action().len(), 1);
        let keep = keep_label(&again);
        assert_eq!(keep, keep_before);
        let keep = again.toasts.press(&keep).expect("the keep button");
        again.handle_event(keep, false);
        again.emulator.writes.settle();
        assert_eq!(
            again.emulator.store.load_battery(&own).map(|ram| ram[0]),
            Some(0x22)
        );
        let after = relaunched(again);
        assert!(after.toasts.needing_action().is_empty());
    }

    #[test]
    fn a_deleted_notice_does_not_come_back_after_a_relaunch() {
        let (mut app, _) = after_a_synced_session("peer-saves-relaunch-deleted");
        let borrowed = app.emulator.divert().expect("the divert").borrowed.clone();
        assert_eq!(saved(&mut app, &borrowed), Some(0x22));
        app.emulator.unload();
        let delete = app
            .toasts
            .press(&delete_label(&app))
            .expect("a Delete button");
        app.handle_event(delete, false);
        let asked = app.pending_peer_discard.clone().expect("a pending delete");
        assert!(
            app.discard_question(&asked)
                .ends_with("Your own save is untouched."),
            "{}",
            app.discard_question(&asked)
        );
        app.finish_peer_discard(true);
        let again = relaunched(app);
        assert!(again.toasts.needing_action().is_empty());
        assert_eq!(again.emulator.store.load_battery(&borrowed), None);
    }

    #[test]
    fn delete_during_the_session_keeps_my_save_and_asks_again_when_it_ends() {
        let (mut app, own) = after_a_synced_session("peer-saves-delete-running");
        let borrowed = app.emulator.divert().expect("the divert").borrowed.clone();
        let delete = app
            .toasts
            .press(&delete_label(&app))
            .expect("a Delete button");
        app.handle_event(delete, false);
        let asked = app.pending_peer_discard.clone().expect("a pending delete");
        let name = app.friend_name(&asked);
        assert_eq!(
            app.discard_question(&asked),
            format!(
                "Delete {name}'s saves? Your own save is untouched. If {name} saves again, Citrine asks which save to keep."
            ),
        );
        app.finish_peer_discard(true);
        assert!(app.toasts.needing_action().is_empty());

        write_sram(&mut app, 0x33);
        assert_eq!(saved(&mut app, &own), Some(0x11));
        assert_eq!(saved(&mut app, &borrowed), Some(0x33));
        app.tell_peer_saves();
        assert!(app.toasts.needing_action().is_empty());

        app.emulator.unload();
        app.tell_peer_saves();
        assert_eq!(app.toasts.needing_action().len(), 1);
        let keep = keep_label(&app);
        let keep = app.toasts.press(&keep).expect("the keep button");
        app.handle_event(keep, false);
        app.emulator.writes.settle();
        assert_eq!(
            app.emulator.store.load_battery(&own).map(|ram| ram[0]),
            Some(0x33)
        );
    }

    #[test]
    fn the_kept_saves_message_fades_by_itself() {
        let (mut app, _) = after_a_synced_session("peer-saves-kept-fades");
        let keep = keep_label(&app);
        let keep = app.toasts.press(&keep).expect("the keep button");
        app.handle_event(keep, false);
        assert!(app.toasts.button_labels().contains(&"Undo"));
        app.toasts.age(60.0);
        assert!(app.toasts.texts().is_empty(), "{:?}", app.toasts.texts());
    }

    #[test]
    fn the_synced_game_notice_stays_until_answered() {
        let (mut app, _) = after_a_synced_session("peer-saves-notice-stays");
        app.toasts.age(600.0);
        app.toasts.dismiss_all_toasts();
        assert_eq!(app.toasts.needing_action().len(), 1);
    }

    #[test]
    fn delete_asks_first_and_cancel_brings_the_notice_back() {
        let (mut app, own) = after_a_synced_session("peer-saves-delete");
        let divert = app.emulator.divert().expect("the divert").clone();
        let delete = app
            .toasts
            .press(&delete_label(&app))
            .expect("a Delete button");
        app.handle_event(delete, false);
        assert!(app.pending_peer_discard.is_some());
        app.finish_peer_discard(false);
        assert_eq!(app.toasts.needing_action().len(), 1);
        assert!(saved(&mut app, &divert.borrowed).is_some());

        let delete = app
            .toasts
            .press(&delete_label(&app))
            .expect("a Delete button");
        app.handle_event(delete, false);
        app.emulator.writes.settle();
        app.finish_peer_discard(true);
        assert_eq!(app.emulator.store.load_battery(&divert.borrowed), None);
        assert_eq!(
            app.emulator.store.load_battery(&own).map(|ram| ram[0]),
            Some(0x11)
        );
    }
}
