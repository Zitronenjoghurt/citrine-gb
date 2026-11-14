pub struct Sharp {
    pub frame: Vec<u8>,
    pub width: usize,
    pub screen: Vec<u8>,
    pub screen_y: usize,
    pub scale: usize,
}

impl Sharp {
    pub fn bytes(&self) -> usize {
        self.frame.len() + self.screen.len()
    }

    pub fn draw(&self, scale: usize, out: &mut Vec<u8>) {
        let width = self.width;
        let height = self.frame.len() / 4 / width;
        let screen_height = self.screen.len() / 4 / (width * self.scale * self.scale);
        out.resize(width * scale * height * scale * 4, 0);
        let rows = self.screen_y * scale..(self.screen_y + screen_height) * scale;
        for (y, line) in out.chunks_exact_mut(width * scale * 4).enumerate() {
            if rows.contains(&y) {
                resample(&self.screen, width, self.scale, scale, y - rows.start, line);
            } else {
                resample(&self.frame, width, 1, scale, y, line);
            }
        }
    }
}

fn resample(source: &[u8], width: usize, from: usize, to: usize, y: usize, line: &mut [u8]) {
    let stride = width * from * 4;
    if from == to {
        line.copy_from_slice(&source[y * stride..(y + 1) * stride]);
        return;
    }
    let pixels = line.as_chunks_mut::<4>().0.iter_mut().enumerate();
    if from < to {
        let row = &source[(2 * y + 1) * from / (2 * to) * stride..][..stride];
        for (x, pixel) in pixels {
            let at = (2 * x + 1) * from / (2 * to) * 4;
            pixel.copy_from_slice(&row[at..at + 4]);
        }
        return;
    }
    let rows = y * from / to..(y + 1) * from / to;
    for (x, pixel) in pixels {
        let columns = x * from / to..(x + 1) * from / to;
        let mut sum = [0u32; 4];
        for row in rows.clone() {
            let line = &source[row * stride..(row + 1) * stride];
            for column in columns.clone() {
                for (total, &byte) in sum.iter_mut().zip(&line[column * 4..column * 4 + 4]) {
                    *total += u32::from(byte);
                }
            }
        }
        let count = (rows.len() * columns.len()) as u32;
        for (out, total) in pixel.iter_mut().zip(sum) {
            *out = ((total + count / 2) / count) as u8;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WIDTH: usize = 4;
    const SCREEN: usize = 2;

    fn picture(width: usize, height: usize, pixel: impl Fn(usize, usize) -> [u8; 4]) -> Vec<u8> {
        let pixel = &pixel;
        (0..height)
            .flat_map(|y| (0..width).flat_map(move |x| pixel(x, y)))
            .collect()
    }

    fn sharp(scale: usize) -> Sharp {
        Sharp {
            frame: picture(WIDTH, SCREEN * 2, |x, y| [x as u8, y as u8, 1, 255]),
            width: WIDTH,
            screen: picture(WIDTH * scale, SCREEN * scale, |x, y| {
                [100 + x as u8, 100 + y as u8, 2, 255]
            }),
            screen_y: SCREEN,
            scale,
        }
    }

    fn at(out: &[u8], scale: usize, x: usize, y: usize) -> [u8; 4] {
        let p = (y * WIDTH * scale + x) * 4;
        out[p..p + 4].try_into().unwrap()
    }

    #[test]
    fn the_sharp_screen_keeps_every_pixel_at_its_own_scale() {
        let mut out = Vec::new();
        sharp(4).draw(4, &mut out);
        assert_eq!(out.len(), WIDTH * 4 * SCREEN * 2 * 4 * 4);
        assert_eq!(at(&out, 4, 13, 8 + 5), [113, 105, 2, 255]);
        assert_eq!(at(&out, 4, 13, 5), [3, 1, 1, 255]);
    }

    #[test]
    fn a_smaller_video_averages_the_sharp_screen() {
        let mut out = Vec::new();
        sharp(4).draw(2, &mut out);
        assert_eq!(at(&out, 2, 3, 4 + 1), [107, 103, 2, 255]);
        assert_eq!(at(&out, 2, 3, 1), [1, 0, 1, 255]);
    }

    #[test]
    fn a_bigger_video_repeats_the_sharp_screen() {
        let mut out = Vec::new();
        sharp(2).draw(4, &mut out);
        assert_eq!(at(&out, 4, 13, 8 + 5), [106, 102, 2, 255]);
        assert_eq!(at(&out, 4, 0, 15), [100, 103, 2, 255]);
    }
}
