pub fn texture(ctx: &egui::Context) -> Option<egui::TextureHandle> {
    let id = egui::Id::new("app_icon");
    if let Some(handle) = ctx.data(|d| d.get_temp::<egui::TextureHandle>(id)) {
        return Some(handle);
    }
    let frame = crate::media::decode_png(include_bytes!("../../../assets/icon-128x128.png"))?;
    let image = egui::ColorImage::from_rgba_unmultiplied(
        [frame.width as usize, frame.height as usize],
        &frame.rgba,
    );
    let handle = ctx.load_texture("app_icon", image, egui::TextureOptions::LINEAR);
    ctx.data_mut(|d| d.insert_temp(id, handle.clone()));
    Some(handle)
}
