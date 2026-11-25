use crate::media::{MediaId, MediaKind, MediaStore};
use jni::objects::JValue;
use std::sync::mpsc::Sender;

pub fn shares() -> bool {
    true
}

pub fn copies_images() -> bool {
    false
}

pub fn copy(
    _store: &MediaStore,
    _id: &MediaId,
    _kind: MediaKind,
    done: Sender<Result<String, String>>,
) {
    let _ = done.send(Err("copying files works only on a computer".into()));
}

fn mime(kind: MediaKind) -> &'static str {
    match kind {
        MediaKind::Screenshot | MediaKind::Print => "image/png",
        MediaKind::Recording => "video/mp4",
        MediaKind::Replay => "application/octet-stream",
    }
}

pub fn share(
    store: &MediaStore,
    id: &MediaId,
    kind: MediaKind,
    done: Sender<Result<String, String>>,
) {
    let path = store.path(id).to_string_lossy().into_owned();
    let _ = done.send(share_file(&path, mime(kind)).map(|()| String::new()));
}

pub fn share_file(path: &str, mime: &str) -> Result<(), String> {
    super::java(|env, activity| {
        let path = env.new_string(path)?;
        let mime = env.new_string(mime)?;
        env.call_method(
            activity,
            "shareFile",
            "(Ljava/lang/String;Ljava/lang/String;)V",
            &[JValue::Object(&path), JValue::Object(&mime)],
        )
        .map(drop)
    })
    .ok_or_else(|| "could not open the share sheet".into())
}

pub fn copy_text(text: &str) {
    super::java(|env, activity| {
        let text = env.new_string(text)?;
        env.call_method(
            activity,
            "copyText",
            "(Ljava/lang/String;)V",
            &[JValue::Object(&text)],
        )
        .map(drop)
    });
}

pub fn mirror_copies(ctx: &egui::Context) {
    let copies: Vec<String> = ctx.output(|o| {
        o.commands
            .iter()
            .filter_map(|command| match command {
                egui::OutputCommand::CopyText(text) => Some(text.clone()),
                _ => None,
            })
            .collect()
    });
    for text in copies {
        copy_text(&text);
    }
}
