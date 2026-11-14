use crate::media::Frame;

const MOST_SIDE: u32 = 320;

pub fn shrink(png: &[u8]) -> Option<Vec<u8>> {
    let frame = crate::media::decode_png(png)?;
    let small = fit(&frame, MOST_SIDE);
    crate::utils::screenshot::encode_png(small.width as usize, small.height as usize, &small.rgba)
}

pub fn decode(png: &[u8]) -> Option<Frame> {
    crate::media::decode_png(png)
}

fn fit(frame: &Frame, most: u32) -> Frame {
    let long = frame.width.max(frame.height);
    if long <= most {
        return Frame {
            width: frame.width,
            height: frame.height,
            rgba: frame.rgba.clone(),
        };
    }
    let width = (frame.width as u64 * most as u64 / long as u64).max(1) as u32;
    let height = (frame.height as u64 * most as u64 / long as u64).max(1) as u32;
    let span = |out: u32, of: u32, from: u32| {
        let start = out as u64 * from as u64 / of as u64;
        let end = ((out as u64 + 1) * from as u64).div_ceil(of as u64);
        start as u32..(end as u32).min(from)
    };
    let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
    for y in 0..height {
        let rows = span(y, height, frame.height);
        for x in 0..width {
            let columns = span(x, width, frame.width);
            let mut sum = [0u32; 4];
            let mut count = 0;
            for sy in rows.clone() {
                for sx in columns.clone() {
                    let at = ((sy * frame.width + sx) * 4) as usize;
                    for (channel, total) in sum.iter_mut().enumerate() {
                        *total += frame.rgba[at + channel] as u32;
                    }
                    count += 1;
                }
            }
            rgba.extend(sum.iter().map(|total| (total / count.max(1)) as u8));
        }
    }
    Frame {
        width,
        height,
        rgba,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(width: u32, height: u32) -> Vec<u8> {
        let rgba: Vec<u8> = (0..width * height)
            .flat_map(|i| [(i % 251) as u8, 40, 200, 255])
            .collect();
        crate::utils::screenshot::encode_png(width as usize, height as usize, &rgba).unwrap()
    }

    #[test]
    fn a_big_box_shrinks_to_fit_and_keeps_its_shape() {
        let small = decode(&shrink(&png(512, 460)).unwrap()).unwrap();
        assert_eq!((small.width, small.height), (320, 287));
        let tall = decode(&shrink(&png(300, 640)).unwrap()).unwrap();
        assert_eq!((tall.width, tall.height), (150, 320));
    }

    #[test]
    fn a_small_box_stays_as_it_is() {
        let small = decode(&shrink(&png(200, 180)).unwrap()).unwrap();
        assert_eq!((small.width, small.height), (200, 180));
        assert_eq!(small.rgba[4..8], [1, 40, 200, 255]);
    }

    #[test]
    fn a_page_that_is_no_picture_is_refused() {
        assert_eq!(shrink(b"<html>404</html>"), None);
    }
}
