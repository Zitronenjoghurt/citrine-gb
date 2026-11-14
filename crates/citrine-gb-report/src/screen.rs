use crate::archive::ReportError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Screen {
    pub seconds_before: u32,
    pub png: Vec<u8>,
}

impl Screen {
    pub fn from_rgba(
        rgba: &[u8],
        width: usize,
        height: usize,
        seconds_before: u32,
    ) -> Result<Self, ReportError> {
        Ok(Self {
            seconds_before,
            png: encode_png(rgba, width, height)?,
        })
    }
}

pub fn encode_png(rgba: &[u8], width: usize, height: usize) -> Result<Vec<u8>, ReportError> {
    let mut png = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut png, width as u32, height as u32);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header()?;
        writer.write_image_data(rgba)?;
    }
    Ok(png)
}
