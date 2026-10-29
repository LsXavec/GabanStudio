//! GM4: the stroke stabilizer — pulled-string smoothing, pure (no egui).
//!
//! The ink trails the pen on a string `level × STRING_PX` screen points
//! long: the pen moves freely inside the string's slack and drags the ink
//! only once the string is taut, which irons out hand tremor; a light
//! running average then rounds the corners the string leaves. Level 0 is
//! the raw pen, sample for sample. At pen-up the tail is reeled in, so the
//! stroke always ends on the pen-up point.

/// The OPTIONS row's range: 0 (raw) to 10.
pub const MAX_LEVEL: u8 = 10;
/// String length per level, in screen points (the feel holds at any zoom).
pub const STRING_PX: f32 = 2.0;

#[derive(Debug, Clone)]
pub struct Stabilizer {
    level: u8,
    /// String length in paper px.
    len: f32,
    /// Paper px per screen point (the catch-up's finest step).
    unit: f32,
    /// The string's end: where the taut string has dragged the ink.
    ink: Option<[f32; 2]>,
    /// The last point handed out (the smoothed ink).
    out: [f32; 2],
}

impl Stabilizer {
    /// `unit`: paper px per screen point (1 / zoom).
    pub fn new(level: u8, unit: f32) -> Self {
        let level = level.min(MAX_LEVEL);
        let unit = unit.max(1e-4);
        Self {
            level,
            len: level as f32 * STRING_PX * unit,
            unit,
            ink: None,
            out: [0.0; 2],
        }
    }

    /// One pen sample in; the ink point to draw out, or None while the pen
    /// is still inside the string's slack. The first sample is the ink.
    pub fn feed(&mut self, p: [f32; 2]) -> Option<[f32; 2]> {
        let Some(ink) = self.ink else {
            self.ink = Some(p);
            self.out = p;
            return Some(p);
        };
        if self.level == 0 {
            self.ink = Some(p);
            self.out = p;
            return Some(p);
        }
        let (dx, dy) = (p[0] - ink[0], p[1] - ink[1]);
        let d = (dx * dx + dy * dy).sqrt();
        if d <= self.len {
            return None;
        }
        let f = 1.0 - self.len / d;
        let ink = [ink[0] + dx * f, ink[1] + dy * f];
        self.ink = Some(ink);
        let a = 1.0 / (1.0 + self.level as f32 * 0.35);
        self.out = [
            self.out[0] + (ink[0] - self.out[0]) * a,
            self.out[1] + (ink[1] - self.out[1]) * a,
        ];
        Some(self.out)
    }

    /// Pen-up: the catch-up points from the ink to `pen_up`, the last one
    /// exactly `pen_up`. Level 0 has nothing to catch up.
    pub fn finish(&mut self, pen_up: [f32; 2]) -> Vec<[f32; 2]> {
        if self.level == 0 || self.ink.is_none() {
            return Vec::new();
        }
        let from = self.out;
        let (dx, dy) = (pen_up[0] - from[0], pen_up[1] - from[1]);
        let d = (dx * dx + dy * dy).sqrt();
        if d <= 1e-6 {
            return Vec::new();
        }
        let step = (self.len / 4.0).max(self.unit);
        let n = (d / step).ceil().max(1.0) as usize;
        let mut pts: Vec<[f32; 2]> = (1..n)
            .map(|k| {
                let t = k as f32 / n as f32;
                [from[0] + dx * t, from[1] + dy * t]
            })
            .collect();
        pts.push(pen_up);
        self.ink = Some(pen_up);
        self.out = pen_up;
        pts
    }
}

/// A whole path through a fresh stabilizer: every drawn point, then the
/// catch-up to the path's last sample (the pen-up).
pub fn smooth_path(level: u8, unit: f32, path: &[[f32; 2]]) -> Vec<[f32; 2]> {
    let mut s = Stabilizer::new(level, unit);
    let mut out: Vec<[f32; 2]> = path.iter().filter_map(|p| s.feed(*p)).collect();
    if let Some(last) = path.last() {
        out.extend(s.finish(*last));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A seeded noisy straight-ish path (LCG noise, ±3 px).
    fn noisy_path(seed: u32) -> Vec<[f32; 2]> {
        let mut s = seed;
        let mut next = move || {
            s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            s as f32 / u32::MAX as f32
        };
        (0..200)
            .map(|k| {
                let x = k as f32 * 2.0 + (next() - 0.5) * 6.0;
                let y = 40.0 * (k as f32 / 30.0).sin() + (next() - 0.5) * 6.0;
                [x, y]
            })
            .collect()
    }

    /// Mean absolute second difference: how jagged a path is.
    fn jag(p: &[[f32; 2]]) -> f32 {
        let n = p.len().saturating_sub(2).max(1) as f32;
        p.windows(3)
            .map(|w| {
                let dx = w[0][0] - 2.0 * w[1][0] + w[2][0];
                let dy = w[0][1] - 2.0 * w[1][1] + w[2][1];
                (dx * dx + dy * dy).sqrt()
            })
            .sum::<f32>()
            / n
    }

    #[test]
    fn viewnav_stabilizer_level_zero_is_the_raw_pen() {
        let raw = noisy_path(7);
        assert_eq!(smooth_path(0, 1.0, &raw), raw);
        assert_eq!(smooth_path(0, 0.37, &raw), raw);
    }

    #[test]
    fn viewnav_stabilizer_level_five_halves_the_jag_and_ends_on_pen_up() {
        for seed in [1, 7, 42, 99] {
            let raw = noisy_path(seed);
            let out = smooth_path(5, 1.0, &raw);
            let (before, after) = (jag(&raw), jag(&out));
            assert!(after <= 0.5 * before, "seed {seed}: jag {before} → {after}");
            assert_eq!(out.last(), raw.last(), "seed {seed}: the tail reaches pen-up");
        }
        // Every level above 0 ends exactly on pen-up too.
        let raw = noisy_path(3);
        for level in 1..=MAX_LEVEL {
            assert_eq!(smooth_path(level, 0.5, &raw).last(), raw.last(), "level {level}");
        }
    }
}
