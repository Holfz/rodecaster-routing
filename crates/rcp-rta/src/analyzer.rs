//! Windowed FFT reduced to log-spaced display bands.
//!
//! No smoothing or peak hold happens here: the UI draws on an animation frame
//! and applies both against elapsed time, so doing it at this end would tie
//! their rate to the audio buffer size.

use std::sync::Arc;

use realfft::num_complex::Complex;
use realfft::{RealFftPlanner, RealToComplex};

/// Points on the curve. Enough that neighbouring pixels differ on a wide
/// window, few enough that a frame stays small on the way to the webview.
pub const BANDS: usize = 256;

/// 8192 at 48 kHz is a 5.9 Hz bin and a 171 ms window. Finer bins would smear
/// speech in time; coarser ones cannot separate a low room mode from the
/// fundamental above it.
pub const FFT_SIZE: usize = 8192;

/// A quarter of the window, so the curve updates about 23 times a second.
pub const HOP: usize = FFT_SIZE / 4;

/// Below this the value is noise about zero, and the log would run away.
pub const FLOOR_DB: f32 = -120.0;

const F_LOW: f32 = 20.0;
const F_HIGH: f32 = 20_000.0;

/// One frame of analysis.
pub struct Frame {
    /// Level per display band in dBFS, where a full-scale sine reads 0.
    pub db: Vec<f32>,
    /// Loudest single sample in the window, in dBFS.
    pub peak_db: f32,
    /// A sample reached full scale, so the band values understate what came in.
    pub clipped: bool,
}

/// Where one display band reads its level.
enum Band {
    /// Wide enough to cover bins of its own, so it takes the loudest of them.
    Bins { lo: usize, hi: usize },
    /// Narrower than the bin spacing, which every band below about 200 Hz is at
    /// this window length. There is nothing to pick from, so the value comes
    /// from the two bins either side of the band's centre. Snapping to the
    /// nearer one instead makes neighbouring bands repeat a value, which draws
    /// as a staircase along the bottom octaves.
    Between { lo: usize, weight: f32 },
}

pub struct Analyzer {
    fft: Arc<dyn RealToComplex<f32>>,
    window: Vec<f32>,
    /// The last `FFT_SIZE` samples, oldest at `write`.
    ring: Vec<f32>,
    write: usize,
    since_hop: usize,
    windowed: Vec<f32>,
    spectrum: Vec<Complex<f32>>,
    scratch: Vec<Complex<f32>>,
    bands: Vec<Band>,
    centres: Vec<f32>,
    /// Turns a bin magnitude into the amplitude of the sine that produced it.
    scale: f32,
    peak: f32,
    clipped: bool,
}

impl Analyzer {
    pub fn new(sample_rate: f32) -> Self {
        let fft = RealFftPlanner::<f32>::new().plan_fft_forward(FFT_SIZE);

        let window: Vec<f32> = (0..FFT_SIZE)
            .map(|n| {
                let phase = std::f32::consts::TAU * n as f32 / FFT_SIZE as f32;
                0.5 * (1.0 - phase.cos())
            })
            .collect();

        // 2 / sum(window) undoes both the window's coherent gain and the half
        // of a real sine's energy that lands in the negative frequencies.
        let scale = 2.0 / window.iter().sum::<f32>();

        let (bands, centres) = plan_bands(sample_rate);

        Self {
            windowed: vec![0.0; FFT_SIZE],
            spectrum: fft.make_output_vec(),
            scratch: fft.make_scratch_vec(),
            fft,
            window,
            ring: vec![0.0; FFT_SIZE],
            write: 0,
            since_hop: 0,
            bands,
            centres,
            scale,
            peak: 0.0,
            clipped: false,
        }
    }

    /// The centre frequency of each band, for the frequency axis.
    pub fn centres(&self) -> &[f32] {
        &self.centres
    }

    /// Feed mono samples. `emit` runs once per hop, so a large audio buffer can
    /// produce several frames and a small one none.
    pub fn push(&mut self, mono: &[f32], mut emit: impl FnMut(Frame)) {
        for &sample in mono {
            self.ring[self.write] = sample;
            self.write = (self.write + 1) % FFT_SIZE;

            let level = sample.abs();
            self.peak = self.peak.max(level);
            self.clipped |= level >= 0.999;

            self.since_hop += 1;
            if self.since_hop >= HOP {
                self.since_hop = 0;
                emit(self.transform());
            }
        }
    }

    fn transform(&mut self) -> Frame {
        for n in 0..FFT_SIZE {
            self.windowed[n] = self.ring[(self.write + n) % FFT_SIZE] * self.window[n];
        }

        self.fft
            .process_with_scratch(&mut self.windowed, &mut self.spectrum, &mut self.scratch)
            .expect("buffers are sized by the same plan");

        let db = self
            .bands
            .iter()
            .map(|band| {
                let amplitude = match *band {
                    Band::Bins { lo, hi } => self.spectrum[lo..hi]
                        .iter()
                        .map(|c| c.norm_sqr())
                        .fold(0.0f32, f32::max)
                        .sqrt(),
                    Band::Between { lo, weight } => {
                        let (below, above) = (self.spectrum[lo].norm(), self.spectrum[lo + 1].norm());
                        below + (above - below) * weight
                    }
                };
                to_db(amplitude * self.scale)
            })
            .collect();

        let frame = Frame { db, peak_db: to_db(self.peak), clipped: self.clipped };

        self.peak = 0.0;
        self.clipped = false;

        frame
    }
}

fn to_db(amplitude: f32) -> f32 {
    if amplitude <= 0.0 {
        return FLOOR_DB;
    }
    (20.0 * amplitude.log10()).max(FLOOR_DB)
}

/// Split 20 Hz to 20 kHz into equal ratios, then map each to the bins under it.
///
/// DC and Nyquist are left out: neither is audio, and the DC bin carries any
/// offset the converter has.
fn plan_bands(sample_rate: f32) -> (Vec<Band>, Vec<f32>) {
    let bin_hz = sample_rate / FFT_SIZE as f32;
    let last = FFT_SIZE / 2 - 1;
    let top = F_HIGH.min(sample_rate * 0.45);

    let mut bands = Vec::with_capacity(BANDS);
    let mut centres = Vec::with_capacity(BANDS);

    let edge = |i: usize| F_LOW * (top / F_LOW).powf(i as f32 / BANDS as f32);

    for k in 0..BANDS {
        let (low, high) = (edge(k), edge(k + 1));
        let centre = (low * high).sqrt();

        let lo = (low / bin_hz).ceil() as usize;
        let hi = (high / bin_hz).floor() as usize + 1;

        bands.push(if hi > lo {
            Band::Bins { lo: lo.clamp(1, last), hi: hi.clamp(lo.clamp(1, last) + 1, last + 1) }
        } else {
            let position = centre / bin_hz;
            Band::Between {
                lo: (position.floor() as usize).clamp(1, last - 1),
                weight: position.fract(),
            }
        });
        centres.push(centre);
    }

    (bands, centres)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;
    const BIN_HZ: f32 = SR / FFT_SIZE as f32;

    /// Run a full window plus one hop through, and take the last frame.
    fn analyse(signal: impl Fn(usize) -> f32) -> Frame {
        let mut rta = Analyzer::new(SR);
        let samples: Vec<f32> = (0..FFT_SIZE + HOP).map(&signal).collect();

        let mut last = None;
        rta.push(&samples, |f| last = Some(f));
        last.expect("a full window plus a hop is at least one frame")
    }

    fn sine(freq: f32, amplitude: f32) -> impl Fn(usize) -> f32 {
        move |n| (std::f32::consts::TAU * freq * n as f32 / SR).sin() * amplitude
    }

    /// A tone sat exactly on a bin, so no scalloping loss is in the reading.
    fn on_bin(bin: usize) -> f32 {
        bin as f32 * BIN_HZ
    }

    /// The band that owns a frequency's bin. Nearest centre would not do: the
    /// bands are geometric, so a frequency can sit closer to one centre in Hz
    /// while its bin belongs to the next band up.
    fn band_of(freq: f32) -> usize {
        let bin = (freq / BIN_HZ).round() as usize;
        Analyzer::new(SR)
            .bands
            .iter()
            .position(|b| match *b {
                Band::Bins { lo, hi } => (lo..hi).contains(&bin),
                Band::Between { lo, .. } => lo == bin || lo + 1 == bin,
            })
            .unwrap_or_else(|| panic!("{freq} Hz is outside every band"))
    }

    #[test]
    fn full_scale_sine_reads_zero_dbfs() {
        let tone = on_bin(170);
        let frame = analyse(sine(tone, 1.0));

        let read = frame.db[band_of(tone)];
        assert!(read.abs() < 0.1, "{tone} Hz read {read} dB");
        assert!(frame.peak_db.abs() < 0.1, "peak read {} dB", frame.peak_db);
    }

    #[test]
    fn half_scale_sine_reads_six_db_down() {
        let tone = on_bin(170);
        let read = analyse(sine(tone, 0.5)).db[band_of(tone)];

        assert!((read + 6.02).abs() < 0.1, "read {read} dB");
    }

    /// Off a bin centre a Hann window loses at most 1.4 dB, which is the price
    /// of the leakage rejection that keeps neighbouring bands clean.
    #[test]
    fn between_bins_the_window_costs_under_a_decibel_and_a_half() {
        let tone = on_bin(170) + BIN_HZ / 2.0;
        let read = analyse(sine(tone, 1.0)).db[band_of(tone)];

        assert!((-1.5..=0.0).contains(&read), "{tone} Hz read {read} dB");
    }

    #[test]
    fn a_tone_stays_in_its_own_bands() {
        let tone = on_bin(170);
        let frame = analyse(sine(tone, 1.0));

        assert!(frame.db[band_of(tone)] > -0.5);
        assert!(frame.db[band_of(250.0)] < -60.0);
        assert!(frame.db[band_of(4000.0)] < -60.0);
    }

    #[test]
    fn silence_sits_on_the_floor() {
        let frame = analyse(|_| 0.0);

        assert!(frame.db.iter().all(|&v| v == FLOOR_DB));
        assert_eq!(frame.peak_db, FLOOR_DB);
        assert!(!frame.clipped);
    }

    #[test]
    fn full_scale_is_reported_as_clipping() {
        assert!(analyse(sine(on_bin(170), 1.0)).clipped);
        assert!(!analyse(sine(on_bin(170), 0.9)).clipped);
    }

    #[test]
    fn bands_ascend_and_none_is_empty() {
        let rta = Analyzer::new(SR);

        assert_eq!(rta.bands.len(), BANDS);
        assert_eq!(rta.centres().len(), BANDS);

        for pair in rta.centres().windows(2) {
            assert!(pair[1] > pair[0]);
        }
        for band in &rta.bands {
            match *band {
                Band::Bins { lo, hi } => {
                    assert!(hi > lo);
                    assert!(hi <= FFT_SIZE / 2);
                }
                Band::Between { lo, weight } => {
                    assert!(lo >= 1 && lo + 1 < FFT_SIZE / 2);
                    assert!((0.0..1.0).contains(&weight), "weight {weight}");
                }
            }
        }
    }

    /// A band narrower than a bin used to snap to the nearest one, so a run of
    /// neighbouring bands read exactly alike and drew as steps.
    #[test]
    fn the_bottom_octaves_interpolate_rather_than_repeat() {
        let frame = analyse(sine(on_bin(10), 1.0));
        let rta = Analyzer::new(SR);

        // Bands away from the tone all sit on the floor, and equal neighbours
        // there say nothing. The skirt around it is where repetition showed.
        let skirt: Vec<f32> = rta
            .centres()
            .iter()
            .enumerate()
            .filter(|(_, &hz)| (30.0..120.0).contains(&hz))
            .map(|(i, _)| frame.db[i])
            .filter(|&db| db > FLOOR_DB)
            .collect();

        assert!(skirt.len() > 8, "only {} bands carry the tone", skirt.len());
        assert!(
            skirt.windows(2).all(|pair| pair[0] != pair[1]),
            "neighbouring bands repeat: {skirt:?}"
        );
    }

    /// A device running at 44.1 kHz cannot show 20 kHz, and its top band must
    /// still land under Nyquist rather than off the end of the spectrum.
    #[test]
    fn a_lower_rate_stops_below_its_nyquist() {
        let rta = Analyzer::new(44_100.0);
        let top = *rta.centres().last().unwrap();

        assert!(top < 22_050.0, "top band at {top} Hz");
        assert!(top > 15_000.0, "top band at {top} Hz");
    }

    /// Two hops of samples are two frames, not one.
    #[test]
    fn frames_arrive_once_per_hop() {
        let mut rta = Analyzer::new(SR);
        let samples = vec![0.0; HOP * 2];

        let mut count = 0;
        rta.push(&samples, |_| count += 1);

        assert_eq!(count, 2);
    }
}
