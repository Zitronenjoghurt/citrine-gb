use crate::app::widgets::text::faint;
use crate::icons;
use crate::update::{Phase, Tap, Updater};
use egui::{Align, Context, Layout, ProgressBar, RichText, Ui};

pub struct Texts {
    pub title: String,
    pub detail: String,
    pub action: Option<&'static str>,
    pub progress: Option<f32>,
}

fn megabytes(bytes: u64) -> String {
    format!("{:.1}", bytes as f64 / 1_000_000.0)
}

pub fn texts(phase: &Phase, android: bool) -> Option<Texts> {
    let offer = phase.offer()?;
    let version = &offer.version;
    let page = (!offer.installable).then_some("Release page");
    let texts = match phase {
        Phase::Idle => return None,
        Phase::Available(_) => Texts {
            title: format!("Citrine {version} is out"),
            detail: if offer.size > 0 {
                format!("{} MB download", megabytes(offer.size))
            } else {
                String::new()
            },
            action: page.or(Some("Update")),
            progress: None,
        },
        Phase::Downloading { done, total, .. } => Texts {
            title: format!("Downloading Citrine {version}"),
            detail: if *total > 0 {
                format!("{} of {} MB", megabytes(*done), megabytes(*total))
            } else {
                format!("{} MB", megabytes(*done))
            },
            action: None,
            progress: Some(if *total > 0 {
                (*done as f32 / *total as f32).clamp(0.0, 1.0)
            } else {
                0.0
            }),
        },
        Phase::Ready(_) if android => Texts {
            title: "Update downloaded".to_owned(),
            detail: "Android asks to confirm".to_owned(),
            action: Some("Install"),
            progress: None,
        },
        Phase::Ready(_) => Texts {
            title: "Update installed".to_owned(),
            detail: "Your game is saved first".to_owned(),
            action: Some("Restart"),
            progress: None,
        },
        Phase::Failed { reason, retry, .. } => Texts {
            title: "Update didn't install".to_owned(),
            detail: reason.clone(),
            action: Some(if *retry && offer.installable {
                "Try again"
            } else {
                "Release page"
            }),
            progress: None,
        },
    };
    Some(texts)
}

pub fn texts_now(updater: &Updater) -> Option<Texts> {
    texts(&updater.phase(), cfg!(target_os = "android"))
}

pub fn follow(ctx: &Context, tap: Tap) {
    match tap {
        Tap::Nothing => {}
        Tap::Open(url) => ctx.open_url(egui::OpenUrl::new_tab(url)),
        Tap::Restart => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
    }
}

pub fn bar(ui: &mut Ui, progress: f32) {
    ui.add(ProgressBar::new(progress).desired_height(6.0));
}

pub fn card(ui: &mut Ui, updater: &mut Updater) {
    let Some(texts) = texts_now(updater) else {
        return;
    };
    let mut tap = Tap::Nothing;
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.label(RichText::new(icons::ARROW_CIRCLE_UP).size(20.0));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if let Some(action) = texts.action
                    && ui.button(action).clicked()
                {
                    tap = updater.tap();
                }
                ui.with_layout(Layout::top_down(Align::Min), |ui| {
                    ui.label(RichText::new(&texts.title).strong());
                    if !texts.detail.is_empty() {
                        ui.label(faint(&texts.detail));
                    }
                });
            });
        });
        if let Some(progress) = texts.progress {
            bar(ui, progress);
        }
    });
    ui.add_space(6.0);
    follow(ui.ctx(), tap);
}
