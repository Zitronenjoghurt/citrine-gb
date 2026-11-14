use crate::covers::net::{Answer, Net};
use crate::storage::Backend;
use egui::{Context, TextureHandle};
use std::collections::HashMap;
use std::sync::mpsc::{Receiver, TryRecvError};

const NAMESPACE: &str = "badges";
const IN_FLIGHT: usize = 6;
const OPENS_PER_FRAME: usize = 8;

enum Badge {
    Waiting,
    Loading(Receiver<Answer>),
    Ready(TextureHandle),
    Missing,
}

pub struct Badges {
    net: Box<dyn Net>,
    cache: Backend,
    badges: HashMap<String, Badge>,
    opened: usize,
    frame: u64,
}

impl Badges {
    pub fn new(net: Box<dyn Net>, cache: Backend) -> Self {
        Self {
            net,
            cache,
            badges: HashMap::new(),
            opened: 0,
            frame: 0,
        }
    }

    pub fn get(&mut self, ctx: &Context, url: &str) -> Option<TextureHandle> {
        let frame = ctx.cumulative_frame_nr();
        if frame != self.frame {
            self.frame = frame;
            self.opened = 0;
        }
        let name = file_name(url)?;
        let in_flight = self
            .badges
            .values()
            .filter(|badge| matches!(badge, Badge::Loading(_)))
            .count();
        let badge = self.badges.entry(url.to_owned()).or_insert(Badge::Waiting);
        if matches!(badge, Badge::Waiting) && self.opened < OPENS_PER_FRAME {
            self.opened += 1;
            *badge = match self.cache.read(NAMESPACE, name) {
                Some(png) => texture(ctx, name, &png).map_or(Badge::Missing, Badge::Ready),
                None if in_flight < IN_FLIGHT => Badge::Loading(self.net.get(url.to_owned())),
                None => Badge::Waiting,
            };
        }
        if let Badge::Loading(reply) = badge {
            *badge = match reply.try_recv() {
                Ok(Ok(png)) => {
                    if let Err(err) = self.cache.write(NAMESPACE, name, &png) {
                        log::warn!("could not keep badge {name}: {err}");
                    }
                    texture(ctx, name, &png).map_or(Badge::Missing, Badge::Ready)
                }
                Ok(Err(err)) => {
                    log::warn!("could not fetch badge {name}: {err}");
                    Badge::Missing
                }
                Err(TryRecvError::Disconnected) => Badge::Missing,
                Err(TryRecvError::Empty) => {
                    ctx.request_repaint_after(std::time::Duration::from_millis(100));
                    return None;
                }
            };
        }
        match badge {
            Badge::Ready(texture) => Some(texture.clone()),
            Badge::Waiting => {
                ctx.request_repaint();
                None
            }
            _ => None,
        }
    }

    pub fn done(&self, url: &str) -> bool {
        file_name(url).is_none()
            || matches!(self.badges.get(url), Some(Badge::Ready(_) | Badge::Missing))
    }
}

fn file_name(url: &str) -> Option<&str> {
    let name = url.rsplit('/').next()?;
    let fine = !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
        && !name.starts_with('.');
    fine.then_some(name)
}

fn texture(ctx: &Context, name: &str, png: &[u8]) -> Option<TextureHandle> {
    let frame = crate::media::decode_png(png)?;
    let size = [frame.width as usize, frame.height as usize];
    let image = egui::ColorImage::from_rgba_unmultiplied(size, &frame.rgba);
    Some(ctx.load_texture(format!("badge_{name}"), image, egui::TextureOptions::LINEAR))
}

#[cfg(test)]
mod tests {
    use super::file_name;

    #[test]
    fn a_badge_keeps_its_own_file_name() {
        assert_eq!(
            file_name("https://media.retroachievements.org/Badge/00234_lock.png"),
            Some("00234_lock.png")
        );
    }

    #[test]
    fn a_strange_name_is_never_a_path() {
        assert_eq!(file_name("https://x/Badge/..png"), None);
        assert_eq!(file_name("https://x/Badge/a%2Fb.png"), None);
        assert_eq!(file_name("https://x/Badge/"), None);
    }
}
