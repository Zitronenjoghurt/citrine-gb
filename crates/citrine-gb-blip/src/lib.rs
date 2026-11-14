pub const MAX_RATIO: u64 = 1 << 20;
pub const MAX_FRAME: u32 = 4000;

const PRE_SHIFT: usize = 32;
const TIME_BITS: usize = PRE_SHIFT + 20;
const TIME_UNIT: u64 = 1 << TIME_BITS;
const BASS_SHIFT: usize = 9;
const END_FRAME_EXTRA: usize = 2;

const HALF_WIDTH: usize = 8;
const WIDTH: usize = HALF_WIDTH * 2;
const BUF_EXTRA: usize = WIDTH + END_FRAME_EXTRA;
const PHASE_BITS: usize = 5;
const PHASE_COUNT: usize = 1 << PHASE_BITS;
const DELTA_BITS: usize = 15;
const DELTA_UNIT: usize = 1 << DELTA_BITS;
const FRAC_BITS: usize = TIME_BITS - PRE_SHIFT;
const PHASE_SHIFT: usize = FRAC_BITS - PHASE_BITS;

pub struct BlipBuf<const N: usize> {
    factor: u64,
    offset: u64,
    integrator: [i32; N],
    avail: usize,
    dirty: usize,
    samples: Vec<[i32; N]>,
}

impl<const N: usize> BlipBuf<N> {
    pub fn new(sample_count: u32) -> Self {
        const FACTOR: u64 = TIME_UNIT / MAX_RATIO;
        Self {
            factor: FACTOR,
            offset: FACTOR / 2,
            integrator: [0; N],
            avail: 0,
            dirty: 0,
            samples: vec![[0; N]; sample_count as usize + BUF_EXTRA],
        }
    }

    pub fn set_rates(&mut self, clock_rate: f64, sample_rate: f64) {
        let factor = (TIME_UNIT as f64) * sample_rate / clock_rate;
        self.factor = factor as u64;
        assert!(0.0 <= factor - self.factor as f64 && factor - (self.factor as f64) < 1.0);
        self.factor = factor.ceil() as u64;
    }

    pub fn clear(&mut self) {
        self.offset = self.factor / 2;
        self.avail = 0;
        self.integrator = [0; N];
        self.samples.fill([0; N]);
        self.dirty = 0;
    }

    #[inline]
    pub fn add_delta(&mut self, clock_time: u32, delta: [i32; N]) {
        let fixed = ((u64::from(clock_time) * self.factor + self.offset) >> PRE_SHIFT) as usize;
        let out_index = self.avail + (fixed >> FRAC_BITS);
        let phase = fixed >> PHASE_SHIFT & (PHASE_COUNT - 1);
        let interp = (fixed >> (PHASE_SHIFT - DELTA_BITS) & (DELTA_UNIT - 1)) as i32;
        let delta2 = delta.map(|d| (d * interp) >> DELTA_BITS);
        let delta1 = std::array::from_fn::<i32, N, _>(|c| delta[c] - delta2[c]);

        assert!(
            out_index <= self.samples.len() + END_FRAME_EXTRA,
            "buffer size was exceeded"
        );

        let out = &mut self.samples[out_index..out_index + WIDTH];
        if N == 2 && delta.iter().all(|&d| i16::try_from(d).is_ok()) {
            let deltas = [delta1[0], delta2[0], delta1[1], delta2[1]].map(|d| d as i16);
            #[cfg(target_arch = "x86_64")]
            unsafe {
                stereo::add_kernel(out.as_flattened_mut(), &STEREO_KERNEL[phase], deltas);
            }
            #[cfg(not(target_arch = "x86_64"))]
            stereo::add_kernel(out.as_flattened_mut(), &STEREO_KERNEL[phase], deltas);
        } else {
            for ((out, near), next) in out.iter_mut().zip(&KERNEL[phase]).zip(&KERNEL[phase + 1]) {
                for c in 0..N {
                    out[c] += i32::from(*near) * delta1[c] + i32::from(*next) * delta2[c];
                }
            }
        }
        self.dirty = self.dirty.max(out_index + WIDTH);
    }

    pub fn clocks_needed(&self, sample_count: u32) -> u32 {
        assert!(self.avail + sample_count as usize <= self.samples.len());

        let needed = u64::from(sample_count) * TIME_UNIT;
        if needed < self.offset {
            return 0;
        }

        (needed - self.offset).div_ceil(self.factor) as u32
    }

    pub fn end_frame(&mut self, clock_duration: u32) {
        let off = u64::from(clock_duration) * self.factor + self.offset;
        self.avail += (off >> TIME_BITS) as usize;
        self.offset = off & (TIME_UNIT - 1);
        assert!(self.avail <= self.samples.len(), "buffer size was exceeded");
    }

    pub fn samples_avail(&self) -> u32 {
        self.avail as u32
    }

    fn remove_samples(&mut self, count: usize) {
        let remain = (self.avail + BUF_EXTRA).saturating_sub(count);
        self.avail = self.avail.saturating_sub(count);
        self.samples.copy_within(count..count + remain, 0);
        let dirty = self.dirty.max(remain);
        self.samples[remain..dirty].fill([0; N]);
        self.dirty = remain;
    }

    pub fn read_samples(&mut self, buf: &mut [[i16; N]]) -> usize {
        let max = buf.len();
        let mut slots = buf.iter_mut();
        self.read_samples_with(max, |sample| {
            if let Some(slot) = slots.next() {
                *slot = sample;
            }
        })
    }

    pub fn read_samples_with(&mut self, max: usize, mut each: impl FnMut([i16; N])) -> usize {
        let count = max.min(self.avail);

        if count > 0 {
            if let Ok(pair) = <&mut [i32; 2]>::try_from(&mut self.integrator[..]) {
                let samples = self.samples[..count].as_flattened().as_chunks::<2>().0;
                let each = |out: [i16; 2]| each(std::array::from_fn(|c| out[c]));
                #[cfg(target_arch = "x86_64")]
                unsafe {
                    stereo::integrate(pair, samples, each);
                }
                #[cfg(not(target_arch = "x86_64"))]
                stereo::integrate(pair, samples, each);
            } else {
                let mut sum = self.integrator;

                for sample in &self.samples[..count] {
                    let mut out = [0; N];
                    for c in 0..N {
                        let s = (sum[c] >> DELTA_BITS).clamp(i16::MIN.into(), i16::MAX.into());
                        out[c] = s as i16;
                        sum[c] += sample[c];
                        sum[c] -= s << (DELTA_BITS - BASS_SHIFT);
                    }
                    each(out);
                }

                self.integrator = sum;
            }
            self.remove_samples(count);
        }

        count
    }
}

impl BlipBuf<2> {
    pub fn read_high_passed(
        &mut self,
        out: &mut [[f32; 2]],
        scale: f32,
        charge: f32,
        capacitor: &mut [f32; 2],
    ) -> usize {
        let count = out.len().min(self.avail);

        if count > 0 {
            let samples = &self.samples[..count];
            let out = &mut out[..count];
            #[cfg(target_arch = "x86_64")]
            unsafe {
                stereo::integrate_high_passed(
                    &mut self.integrator,
                    samples,
                    out,
                    scale,
                    charge,
                    capacitor,
                );
            }
            #[cfg(not(target_arch = "x86_64"))]
            stereo::integrate_high_passed(
                &mut self.integrator,
                samples,
                out,
                scale,
                charge,
                capacitor,
            );
            self.remove_samples(count);
        }

        count
    }
}

const KERNEL: [[i16; WIDTH]; PHASE_COUNT + 1] = kernel();

const fn kernel() -> [[i16; WIDTH]; PHASE_COUNT + 1] {
    let mut table = [[0; WIDTH]; PHASE_COUNT + 1];
    let mut phase = 0;
    while phase <= PHASE_COUNT {
        let mut i = 0;
        while i < HALF_WIDTH {
            table[phase][i] = BL_STEP[phase][i];
            table[phase][HALF_WIDTH + i] = BL_STEP[PHASE_COUNT - phase][HALF_WIDTH - 1 - i];
            i += 1;
        }
        phase += 1;
    }
    table
}

const STEREO_KERNEL: [[i16; WIDTH * 4]; PHASE_COUNT] = stereo_kernel();

const fn stereo_kernel() -> [[i16; WIDTH * 4]; PHASE_COUNT] {
    let mut table = [[0; WIDTH * 4]; PHASE_COUNT];
    let mut phase = 0;
    while phase < PHASE_COUNT {
        let mut i = 0;
        while i < WIDTH {
            let pair = [KERNEL[phase][i], KERNEL[phase + 1][i]];
            table[phase][i * 4] = pair[0];
            table[phase][i * 4 + 1] = pair[1];
            table[phase][i * 4 + 2] = pair[0];
            table[phase][i * 4 + 3] = pair[1];
            i += 1;
        }
        phase += 1;
    }
    table
}

#[cfg(target_arch = "x86_64")]
mod stereo {
    use core::arch::x86_64::{
        __m128, _mm_add_epi32, _mm_cvtepi32_ps, _mm_cvtsi128_si32, _mm_cvtss_f32, _mm_div_ps,
        _mm_madd_epi16, _mm_mul_ps, _mm_packs_epi32, _mm_set1_ps, _mm_setr_epi16, _mm_setr_epi32,
        _mm_setr_ps, _mm_setzero_si128, _mm_shuffle_epi32, _mm_shuffle_ps, _mm_slli_epi32,
        _mm_srai_epi32, _mm_sub_epi32, _mm_sub_ps, _mm_unpacklo_epi16,
    };

    #[target_feature(enable = "sse2")]
    pub fn integrate(sum: &mut [i32; 2], samples: &[[i32; 2]], mut each: impl FnMut([i16; 2])) {
        let mut acc = _mm_setr_epi32(sum[0], sum[1], 0, 0);
        for sample in samples {
            let clamped = _mm_packs_epi32(
                _mm_srai_epi32::<{ super::DELTA_BITS as i32 }>(acc),
                _mm_setzero_si128(),
            );
            let out = _mm_cvtsi128_si32(clamped);
            each([out as i16, (out >> 16) as i16]);
            let wide = _mm_srai_epi32::<16>(_mm_unpacklo_epi16(clamped, clamped));
            acc = _mm_sub_epi32(
                _mm_add_epi32(acc, _mm_setr_epi32(sample[0], sample[1], 0, 0)),
                _mm_slli_epi32::<{ (super::DELTA_BITS - super::BASS_SHIFT) as i32 }>(wide),
            );
        }
        *sum = [
            _mm_cvtsi128_si32(acc),
            _mm_cvtsi128_si32(_mm_shuffle_epi32::<0b01_01_01_01>(acc)),
        ];
    }

    #[target_feature(enable = "sse2")]
    pub fn integrate_high_passed(
        sum: &mut [i32; 2],
        samples: &[[i32; 2]],
        out: &mut [[f32; 2]],
        scale: f32,
        charge: f32,
        capacitor: &mut [f32; 2],
    ) {
        let mut acc = _mm_setr_epi32(sum[0], sum[1], 0, 0);
        let scale = _mm_set1_ps(scale);
        let charge = _mm_set1_ps(charge);
        let mut held = _mm_setr_ps(capacitor[0], capacitor[1], 0.0, 0.0);
        for (sample, slot) in samples.iter().zip(out) {
            let clamped = _mm_packs_epi32(
                _mm_srai_epi32::<{ super::DELTA_BITS as i32 }>(acc),
                _mm_setzero_si128(),
            );
            let wide = _mm_srai_epi32::<16>(_mm_unpacklo_epi16(clamped, clamped));
            let raw = _mm_div_ps(_mm_cvtepi32_ps(wide), scale);
            let filtered = _mm_sub_ps(raw, held);
            held = _mm_sub_ps(raw, _mm_mul_ps(filtered, charge));
            *slot = low_pair(filtered);
            acc = _mm_sub_epi32(
                _mm_add_epi32(acc, _mm_setr_epi32(sample[0], sample[1], 0, 0)),
                _mm_slli_epi32::<{ (super::DELTA_BITS - super::BASS_SHIFT) as i32 }>(wide),
            );
        }
        *sum = [
            _mm_cvtsi128_si32(acc),
            _mm_cvtsi128_si32(_mm_shuffle_epi32::<0b01_01_01_01>(acc)),
        ];
        *capacitor = low_pair(held);
    }

    #[target_feature(enable = "sse2")]
    fn low_pair(value: __m128) -> [f32; 2] {
        [
            _mm_cvtss_f32(value),
            _mm_cvtss_f32(_mm_shuffle_ps::<0b01_01_01_01>(value, value)),
        ]
    }

    #[target_feature(enable = "sse2")]
    pub fn add_kernel(out: &mut [i32], kernel: &[i16; super::WIDTH * 4], deltas: [i16; 4]) {
        let [a, b, c, d] = deltas;
        let deltas = _mm_setr_epi16(a, b, c, d, a, b, c, d);
        for (out, k) in out
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(kernel.as_chunks::<8>().0)
        {
            let kernel = _mm_setr_epi16(k[0], k[1], k[2], k[3], k[4], k[5], k[6], k[7]);
            let sum = _mm_add_epi32(
                _mm_setr_epi32(out[0], out[1], out[2], out[3]),
                _mm_madd_epi16(kernel, deltas),
            );
            out[0] = _mm_cvtsi128_si32(sum);
            out[1] = _mm_cvtsi128_si32(_mm_shuffle_epi32::<0b01_01_01_01>(sum));
            out[2] = _mm_cvtsi128_si32(_mm_shuffle_epi32::<0b10_10_10_10>(sum));
            out[3] = _mm_cvtsi128_si32(_mm_shuffle_epi32::<0b11_11_11_11>(sum));
        }
    }
}

#[cfg(not(target_arch = "x86_64"))]
mod stereo {
    #[inline(always)]
    pub fn integrate(sum: &mut [i32; 2], samples: &[[i32; 2]], mut each: impl FnMut([i16; 2])) {
        for sample in samples {
            let mut out = [0; 2];
            for c in 0..2 {
                let s = (sum[c] >> super::DELTA_BITS).clamp(i16::MIN.into(), i16::MAX.into());
                out[c] = s as i16;
                sum[c] += sample[c];
                sum[c] -= s << (super::DELTA_BITS - super::BASS_SHIFT);
            }
            each(out);
        }
    }

    #[inline(always)]
    pub fn integrate_high_passed(
        sum: &mut [i32; 2],
        samples: &[[i32; 2]],
        out: &mut [[f32; 2]],
        scale: f32,
        charge: f32,
        capacitor: &mut [f32; 2],
    ) {
        for (sample, slot) in samples.iter().zip(out) {
            for c in 0..2 {
                let s = (sum[c] >> super::DELTA_BITS).clamp(i16::MIN.into(), i16::MAX.into());
                sum[c] += sample[c];
                sum[c] -= s << (super::DELTA_BITS - super::BASS_SHIFT);
                let raw = s as f32 / scale;
                let filtered = raw - capacitor[c];
                capacitor[c] = raw - filtered * charge;
                slot[c] = filtered;
            }
        }
    }

    #[inline(always)]
    pub fn add_kernel(out: &mut [i32], kernel: &[i16; super::WIDTH * 4], deltas: [i16; 4]) {
        for (out, k) in out
            .as_chunks_mut::<2>()
            .0
            .iter_mut()
            .zip(kernel.as_chunks::<4>().0)
        {
            out[0] +=
                i32::from(k[0]) * i32::from(deltas[0]) + i32::from(k[1]) * i32::from(deltas[1]);
            out[1] +=
                i32::from(k[2]) * i32::from(deltas[2]) + i32::from(k[3]) * i32::from(deltas[3]);
        }
    }
}

const BL_STEP: [[i16; HALF_WIDTH]; PHASE_COUNT + 1] = [
    [43, -115, 350, -488, 1136, -914, 5861, 21022],
    [44, -118, 348, -473, 1076, -799, 5274, 21001],
    [45, -121, 344, -454, 1011, -677, 4706, 20936],
    [46, -122, 336, -431, 942, -549, 4156, 20829],
    [47, -123, 327, -404, 868, -418, 3629, 20679],
    [47, -122, 316, -375, 792, -285, 3124, 20488],
    [47, -120, 303, -344, 714, -151, 2644, 20256],
    [46, -117, 289, -310, 634, -17, 2188, 19985],
    [46, -114, 273, -275, 553, 117, 1758, 19675],
    [44, -108, 255, -237, 471, 247, 1356, 19327],
    [43, -103, 237, -199, 390, 373, 981, 18944],
    [42, -98, 218, -160, 310, 495, 633, 18527],
    [40, -91, 198, -121, 231, 611, 314, 18078],
    [38, -84, 178, -81, 153, 722, 22, 17599],
    [36, -76, 157, -43, 80, 824, -241, 17092],
    [34, -68, 135, -3, 8, 919, -476, 16558],
    [32, -61, 115, 34, -60, 1006, -683, 16001],
    [29, -52, 94, 70, -123, 1083, -862, 15422],
    [27, -44, 73, 106, -184, 1152, -1015, 14824],
    [25, -36, 53, 139, -239, 1211, -1142, 14210],
    [22, -27, 34, 170, -290, 1261, -1244, 13582],
    [20, -20, 16, 199, -335, 1301, -1322, 12942],
    [18, -12, -3, 226, -375, 1331, -1376, 12293],
    [15, -4, -19, 250, -410, 1351, -1408, 11638],
    [13, 3, -35, 272, -439, 1361, -1419, 10979],
    [11, 9, -49, 292, -464, 1362, -1410, 10319],
    [9, 16, -63, 309, -483, 1354, -1383, 9660],
    [7, 22, -75, 322, -496, 1337, -1339, 9005],
    [6, 26, -85, 333, -504, 1312, -1280, 8355],
    [4, 31, -94, 341, -507, 1278, -1205, 7713],
    [3, 35, -102, 347, -506, 1238, -1119, 7082],
    [1, 40, -110, 350, -499, 1190, -1021, 6464],
    [0, 43, -115, 350, -488, 1136, -914, 5861],
];

#[cfg(test)]
mod tests {
    use super::BlipBuf;

    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }

        fn below(&mut self, n: u64) -> u64 {
            self.next() % n
        }
    }

    fn compare<const N: usize>(seed: u64, clock_rate: f64, sample_rate: f64, size: u32) {
        let mut rng = Rng(seed);
        let mut ours = BlipBuf::<N>::new(size);
        let mut theirs: [_; N] = std::array::from_fn(|_| blip_buf::BlipBuf::new(size));
        ours.set_rates(clock_rate, sample_rate);
        for buf in &mut theirs {
            buf.set_rates(clock_rate, sample_rate);
        }
        let per_sample = (clock_rate / sample_rate).ceil() as u64;
        let mut level = [0; N];

        for frame in 0..400 {
            let samples = 1 + rng.below(u64::from(size) / 4) as u32;
            let duration = ours.clocks_needed(samples);
            for buf in &theirs {
                assert_eq!(duration, buf.clocks_needed(samples));
            }
            let deltas = rng.below(200);
            let mut time = 0;
            for _ in 0..deltas {
                time += rng.below(u64::from(duration) / (deltas + 1) * 2 + 1) as u32;
                if u64::from(time) > u64::from(duration) + per_sample {
                    break;
                }
                let mut delta = [0; N];
                for c in 0..N {
                    if rng.below(4) == 0 {
                        continue;
                    }
                    let next = rng.below(60_001) as i32 - 30_000;
                    delta[c] = next - level[c];
                    level[c] = next;
                    if delta[c] != 0 {
                        theirs[c].add_delta(time, delta[c]);
                    }
                }
                ours.add_delta(time, delta);
            }
            ours.end_frame(duration);
            for buf in &mut theirs {
                buf.end_frame(duration);
                assert_eq!(ours.samples_avail(), buf.samples_avail());
            }

            let avail = ours.samples_avail() as usize;
            let len = if avail > size as usize / 2 {
                avail
            } else {
                match rng.below(3) {
                    0 => avail,
                    1 => 1 + rng.below(avail as u64 + 1) as usize,
                    _ => 2 * avail,
                }
            };
            let mut out_ours = vec![[0; N]; len];
            let count = ours.read_samples(&mut out_ours);
            assert_eq!(count, len.min(avail));
            for (c, buf) in theirs.iter_mut().enumerate() {
                let mut out_theirs = vec![0; len];
                assert_eq!(buf.read_samples(&mut out_theirs, false), count);
                let channel: Vec<i16> = out_ours.iter().map(|frame| frame[c]).collect();
                assert_eq!(channel, out_theirs, "seed {seed} frame {frame} channel {c}");
                assert_eq!(ours.samples_avail(), buf.samples_avail());
            }

            if rng.below(50) == 0 {
                ours.clear();
                for buf in &mut theirs {
                    buf.clear();
                }
                level = [0; N];
            }
        }
    }

    #[test]
    fn high_passed_read_matches_scalar_filter() {
        for seed in 1..=10 {
            let mut rng = Rng(seed);
            let mut plain = BlipBuf::<2>::new(4096);
            let mut filtered = BlipBuf::<2>::new(4096);
            plain.set_rates(4_194_304.0, 48_000.0);
            filtered.set_rates(4_194_304.0, 48_000.0);
            let charge = 0.996 + rng.below(4000) as f32 / 1_000_000.0;
            let mut level = [0; 2];
            let mut held = [0.0_f32; 2];
            let mut capacitor = [0.0_f32; 2];
            for frame in 0..200 {
                let duration = plain.clocks_needed(1 + rng.below(800) as u32);
                let deltas = rng.below(100);
                let mut time = 0;
                for _ in 0..deltas {
                    time += rng.below(u64::from(duration) / (deltas + 1) * 2 + 1) as u32;
                    if time >= duration {
                        break;
                    }
                    let mut delta = [0; 2];
                    for c in 0..2 {
                        let next = rng.below(60_001) as i32 - 30_000;
                        delta[c] = next - level[c];
                        level[c] = next;
                    }
                    plain.add_delta(time, delta);
                    filtered.add_delta(time, delta);
                }
                plain.end_frame(duration);
                filtered.end_frame(duration);

                let avail = plain.samples_avail() as usize;
                let len = if avail > 2048 {
                    avail
                } else {
                    1 + rng.below(avail as u64 * 2 + 1) as usize
                };
                let mut raw = vec![[0; 2]; len];
                let count = plain.read_samples(&mut raw);
                let expected: Vec<[u32; 2]> = raw[..count]
                    .iter()
                    .map(|sample| {
                        let mut bits = [0; 2];
                        for c in 0..2 {
                            let input = f32::from(sample[c]) / 16_000.0;
                            let output = input - held[c];
                            held[c] = input - output * charge;
                            bits[c] = output.to_bits();
                        }
                        bits
                    })
                    .collect();

                let mut out = vec![[0.0; 2]; len];
                let read = filtered.read_high_passed(&mut out, 16_000.0, charge, &mut capacitor);
                assert_eq!(read, count);
                let got: Vec<[u32; 2]> = out[..read]
                    .iter()
                    .map(|sample| sample.map(f32::to_bits))
                    .collect();
                assert_eq!(got, expected, "seed {seed} frame {frame}");
                assert_eq!(capacitor.map(f32::to_bits), held.map(f32::to_bits));
                assert_eq!(filtered.samples_avail(), plain.samples_avail());
            }
        }
    }

    #[test]
    fn output_matches_blip_buf() {
        for seed in 1..=20 {
            compare::<1>(seed, 4_194_304.0, 44_100.0, 8192);
            compare::<2>(seed, 4_194_304.0, 44_100.0, 8192);
            compare::<2>(seed, 8_388_608.0, 48_000.0, 8192);
            compare::<2>(seed, 4_194_304.0, 22_050.0, 2048);
            compare::<1>(seed, 2_097_152.0, 96_000.0, 4096);
            compare::<2>(seed, 2_097_152.0, 96_000.0, 4096);
        }
    }
}
