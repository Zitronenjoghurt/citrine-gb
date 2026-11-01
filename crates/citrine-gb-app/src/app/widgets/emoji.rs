use egui::emath::GuiRounding;
use egui::text::{CCursor, LayoutJob, TextFormat};
use egui::{
    Color32, Galley, Painter, Rect, Response, Sense, TextStyle, TextureHandle, Ui, Vec2, pos2,
};
use std::collections::HashMap;
use std::ops::Range;
use std::sync::{Arc, OnceLock};

static BUNDLE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/emoji.br"));
static EXTRA_FONT: &[u8] = include_bytes!("../../../assets/fonts/NotoEmojiExtra.ttf");
const EXTRA_FONT_NAME: &str = "noto-emoji-extra";
const VARIATION: char = '\u{FE0F}';
const SKIN_TONES: std::ops::RangeInclusive<char> = '\u{1F3FB}'..='\u{1F3FF}';

pub const EMOJI: [(&str, &str); 111] = [
    ("😀", "grinning face"),
    ("😃", "grinning face with big eyes"),
    ("😄", "grinning face with smiling eyes"),
    ("😁", "beaming face with smiling eyes"),
    ("😆", "grinning squinting face"),
    ("😅", "grinning face with sweat"),
    ("🤣", "rolling on the floor laughing"),
    ("😂", "face with tears of joy"),
    ("🙂", "slightly smiling face"),
    ("🙃", "upside-down face"),
    ("🫠", "melting face"),
    ("😉", "winking face"),
    ("😊", "smiling face with smiling eyes"),
    ("😇", "smiling face with halo"),
    ("🥰", "smiling face with hearts"),
    ("😍", "smiling face with heart-eyes"),
    ("🤩", "star-struck"),
    ("😘", "face blowing a kiss"),
    ("😗", "kissing face"),
    ("☺️", "smiling face"),
    ("😚", "kissing face with closed eyes"),
    ("😙", "kissing face with smiling eyes"),
    ("🥲", "smiling face with tear"),
    ("😋", "face savoring food"),
    ("😛", "face with tongue"),
    ("😜", "winking face with tongue"),
    ("🤪", "zany face"),
    ("😝", "squinting face with tongue"),
    ("🤑", "money-mouth face"),
    ("🤗", "hugging face"),
    ("🤭", "face with hand over mouth"),
    ("🫢", "face with open eyes and hand over mouth"),
    ("🫣", "face with peeking eye"),
    ("🤫", "shushing face"),
    ("🤔", "thinking face"),
    ("🫡", "saluting face"),
    ("🤐", "zipper-mouth face"),
    ("🤨", "face with raised eyebrow"),
    ("😐", "neutral face"),
    ("😑", "expressionless face"),
    ("😶", "face without mouth"),
    ("🫥", "dotted line face"),
    ("😶‍🌫️", "face in clouds"),
    ("😏", "smirking face"),
    ("😒", "unamused face"),
    ("🙄", "face with rolling eyes"),
    ("😬", "grimacing face"),
    ("😮‍💨", "face exhaling"),
    ("🤥", "lying face"),
    ("🫨", "shaking face"),
    ("🙂‍↔️", "head shaking horizontally"),
    ("🙂‍↕️", "head shaking vertically"),
    ("😌", "relieved face"),
    ("😔", "pensive face"),
    ("😪", "sleepy face"),
    ("🤤", "drooling face"),
    ("😴", "sleeping face"),
    ("😷", "face with medical mask"),
    ("🤒", "face with thermometer"),
    ("🤕", "face with head bandage"),
    ("🤢", "nauseated face"),
    ("🤮", "face vomiting"),
    ("🤧", "sneezing face"),
    ("🥵", "hot face"),
    ("🥶", "cold face"),
    ("🥴", "woozy face"),
    ("😵", "dizzy face"),
    ("😵‍💫", "face with spiral eyes"),
    ("🤯", "exploding head"),
    ("🤠", "cowboy hat face"),
    ("🥳", "partying face"),
    ("🥸", "disguised face"),
    ("😎", "smiling face with sunglasses"),
    ("🤓", "nerd face"),
    ("🧐", "face with monocle"),
    ("😕", "confused face"),
    ("🫤", "face with diagonal mouth"),
    ("😟", "worried face"),
    ("🙁", "slightly frowning face"),
    ("☹️", "frowning face"),
    ("😮", "face with open mouth"),
    ("😯", "hushed face"),
    ("😲", "astonished face"),
    ("😳", "flushed face"),
    ("🥺", "pleading face"),
    ("🥹", "face holding back tears"),
    ("😦", "frowning face with open mouth"),
    ("😧", "anguished face"),
    ("😨", "fearful face"),
    ("😰", "anxious face with sweat"),
    ("😥", "sad but relieved face"),
    ("😢", "crying face"),
    ("😭", "loudly crying face"),
    ("😱", "face screaming in fear"),
    ("😖", "confounded face"),
    ("😣", "persevering face"),
    ("😞", "disappointed face"),
    ("😓", "downcast face with sweat"),
    ("😩", "weary face"),
    ("😫", "tired face"),
    ("🥱", "yawning face"),
    ("😤", "face with steam from nose"),
    ("😡", "pouting face"),
    ("😠", "angry face"),
    ("🤬", "face with symbols on mouth"),
    ("😈", "smiling face with horns"),
    ("👿", "angry face with horns"),
    ("💀", "skull"),
    ("💩", "pile of poo"),
    ("🤡", "clown face"),
    ("🤖", "robot"),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Piece<'a> {
    Text(&'a str),
    Emoji(usize),
}

pub fn add_font(fonts: &mut egui::FontDefinitions) {
    fonts.font_data.insert(
        EXTRA_FONT_NAME.to_owned(),
        Arc::new(egui::FontData::from_static(EXTRA_FONT)),
    );
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts
            .families
            .entry(family)
            .or_default()
            .push(EXTRA_FONT_NAME.to_owned());
    }
}

fn prefix_len(text: &str, glyph: &str) -> Option<usize> {
    let mut want = glyph.chars().filter(|c| *c != VARIATION).peekable();
    let mut len = 0;
    for c in text.chars() {
        let Some(next) = want.peek() else {
            break;
        };
        if c == VARIATION && len > 0 {
            len += c.len_utf8();
        } else if c == *next {
            want.next();
            len += c.len_utf8();
        } else {
            return None;
        }
    }
    want.peek().is_none().then_some(len)
}

fn match_at(text: &str) -> Option<(usize, usize)> {
    let (index, mut len) = EMOJI
        .iter()
        .enumerate()
        .filter_map(|(i, (glyph, _))| prefix_len(text, glyph).map(|len| (i, len, glyph.len())))
        .max_by_key(|(_, _, glyph_len)| *glyph_len)
        .map(|(i, len, _)| (i, len))?;
    for c in text[len..].chars() {
        if c == VARIATION || SKIN_TONES.contains(&c) {
            len += c.len_utf8();
        } else {
            break;
        }
    }
    Some((index, len))
}

fn spans(text: &str) -> Vec<(Range<usize>, usize)> {
    let mut out = Vec::new();
    let mut at = 0;
    while at < text.len() {
        if let Some((index, len)) = match_at(&text[at..]) {
            out.push((at..at + len, index));
            at += len;
        } else {
            at += text[at..].chars().next().map_or(1, char::len_utf8);
        }
    }
    out
}

pub fn pieces(text: &str) -> Vec<Piece<'_>> {
    let mut out = Vec::new();
    let mut start = 0;
    for (range, index) in spans(text) {
        if start < range.start {
            out.push(Piece::Text(&text[start..range.start]));
        }
        out.push(Piece::Emoji(index));
        start = range.end;
    }
    if start < text.len() {
        out.push(Piece::Text(&text[start..]));
    }
    out
}

pub fn only_emoji(pieces: &[Piece]) -> Option<usize> {
    let mut count = 0;
    for piece in pieces {
        match piece {
            Piece::Emoji(_) => count += 1,
            Piece::Text(text) if text.trim().is_empty() => {}
            Piece::Text(_) => return None,
        }
    }
    (count > 0).then_some(count)
}

fn key(glyph: &str) -> String {
    glyph
        .chars()
        .filter(|c| *c != VARIATION)
        .map(|c| format!("{:x}", c as u32))
        .collect::<Vec<_>>()
        .join("-")
}

fn sources() -> &'static [String] {
    static SOURCES: OnceLock<Vec<String>> = OnceLock::new();
    SOURCES.get_or_init(|| {
        let mut bundle = String::new();
        if std::io::Read::read_to_string(&mut brotli::Decompressor::new(BUNDLE, 4096), &mut bundle)
            .is_err()
        {
            return Vec::new();
        }
        let files: HashMap<&str, &str> = bundle
            .lines()
            .collect::<Vec<_>>()
            .as_chunks::<2>()
            .0
            .iter()
            .map(|[glyph, file]| (*glyph, *file))
            .collect();
        EMOJI
            .iter()
            .map(|(glyph, _)| {
                files
                    .get(key(glyph).as_str())
                    .copied()
                    .unwrap_or_default()
                    .to_string()
            })
            .collect()
    })
}

fn render(index: usize, px: u32) -> Option<egui::ColorImage> {
    let svg = sources().get(index)?;
    let tree = resvg::usvg::Tree::from_str(svg, &resvg::usvg::Options::default()).ok()?;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(px, px)?;
    let scale = px as f32 / tree.size().width().max(tree.size().height());
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    Some(egui::ColorImage::from_rgba_premultiplied(
        [px as usize, px as usize],
        pixmap.data(),
    ))
}

type Textures = HashMap<(usize, u32), TextureHandle>;

fn texture(ctx: &egui::Context, index: usize, px: u32) -> Option<TextureHandle> {
    let id = egui::Id::new("emoji_textures");
    if let Some(handle) = ctx.data(|d| {
        d.get_temp::<Textures>(id)
            .and_then(|t| t.get(&(index, px)).cloned())
    }) {
        return Some(handle);
    }
    let image = render(index, px)?;
    let handle = ctx.load_texture(
        format!("emoji_{index}_{px}"),
        image,
        egui::TextureOptions::LINEAR,
    );
    ctx.data_mut(|d| {
        d.get_temp_mut_or_default::<Textures>(id)
            .insert((index, px), handle.clone())
    });
    Some(handle)
}

pub fn paint(ui: &Ui, rect: Rect, index: usize) {
    paint_on(ui.painter(), rect, index);
}

fn paint_on(painter: &Painter, rect: Rect, index: usize) {
    let ppp = painter.ctx().pixels_per_point();
    let px = (rect.width().min(rect.height()) * ppp).round().max(1.0) as u32;
    let Some(handle) = texture(painter.ctx(), index, px) else {
        return;
    };
    let side = Vec2::splat(px as f32 / ppp);
    let min = (rect.center() - side / 2.0).round_to_pixels(ppp);
    painter.image(
        handle.id(),
        Rect::from_min_size(min, side),
        Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
        Color32::WHITE,
    );
}

fn field_side(ui: &Ui) -> f32 {
    ui.text_style_height(&TextStyle::Body)
}

pub fn field_galley(ui: &Ui, text: &str, wrap_width: f32) -> Arc<Galley> {
    let color = ui
        .visuals()
        .override_text_color
        .unwrap_or_else(|| ui.visuals().widgets.inactive.text_color());
    spaced_galley(ui, text, TextStyle::Body, color, wrap_width)
}

fn spaced_galley(
    ui: &Ui,
    text: &str,
    style: TextStyle,
    color: Color32,
    wrap_width: f32,
) -> Arc<Galley> {
    let font = style.resolve(ui.style());
    let side = ui.text_style_height(&style);
    let plain = TextFormat::simple(font.clone(), color);
    let hidden = TextFormat::simple(font.clone(), Color32::TRANSPARENT);
    let mut job = LayoutJob::default();
    job.wrap.max_width = wrap_width;
    let mut at = 0;
    for (range, _) in spans(text) {
        if at < range.start {
            job.append(&text[at..range.start], 0.0, plain.clone());
        }
        let glyphs = &text[range.clone()];
        let mut format = TextFormat {
            line_height: Some(side),
            ..hidden.clone()
        };
        for _ in 0..2 {
            let width = ui.fonts_mut(|f| {
                f.layout_no_wrap(
                    glyphs.to_owned(),
                    format.font_id.clone(),
                    Color32::TRANSPARENT,
                )
                .size()
                .x
            });
            if width > 0.1 {
                format.font_id.size *= side / width;
            }
        }
        job.append(glyphs, 0.0, format);
        at = range.end;
    }
    if at < text.len() || text.is_empty() {
        job.append(&text[at..], 0.0, plain);
    }
    ui.fonts_mut(|f| f.layout_job(job))
}

pub fn line(ui: &mut Ui, text: &str, style: TextStyle, color: Color32) -> Response {
    let measure = |t: &str| {
        spaced_galley(ui, t, style.clone(), color, f32::INFINITY)
            .size()
            .x
    };
    let shown = crate::app::tabs::shorten(text, ui.available_width(), measure);
    let galley = spaced_galley(ui, &shown, style.clone(), color, f32::INFINITY);
    let (rect, response) = ui.allocate_exact_size(galley.size(), Sense::click());
    if ui.is_rect_visible(rect) {
        let side = ui.text_style_height(&style);
        let painter = ui.painter();
        for (spot, index) in picture_rects(&galley, &shown, side) {
            paint_on(painter, spot.translate(rect.min.to_vec2()), index);
        }
        painter.galley(rect.min, galley, color);
    }
    if shown != text {
        response.on_hover_text(text)
    } else {
        response
    }
}

pub fn paint_field(ui: &Ui, output: &egui::text_edit::TextEditOutput, text: &str) {
    let painter = ui
        .painter()
        .with_clip_rect(output.text_clip_rect.intersect(ui.clip_rect()));
    let offset = output.galley_pos.to_vec2();
    for (rect, index) in picture_rects(&output.galley, text, field_side(ui)) {
        paint_on(&painter, rect.translate(offset), index);
    }
}

fn picture_rects(galley: &Galley, text: &str, side: f32) -> Vec<(Rect, usize)> {
    spans(text)
        .into_iter()
        .map(|(range, index)| {
            let after = text[..range.end].chars().count();
            let end = galley.pos_from_cursor(CCursor::new(after));
            let span = Rect::from_x_y_ranges((end.min.x - side)..=end.min.x, end.y_range());
            (
                Rect::from_center_size(span.center(), Vec2::splat(side)),
                index,
            )
        })
        .collect()
}

pub fn image(ui: &mut Ui, index: usize, size: f32) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    if ui.is_rect_visible(rect) {
        paint(ui, rect, index);
    }
    response
}

pub fn picker(ui: &mut Ui, size: f32) -> Option<&'static str> {
    let mut picked = None;
    let spacing = 2.0;
    let pad = 4.0;
    let room = ui.available_width() + spacing;
    let columns = (room / (size + pad * 2.0 + spacing))
        .floor()
        .clamp(4.0, 8.0);
    let cell = (room / columns - spacing).min(size + pad * 2.0);
    let columns = columns as usize;
    egui::Grid::new("emoji_picker")
        .spacing(Vec2::splat(spacing))
        .min_col_width(cell)
        .max_col_width(cell)
        .min_row_height(cell)
        .show(ui, |ui| {
            for (i, (glyph, name)) in EMOJI.iter().enumerate() {
                let (rect, response) = ui.allocate_exact_size(Vec2::splat(cell), Sense::click());
                let response = response
                    .on_hover_text(*name)
                    .on_hover_cursor(egui::CursorIcon::PointingHand);
                if response.hovered() || response.has_focus() {
                    ui.painter()
                        .rect_filled(rect, 6.0, ui.visuals().widgets.hovered.weak_bg_fill);
                }
                paint(ui, rect.shrink(pad), i);
                if response.clicked() {
                    picked = Some(*glyph);
                }
                if (i + 1) % columns == 0 {
                    ui.end_row();
                }
            }
        });
    picked
}

#[cfg(test)]
mod tests {
    use super::*;

    const EGUI_REPLACEMENT: char = '\u{25FB}';
    const NEWER_THAN_THE_FONT: &str = "1F6D8 1F6D9 1FA89 1FA8A 1FA8B 1FA8C 1FA8D 1FA8E 1FA8F 1FABE 1FAC6 1FAC8 1FACC 1FACD 1FADC 1FADD 1FADF 1FAE9 1FAEA 1FAEB 1FAEF 1FAF9 1FAFA";

    fn code_points(text: &str) -> Vec<char> {
        text.lines()
            .filter(|line| !line.starts_with('#'))
            .flat_map(str::split_whitespace)
            .map(|hex| char::from_u32(u32::from_str_radix(hex, 16).unwrap()).unwrap())
            .collect()
    }

    fn context_with_the_font() -> egui::Context {
        let ctx = egui::Context::default();
        let mut fonts = egui::FontDefinitions::default();
        add_font(&mut fonts);
        ctx.set_fonts(fonts);
        let _ = ctx.run(Default::default(), |_| {});
        ctx
    }

    #[test]
    fn a_skin_tone_after_an_emoji_draws_no_swatch_beside_it() {
        let ctx = context_with_the_font();
        let style = egui::FontId::proportional(14.0);
        for tone in '\u{1F3FB}'..='\u{1F3FF}' {
            let width = ctx.fonts_mut(|fonts| fonts.glyph_width(&style, tone));
            assert_eq!(width, 0.0, "{:X}", tone as u32);
        }
    }

    #[test]
    fn every_emoji_up_to_unicode_15_1_has_a_glyph_and_never_a_box() {
        let ctx = context_with_the_font();
        let newer = code_points(NEWER_THAN_THE_FONT);
        let all = code_points(include_str!("../../../assets/fonts/emoji-code-points.txt"));
        assert!(all.len() > 1400);
        for style in [
            egui::FontId::proportional(14.0),
            egui::FontId::monospace(14.0),
        ] {
            let missing: Vec<String> = ctx.fonts_mut(|fonts| {
                all.iter()
                    .filter(|c| **c != EGUI_REPLACEMENT)
                    .filter(|c| !newer.contains(c) && !fonts.has_glyph(&style, **c))
                    .map(|c| format!("{:X}", *c as u32))
                    .collect()
            });
            assert!(missing.is_empty(), "{style:?} has no glyph for {missing:?}");
        }
    }

    #[test]
    fn every_emoji_has_its_picture() {
        assert_eq!(sources().len(), EMOJI.len());
        for (i, (glyph, _)) in EMOJI.iter().enumerate() {
            assert!(sources()[i].starts_with("<svg"), "{glyph}");
        }
    }

    #[test]
    fn emoji_render_at_the_asked_size_with_soft_edges_and_no_fringe() {
        for px in [20, 40, 53] {
            let image = render(0, px).unwrap();
            assert_eq!(image.size, [px as usize, px as usize]);
            assert_eq!(image.pixels[0].a(), 0);
            assert!(image.pixels.iter().any(|p| p.a() == 255));
            assert!(image.pixels.iter().any(|p| p.a() > 0 && p.a() < 255));
            for p in &image.pixels {
                assert!(p.r() <= p.a() && p.g() <= p.a() && p.b() <= p.a());
            }
        }
    }

    #[test]
    fn every_glyph_is_one_emoji_and_unique() {
        for (i, (glyph, _)) in EMOJI.iter().enumerate() {
            assert_eq!(pieces(glyph), vec![Piece::Emoji(i)], "{glyph}");
            let bare: String = glyph.chars().filter(|c| *c != VARIATION).collect();
            assert_eq!(pieces(&bare), vec![Piece::Emoji(i)], "{glyph}");
        }
        let mut keys: Vec<String> = EMOJI.iter().map(|(g, _)| key(g)).collect();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), EMOJI.len());
    }

    #[test]
    fn typed_emoji_split_out_of_text() {
        let joy = EMOJI.iter().position(|(g, _)| *g == "😂").unwrap();
        let smile = EMOJI.iter().position(|(g, _)| *g == "🙂").unwrap();
        let shake = EMOJI
            .iter()
            .position(|(g, _)| *g == "🙂\u{200D}↔\u{FE0F}")
            .unwrap();
        let warm = EMOJI.iter().position(|(g, _)| *g == "☺\u{FE0F}").unwrap();
        assert_eq!(
            pieces("gg 😂 nice🙂‍↔️!☺"),
            vec![
                Piece::Text("gg "),
                Piece::Emoji(joy),
                Piece::Text(" nice"),
                Piece::Emoji(shake),
                Piece::Text("!"),
                Piece::Emoji(warm),
            ]
        );
        assert_eq!(
            pieces("🙂🙂"),
            vec![Piece::Emoji(smile), Piece::Emoji(smile)]
        );
        assert_eq!(pieces("plain"), vec![Piece::Text("plain")]);
        assert_eq!(pieces("🦀❤️"), vec![Piece::Text("🦀❤️")]);
    }

    #[test]
    fn the_field_keeps_the_text_and_leaves_room_for_each_picture() {
        let ctx = egui::Context::default();
        let glyph = EMOJI[0].0;
        let text = format!("hi {glyph}{glyph} there");
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let galley = field_galley(ui, &text, 300.0);
                assert_eq!(galley.text(), text);
                let x = |chars: usize| galley.pos_from_cursor(CCursor::new(chars)).min.x;
                let len = glyph.chars().count();
                for first in [3, 3 + len] {
                    assert!((x(first + len) - x(first) - field_side(ui)).abs() < 1.5);
                }
            });
        });
    }

    #[test]
    fn an_empty_field_is_one_row_tall_so_the_cursor_sits_on_the_text_line() {
        let ctx = egui::Context::default();
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let empty = field_galley(ui, "", 300.0);
                let text = field_galley(ui, "Message", 300.0);
                assert!(empty.size().y > 0.0);
                assert_eq!(empty.size().y, text.size().y);
            });
        });
    }

    #[test]
    fn a_picture_that_wraps_in_the_field_is_drawn_where_its_room_is() {
        let ctx = egui::Context::default();
        let glyph = EMOJI[0].0;
        let text = format!("See you at 3 {glyph}");
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let side = field_side(ui);
                let words = ui.fonts_mut(|f| {
                    f.layout_no_wrap(
                        "See you at 3 ".into(),
                        TextStyle::Body.resolve(ui.style()),
                        Color32::WHITE,
                    )
                    .size()
                    .x
                });
                let galley = field_galley(ui, &text, words + side / 2.0);
                assert_eq!(galley.rows.len(), 2);
                let rects = picture_rects(&galley, &text, side);
                let second = galley.rows[1].rect();
                assert!(rects[0].0.center().y > second.top(), "{rects:?} {second:?}");
                assert!(
                    rects[0].0.left() < second.left() + 1.0,
                    "{rects:?} {second:?}"
                );
            });
        });
    }

    #[test]
    fn a_message_of_only_emoji_is_counted() {
        assert_eq!(only_emoji(&pieces("😂 😂")), Some(2));
        assert_eq!(only_emoji(&pieces("😂 hi")), None);
        assert_eq!(only_emoji(&pieces("  ")), None);
    }
}
