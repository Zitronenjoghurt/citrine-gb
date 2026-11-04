use super::Citrine;
use crate::emulator::gimmicks::{CameraSource, camera, rumble_step};
use crate::senses::Wanted;
use crate::utils::file_loader::{FileLoader, PickedFile};

pub const PICTURE_TYPES: [&str; 5] = ["png", "jpg", "jpeg", "webp", "bmp"];

impl Citrine {
    pub(super) fn sense_gimmicks(&mut self, ctx: &egui::Context) {
        let cart = self.emulator.cart();
        let settings = &self.ui.settings.gimmicks;
        let running = self.emulator.is_running();
        self.senses.follow(Wanted::of(cart, settings, running));
        if cart.motion() {
            self.emulator.gimmicks.sensed = self.senses.motion(self.pads.as_mut());
            let seconds = ctx.input(|i| i.stable_dt);
            self.emulator.gimmicks.update_motion(seconds);
        }
        if cart.light {
            self.emulator.gimmicks.light = settings.light_level(self.senses.light());
        }
        if cart.camera
            && let Some(frame) = self
                .senses
                .take_camera_frame(settings.camera, settings.camera_view)
        {
            self.emulator.gimmicks.set_camera_frame(frame);
        }
    }

    pub(super) fn use_gimmicks(&mut self) {
        let level = if self.ui.settings.gimmicks.rumble {
            self.emulator.gimmicks.rumble()
        } else {
            0.0
        };
        self.senses.rumble(self.pads.as_mut(), rumble_step(level));
        for print in self.emulator.gimmicks.take_prints() {
            match crate::media::save_print(&self.emulator, &mut self.media, &print) {
                Ok(item) => self
                    .events
                    .push(super::events::AppEvent::MediaAdded { id: item.id }),
                Err(err) => {
                    self.toasts.error(err);
                }
            }
        }
    }

    pub(crate) fn pick_camera_picture(&self) {
        FileLoader::new()
            .title("Choose a picture")
            .add_filter("Pictures", &PICTURE_TYPES)
            .dispatch(self.files.picture_tx.clone());
    }

    pub(super) fn picture_picked(&mut self, file: PickedFile) {
        let Some(frame) = camera::from_picture(&file.data) else {
            self.toasts
                .error(format!("{} isn't a picture Citrine can read", file.name));
            return;
        };
        self.senses.set_picture(frame);
        let settings = &mut self.ui.settings;
        if settings.gimmicks.camera != CameraSource::Picture {
            settings.gimmicks.camera = CameraSource::Picture;
            settings.dirty = true;
        }
    }

    pub(crate) fn centre_tilt(&mut self) {
        self.senses.centre();
    }
}
