use openh264::OpenH264API;
use openh264::encoder::{
    BitRate, Complexity, Encoder, EncoderConfig, FrameRate, IntraFramePeriod, Profile,
    RateControlMode, UsageType, VuiConfig,
};
use openh264::formats::YUVSlices;

const VIDEO_BITRATE: u32 = 2_500_000;

pub struct EncodedFrame {
    pub sample: Vec<u8>,
    pub keyframe: bool,
    pub sps: Option<Vec<u8>>,
    pub pps: Option<Vec<u8>>,
}

pub struct H264Encoder {
    encoder: Encoder,
    width: usize,
    height: usize,
    scale: usize,
    yuv: Vec<u8>,
}

impl H264Encoder {
    pub fn new(
        api: OpenH264API,
        width: usize,
        height: usize,
        scale: usize,
        fps: f32,
    ) -> anyhow::Result<Self> {
        let scale = scale.max(2) & !1;
        let config = EncoderConfig::new()
            .usage_type(UsageType::ScreenContentRealTime)
            .rate_control_mode(RateControlMode::Bitrate)
            .bitrate(BitRate::from_bps(super::video_bitrate(
                VIDEO_BITRATE,
                scale,
            )))
            .max_frame_rate(FrameRate::from_hz(fps))
            .skip_frames(false)
            .profile(Profile::Baseline)
            .complexity(Complexity::Low)
            .adaptive_quantization(false)
            .background_detection(false)
            .intra_frame_period(IntraFramePeriod::from_num_frames(fps.round() as u32 * 2))
            .vui(VuiConfig::bt709());
        let encoder = Encoder::with_api_config(api, config)?;
        let out_w = width * scale;
        let out_h = height * scale;
        Ok(Self {
            encoder,
            width,
            height,
            scale,
            yuv: vec![0; out_w * out_h * 3 / 2],
        })
    }

    pub fn encode(&mut self, rgba: &[u8]) -> anyhow::Result<EncodedFrame> {
        let (w, h) = (self.width * self.scale, self.height * self.scale);
        upscale_to_i420(rgba, self.width, self.height, self.scale, &mut self.yuv);
        let (y, chroma) = self.yuv.split_at(w * h);
        let (u, v) = chroma.split_at(w * h / 4);
        let source = YUVSlices::new((y, u, v), (w, h), (w, w / 2, w / 2));
        let stream = self.encoder.encode(&source)?;
        let mut frame = EncodedFrame {
            sample: Vec::new(),
            keyframe: false,
            sps: None,
            pps: None,
        };
        for layer in (0..stream.num_layers()).filter_map(|i| stream.layer(i)) {
            for nal in (0..layer.nal_count()).filter_map(|i| layer.nal_unit(i)) {
                for unit in split_annex_b(nal) {
                    push_nal(&mut frame, unit);
                }
            }
        }
        Ok(frame)
    }
}

fn push_nal(frame: &mut EncodedFrame, unit: &[u8]) {
    let Some(&header) = unit.first() else {
        return;
    };
    match header & 0x1F {
        7 => frame.sps = Some(unit.to_vec()),
        8 => frame.pps = Some(unit.to_vec()),
        9 => {}
        kind => {
            if kind == 5 {
                frame.keyframe = true;
            }
            frame
                .sample
                .extend_from_slice(&(unit.len() as u32).to_be_bytes());
            frame.sample.extend_from_slice(unit);
        }
    }
}

pub fn split_annex_b(data: &[u8]) -> Vec<&[u8]> {
    let mut starts = Vec::new();
    let mut i = 0;
    while i + 3 <= data.len() {
        if data[i] == 0 && data[i + 1] == 0 && data[i + 2] == 1 {
            starts.push((i, i + 3));
            i += 3;
        } else {
            i += 1;
        }
    }
    if starts.is_empty() {
        return if data.is_empty() { vec![] } else { vec![data] };
    }
    let mut units = Vec::new();
    for (n, &(_, begin)) in starts.iter().enumerate() {
        let mut end = starts.get(n + 1).map_or(data.len(), |&(at, _)| at);
        while end > begin && data[end - 1] == 0 {
            end -= 1;
        }
        if end > begin {
            units.push(&data[begin..end]);
        }
    }
    units
}

pub fn upscale_to_i420(rgba: &[u8], width: usize, height: usize, scale: usize, out: &mut [u8]) {
    let out_w = width * scale;
    let out_h = height * scale;
    let half = scale / 2;
    let chroma_w = out_w / 2;
    let (y_plane, chroma) = out.split_at_mut(out_w * out_h);
    let (u_plane, v_plane) = chroma.split_at_mut(chroma_w * (out_h / 2));
    for sy in 0..height {
        for sx in 0..width {
            let p = (sy * width + sx) * 4;
            let (r, g, b) = (rgba[p] as i32, rgba[p + 1] as i32, rgba[p + 2] as i32);
            let y = (16 + ((47 * r + 157 * g + 16 * b + 128) >> 8)) as u8;
            let u = (128 + ((-26 * r - 86 * g + 112 * b + 128) >> 8)) as u8;
            let v = (128 + ((112 * r - 102 * g - 10 * b + 128) >> 8)) as u8;
            for dy in 0..scale {
                let row = (sy * scale + dy) * out_w + sx * scale;
                y_plane[row..row + scale].fill(y);
            }
            for dy in 0..half {
                let row = (sy * half + dy) * chroma_w + sx * half;
                u_plane[row..row + half].fill(u);
                v_plane[row..row + half].fill(v);
            }
        }
    }
}
