use super::bubble::long_pressed;
use super::palette::shared_palette;
use super::{Action, ChatUi};
use crate::app::ui_state::palette::PaletteLibrary;
use crate::icons;
use crate::online::Online;
use crate::online::chat::ChatEntry;
use citrine_gb_net::{Body, OnlineId};
use egui::containers::menu::menu_style;
use egui::{Align, Layout, Popup, PopupKind, Rect, RectAlign, SetOpenCommand, Ui, vec2};

const MORE_SIDE: f32 = 24.0;
const MORE_GAP: f32 = 4.0;

struct Items<'a> {
    copy: Option<&'a str>,
    edit: bool,
    delete: bool,
    add_palette: bool,
}

impl<'a> Items<'a> {
    fn of(
        online: &Online,
        friend: OnlineId,
        entry: &'a ChatEntry,
        palettes: &PaletteLibrary,
    ) -> Self {
        let own = entry.mine && !entry.is_deleted() && online.can_chat(friend);
        Self {
            copy: match &entry.body {
                Body::Text(text) if !entry.is_deleted() => Some(text),
                _ => None,
            },
            edit: own && online.can_edit(friend, entry.id),
            delete: own,
            add_palette: !entry.mine
                && shared_palette(entry).is_some_and(|p| palettes.with_colors(&p.colors).is_none()),
        }
    }

    fn any(&self) -> bool {
        self.copy.is_some() || self.edit || self.delete || self.add_palette
    }
}

pub(super) fn menu_id(entry: u64) -> egui::Id {
    egui::Id::new(("chat_menu", entry))
}

#[allow(clippy::too_many_arguments)]
pub(super) fn message_menu(
    ui: &mut Ui,
    state: &mut ChatUi,
    online: &Online,
    friend: OnlineId,
    entry: &ChatEntry,
    bubble: Rect,
    palettes: &PaletteLibrary,
    actions: &mut Vec<Action>,
) {
    let items = Items::of(online, friend, entry, palettes);
    if !items.any() {
        return;
    }
    let id = menu_id(entry.id);
    let open = Popup::is_id_open(ui.ctx(), id);
    let (secondary, held, down) = ui.input(|i| {
        (
            i.pointer.secondary_clicked(),
            long_pressed(i, bubble),
            i.pointer.primary_down(),
        )
    });
    let mut command = None;
    if secondary && ui.rect_contains_pointer(bubble) {
        command = Some(SetOpenCommand::Bool(true));
    }
    if held {
        state.held = Some(entry.id);
    } else if !down && state.held == Some(entry.id) {
        state.held = None;
        command = Some(SetOpenCommand::Bool(true));
    }
    if !state.touch {
        let side = if entry.mine { -1.0 } else { 1.0 };
        let reach = MORE_SIDE + MORE_GAP;
        let row = bubble.union(bubble.translate(vec2(side * reach, 0.0)));
        if open || ui.rect_contains_pointer(row) {
            let edge = if entry.mine {
                bubble.left() - MORE_GAP - MORE_SIDE / 2.0
            } else {
                bubble.right() + MORE_GAP + MORE_SIDE / 2.0
            };
            let middle = bubble.top() + bubble.height().min(MORE_SIDE * 1.5) / 2.0;
            let spot = Rect::from_center_size(egui::pos2(edge, middle), vec2(MORE_SIDE, MORE_SIDE));
            let more = ui
                .new_child(egui::UiBuilder::new().max_rect(spot))
                .add_sized(
                    spot.size(),
                    egui::Button::new(icons::DOTS_THREE)
                        .frame_when_inactive(false)
                        .selected(open),
                )
                .on_hover_text("More");
            if more.clicked() {
                command = Some(SetOpenCommand::Toggle);
            }
        }
    }
    let align = if entry.mine {
        RectAlign::BOTTOM_END
    } else {
        RectAlign::BOTTOM_START
    };
    Popup::new(id, ui.ctx().clone(), bubble, ui.layer_id())
        .kind(PopupKind::Menu)
        .layout(Layout::top_down_justified(Align::Min))
        .style(menu_style)
        .align(align)
        .gap(4.0)
        .open_memory(command)
        .show(|ui| {
            if let Some(text) = items.copy
                && ui.button(format!("{}  Copy text", icons::COPY)).clicked()
            {
                ui.ctx().copy_text(text.to_owned());
            }
            if items.add_palette
                && ui
                    .button(format!("{}  Add to palettes", icons::PALETTE))
                    .clicked()
            {
                actions.push(Action::AddPalette(entry.id));
            }
            if items.edit
                && ui
                    .button(format!("{}  Edit", icons::PENCIL_SIMPLE))
                    .clicked()
            {
                actions.push(Action::Edit(entry.id));
            }
            if items.delete && ui.button(format!("{}  Delete", icons::TRASH)).clicked() {
                actions.push(Action::AskDelete(entry.id));
            }
        });
}
