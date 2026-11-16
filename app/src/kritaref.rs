//! Krita 5.2 brush math, ported as the reference that brush-fidelity tests
//! measure Gaban's dabs against (test-only; no Krita renders are possible on
//! this PC, see LESSONS.md "brush refs (try 4)").
//!
//! Source: github.com/KDE/krita, branch krita/5.2. Each item names the file and
//! function it ports. Krita works on an 8-bit RGBA canvas here, so the mask and
//! the compositing keep its u8 arithmetic (UINT8_MULT / UINT8_BLEND rounding).
//!
//! Scope: the auto-brush circle mask ("default" and "soft" curve), dab spacing
//! and the dab walk along a stroke, and the paintbrush stroke compositing
//! (creamy alpha darken into the stroke buffer, opacity once per stroke).
//! Not ported: randomness and density (all reference presets use 0 and 1), the
//! rectangle and gaussian generators (no reference preset uses them), predefined
//! tips (gbr/png/svg are images, not formulas).

// ---------------------------------------------------------------- u8 maths
// libs/pigment/kis_global.h UINT8_MULT, UINT8_DIVIDE, UINT8_BLEND.

pub fn u8_mult(a: u32, b: u32) -> u32 {
    let c = a * b + 0x80;
    ((c >> 8) + c) >> 8
}

pub fn u8_divide(a: u32, b: u32) -> u32 {
    if b == 0 {
        return 255;
    }
    (a * 255 + b / 2) / b
}

/// `(a - b) * alpha + b`, rounded as Krita does.
pub fn u8_blend(a: u32, b: u32, alpha: u32) -> u32 {
    let mut c = (a as i32 - b as i32) * alpha as i32 + 0x80;
    c = ((c >> 8) + c) >> 8;
    (c + b as i32) as u32
}

/// KoCompositeOpFunctions Arithmetic::lerp(a, b, alpha) = blend(b, a, alpha).
pub fn u8_lerp(a: u32, b: u32, alpha: u32) -> u32 {
    u8_blend(b, a, alpha)
}

/// KoColorSpaceMaths<float, quint8>::scaleToA: float2int(clamp(v * 255)).
pub fn u8_from_f(v: f64) -> u32 {
    (v * 255.0).clamp(0.0, 255.0).round() as u32
}

// ---------------------------------------------------------------- spline
// libs/image/kis_cubic_curve.cpp KisCubicSpline (natural cubic spline:
// second derivative 0 at both ends) and KisCubicCurve::Data::value (x clamped
// to the curve's range, y clamped to 0..1).

pub struct Spline {
    x0: Vec<f64>,
    a: Vec<f64>,
    b: Vec<f64>,
    c: Vec<f64>,
    d: Vec<f64>,
    h: Vec<f64>,
}

impl Spline {
    /// KisCubicSpline::createSpline. Points must be sorted by x (keepSorted).
    pub fn new(pts: &[(f64, f64)]) -> Spline {
        let n = pts.len() - 1;
        let h: Vec<f64> = (0..n).map(|i| pts[i + 1].0 - pts[i].0).collect();
        let a: Vec<f64> = pts.iter().map(|p| p.1).collect();
        // Tridiagonal system for the interior second derivatives
        // (KisTridiagonalSystem::calculate, Thomas algorithm).
        let m = n.saturating_sub(1);
        let mut c_in = vec![0.0; m];
        if m > 0 {
            let diag: Vec<f64> = (0..m).map(|i| 2.0 * (h[i] + h[i + 1])).collect();
            let rhs: Vec<f64> = (0..m)
                .map(|i| 6.0 * ((a[i + 2] - a[i + 1]) / h[i + 1] - (a[i + 1] - a[i]) / h[i]))
                .collect();
            let off: Vec<f64> = (1..m).map(|i| h[i]).collect();
            let mut cp = vec![0.0; m];
            let mut dp = vec![0.0; m];
            cp[0] = if m > 1 { off[0] / diag[0] } else { 0.0 };
            dp[0] = rhs[0] / diag[0];
            for i in 1..m {
                let den = diag[i] - off[i - 1] * cp[i - 1];
                cp[i] = if i < m - 1 { off[i] / den } else { 0.0 };
                dp[i] = (rhs[i] - off[i - 1] * dp[i - 1]) / den;
            }
            c_in[m - 1] = dp[m - 1];
            for i in (0..m - 1).rev() {
                c_in[i] = dp[i] - cp[i] * c_in[i + 1];
            }
        }
        let mut c = vec![0.0];
        c.extend(c_in);
        c.push(0.0);
        let d: Vec<f64> = (0..n).map(|i| (c[i + 1] - c[i]) / h[i]).collect();
        let b: Vec<f64> = (0..n)
            .map(|i| -0.5 * c[i] * h[i] - d[i] * h[i] * h[i] / 6.0 + (a[i + 1] - a[i]) / h[i])
            .collect();
        let x0 = pts.iter().map(|p| p.0).collect();
        Spline { x0, a, b, c, d, h }
    }

    /// KisCubicSpline::getValue with findRegion.
    pub fn eval(&self, x: f64) -> f64 {
        let n = self.h.len();
        let mut i = n - 1;
        for k in 0..n {
            if x >= self.x0[k] && x < self.x0[k] + self.h[k] {
                i = k;
                break;
            }
        }
        let t = x - self.x0[i];
        self.a[i] + self.b[i] * t + 0.5 * self.c[i] * t * t + self.d[i] * t * t * t / 6.0
    }

    /// KisCubicCurve::Data::value.
    pub fn value(&self, x: f64) -> f64 {
        let x = x.clamp(self.x0[0], *self.x0.last().unwrap());
        self.eval(x).clamp(0.0, 1.0)
    }

    /// KisCubicCurve::floatTransfer(size) (Data::updateTransfer).
    pub fn float_transfer(&self, size: usize) -> Vec<f64> {
        let end = 1.0 / (size as f64 - 1.0);
        (0..size).map(|i| self.value(i as f64 * end).clamp(0.0, 1.0)).collect()
    }
}

/// Krita's curve string "x,y;x,y;" (KisCubicCurve::fromString).
pub fn parse_curve(s: &str) -> Vec<(f64, f64)> {
    let mut v: Vec<(f64, f64)> = s
        .split(';')
        .filter_map(|p| {
            let (x, y) = p.split_once(',')?;
            Some((x.trim().parse().ok()?, y.trim().parse().ok()?))
        })
        .collect();
    v.sort_by(|a, b| a.0.total_cmp(&b.0));
    v
}

// ---------------------------------------------------------------- masks

pub enum Shape {
    /// KisCircleMaskGenerator ("default"): hfade/vfade as in the .kpp.
    Default { hfade: f64, vfade: f64 },
    /// KisCurveCircleMaskGenerator ("soft"): the softness_curve points.
    Soft { curve: Vec<(f64, f64)> },
}

pub struct CircleMask {
    /// The preset's diameter (KisMaskGenerator::width()); the dab's diameter is
    /// given per dab, as Krita's setScale does.
    pub base_diameter: f64,
    pub ratio: f64,
    pub spikes: u32,
    pub antialias: bool,
    pub shape: Shape,
    curve_res: usize,
    curve_data: Vec<f64>,
}

impl CircleMask {
    pub fn new(base_diameter: f64, ratio: f64, spikes: u32, antialias: bool, shape: Shape) -> CircleMask {
        // kis_curve_circle_mask_generator.cpp constructor:
        // curveResolution = qRound(max(width, height) * OVERSAMPLING(4)),
        // curveData = floatTransfer(curveResolution + 2).
        let h = if spikes == 2 { base_diameter * ratio } else { base_diameter };
        let curve_res = (base_diameter.max(h) * 4.0).round() as usize;
        let curve_data = match &shape {
            Shape::Soft { curve } => Spline::new(curve).float_transfer(curve_res + 2),
            Shape::Default { .. } => Vec::new(),
        };
        CircleMask { base_diameter, ratio, spikes, antialias, shape, curve_res, curve_data }
    }

    /// KisMaskGenerator::fixRotation: fold the angle into one spike.
    fn fix_rotation(&self, xr: &mut f64, yr: &mut f64) {
        if self.spikes > 2 {
            let spike = std::f64::consts::PI / self.spikes as f64;
            let (cs, ss) = ((-2.0 * spike).cos(), (-2.0 * spike).sin());
            let mut angle = yr.atan2(*xr);
            while angle > spike {
                let (sx, sy) = (*xr, *yr);
                *xr = cs * sx - ss * sy;
                *yr = ss * sx + cs * sy;
                angle -= 2.0 * spike;
            }
        }
    }

    /// Krita mask byte at (x, y) from the dab centre for a dab of `diameter`:
    /// 0 opaque, 255 transparent (KisCircleMaskGenerator::valueAt and
    /// KisCurveCircleMaskGenerator::valueAt; softness 1).
    pub fn value_at(&self, diameter: f64, x: f64, y: f64) -> u32 {
        let w = diameter;
        let h = diameter * self.ratio;
        let xcoef = 2.0 / w;
        let ycoef = 2.0 / h;
        let mut xr = x;
        let mut yr = y.abs();
        self.fix_rotation(&mut xr, &mut yr);
        match &self.shape {
            Shape::Default { hfade, vfade } => {
                // setScale: xfadecoef = fade == 0 ? 1 : 2 / (fade * width);
                // transformedFade = fadecoef * safeSoftnessCoeff (1 / max(0.01, 1)).
                let tfx = if *hfade == 0.0 { 1.0 } else { 2.0 / (hfade * w) };
                let tfy = if *vfade == 0.0 { 1.0 } else { 2.0 / (vfade * h) };
                let n = (xr * xcoef).powi(2) + (yr * ycoef).powi(2);
                if n > 1.0 {
                    return 255;
                }
                if self.antialias {
                    // "we add +1.0 to ensure correct antialiasing on the border"
                    xr = xr.abs() + 1.0;
                    yr = yr.abs() + 1.0;
                }
                let nf = (xr * tfx).powi(2) + (yr * tfy).powi(2);
                if nf < 1.0 {
                    return 0;
                }
                (255.0 * n * (nf - 1.0) / (nf - n)) as u32
            }
            Shape::Soft { .. } => {
                let dist = (xr * xcoef).powi(2) + (yr * ycoef).powi(2);
                // KisAntialiasingFadeMaker1D::setSquareNormCoeffs / needFade.
                if dist > 1.0 {
                    return 255;
                }
                if self.antialias {
                    let xf = (1.0 - xcoef).max(0.0);
                    let yf = (1.0 - ycoef).max(0.0);
                    let start = (0.5 * (xf + yf)).powi(2);
                    let start_v = self.curve_value(start) as f64;
                    let coeff = (255.0 - start_v).max(0.0) / (1.0 - start);
                    if dist > start {
                        return (start_v + (dist - start) * coeff) as u32;
                    }
                }
                self.curve_value(dist)
            }
        }
    }

    /// KisCurveCircleMaskGenerator::Private::value.
    fn curve_value(&self, dist: f64) -> u32 {
        let distance = dist * self.curve_res as f64;
        let i = distance as usize;
        let f = distance - i as f64;
        let a = (1.0 - f) * self.curve_data[i] + f * self.curve_data[(i + 1).min(self.curve_data.len() - 1)];
        ((1.0 - a) * 255.0) as u32
    }

    /// Mask alpha (0..255) of the pixel whose centre is (dx, dy) from the dab
    /// position: KisBrushMaskScalarApplicator::processScalar, with Krita's
    /// supersampling for small antialiased dabs (KisMaskGenerator::
    /// shouldSupersample: < 10 px is 3x3, < 1 px is 6x6) and its sample
    /// offsets x + s / ss (not centred). Pixel centre = dab pixel x minus
    /// centerX = hotSpot - 0.5 + subPixel (KisAutoBrush::generateMask...).
    pub fn alpha(&self, diameter: f64, dx: f64, dy: f64) -> u32 {
        let w = diameter;
        let h = diameter * self.ratio;
        let ss: usize = if self.antialias && (w < 10.0 || h < 10.0) {
            if w < 1.0 || h < 1.0 { 6 } else { 3 }
        } else {
            1
        };
        let inv = 1.0 / ss as f64;
        let mut sum = 0u32;
        for sy in 0..ss {
            for sx in 0..ss {
                sum += self.value_at(diameter, dx + sx as f64 * inv, dy + sy as f64 * inv);
            }
        }
        255 - sum / (ss * ss) as u32
    }
}

// ---------------------------------------------------------------- spacing
// libs/image/brushengine/kis_paintop_utils.{h,cpp}: calcAutoSpacing and
// effectiveSpacing (lodScale 1, extraScale 1).

pub fn auto_spacing(value: f64, coeff: f64) -> f64 {
    coeff * if value < 1.0 { value } else { value.sqrt() }
}

/// Spacing in pixels (x along the dab's width, y along its height).
pub fn effective_spacing(
    dab_w: f64,
    dab_h: f64,
    isotropic: bool,
    spacing: f64,
    auto_active: bool,
    auto_coeff: f64,
) -> (f64, f64) {
    if !isotropic {
        if auto_active {
            (auto_spacing(dab_w, auto_coeff), auto_spacing(dab_h, auto_coeff))
        } else {
            (dab_w * spacing, dab_h * spacing)
        }
    } else {
        let s = dab_w.max(dab_h);
        let s = if auto_active { auto_spacing(s, auto_coeff) } else { s * spacing };
        (s, s)
    }
}

/// libs/image/kis_distance_information.cpp MIN_DISTANCE_SPACING.
pub const MIN_SPACING: f64 = 0.5;

/// One input point of a stroke: position and pressure.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pt {
    pub x: f64,
    pub y: f64,
    pub p: f64,
}

/// The dabs Krita paints along a polyline (KisPaintOpUtils::paintLine with
/// KisDistanceInformation::getNextPointPositionIsotropic / Anisotropic): the
/// first dab at the first point, then each next dab where the accumulated
/// distance from the last dab reaches the spacing of the last dab (spacing is
/// re-read from each painted dab, `spacing_at(pressure)`). Position and
/// pressure are interpolated linearly (KisPaintInformation::mix). Rotation 0.
pub fn dab_walk(points: &[Pt], spacing_at: impl Fn(f64) -> (f64, f64)) -> Vec<Pt> {
    let mut out = Vec::new();
    let Some(first) = points.first() else { return out };
    out.push(*first);
    let mut sp = spacing_at(first.p);
    let mut acc = (0.0f64, 0.0f64);
    for seg in points.windows(2) {
        let mut cur = seg[0];
        let end = seg[1];
        loop {
            if cur.x == end.x && cur.y == end.y {
                break;
            }
            let (dxv, dyv) = (end.x - cur.x, end.y - cur.y);
            let t = if sp.0 == sp.1 {
                let spacing = sp.0.max(MIN_SPACING);
                let len = (dxv * dxv + dyv * dyv).sqrt();
                let next = spacing - acc.0;
                if next <= 0.0 {
                    0.0
                } else if next <= len {
                    next / len
                } else {
                    acc.0 += len;
                    -1.0
                }
            } else {
                let ar = 1.0 / sp.0.max(MIN_SPACING);
                let br = 1.0 / sp.1.max(MIN_SPACING);
                let gamma = (acc.0 * ar).powi(2) + (acc.1 * br).powi(2) - 1.0;
                if gamma >= 0.0 {
                    0.0
                } else {
                    let (dx, dy) = (dxv.abs(), dyv.abs());
                    let alpha = (dx * ar).powi(2) + (dy * br).powi(2);
                    let beta = acc.0 * dx * ar * ar + acc.1 * dy * br * br;
                    let d4 = beta * beta - alpha * gamma;
                    let k = (-beta + d4.max(0.0).sqrt()) / alpha;
                    if (0.0..=1.0).contains(&k) {
                        k
                    } else {
                        acc.0 += dx;
                        acc.1 += dy;
                        -1.0
                    }
                }
            };
            if t < 0.0 {
                break;
            }
            acc = (0.0, 0.0);
            cur = Pt {
                x: cur.x + (end.x - cur.x) * t,
                y: cur.y + (end.y - cur.y) * t,
                p: cur.p + (end.p - cur.p) * t,
            };
            out.push(cur);
            sp = spacing_at(cur.p);
        }
    }
    out
}

// ---------------------------------------------------------------- stroke
// The paintbrush paints dabs into the stroke's temporary target with
// COMPOSITE_ALPHA_DARKEN (creamy wrapper, the Krita 5 default:
// libs/pigment/compositeops/KoCompositeOpAlphaDarken.h genericComposite,
// KoAlphaDarkenParamsWrapper.h KoAlphaDarkenParamsWrapperCreamy). In wash mode
// (indirect painting) a dab's opacity is the opacity sensor only
// (KisFlowOpacityOption2::apply, computeSizeLikeValue(info, false)); the
// preset's opacity is applied once when the stroke merges into the layer
// (KisIndirectPaintingSupport::setTemporaryOpacity, COMPOSITE_OVER). Onto an
// empty layer this equals build-up mode, where the dab opacity carries it.

pub struct StrokeBuffer {
    pub w: usize,
    pub h: usize,
    /// Stroke alpha, 0..255.
    pub a: Vec<u32>,
    /// KisDabRenderingQueue averageOpacity, starts at 0.
    avg: f64,
}

impl StrokeBuffer {
    pub fn new(w: usize, h: usize) -> StrokeBuffer {
        StrokeBuffer { w, h, a: vec![0; w * h], avg: 0.0 }
    }

    /// KoCompositeOpAlphaDarken::genericComposite, alpha channel, for one
    /// pixel: mask m, dab opacity o and flow f as u8, average opacity avg.
    pub fn alpha_darken(d: u32, m: u32, o: u32, f: u32, avg: u32, flow_is_one: bool) -> u32 {
        let src = u8_mult(m, o);
        let full = if avg > o {
            let rev = u8_divide(d, avg);
            if avg > d { u8_lerp(src, avg, rev) } else { d }
        } else if o > d {
            u8_lerp(d, o, m)
        } else {
            d
        };
        if flow_is_one {
            full
        } else {
            // Creamy calculateZeroFlowAlphaLegacy returns dstAlpha.
            u8_lerp(d, full, f)
        }
    }

    /// Paint one dab centred at (cx, cy) (canvas pixels, pixel centres at
    /// +0.5), dab diameter `diameter`, sensor opacity `opacity` and `flow`.
    pub fn dab(&mut self, mask: &CircleMask, cx: f64, cy: f64, diameter: f64, opacity: f64, flow: f64) {
        // KisPainter::blendAverageOpacity (exponent 0.1), per dab
        // (KisDabRenderingQueue::Private: averageOpacity starts at 0).
        self.avg = if self.avg < opacity { opacity } else { 0.1 * opacity + 0.9 * self.avg };
        let o = u8_from_f(opacity);
        let f = u8_from_f(flow);
        let avg = u8_from_f(self.avg);
        let r = diameter.max(diameter * mask.ratio) / 2.0 + 2.0;
        let x0 = ((cx - r).floor().max(0.0)) as usize;
        let y0 = ((cy - r).floor().max(0.0)) as usize;
        let x1 = ((cx + r).ceil() as usize).min(self.w);
        let y1 = ((cy + r).ceil() as usize).min(self.h);
        for y in y0..y1 {
            for x in x0..x1 {
                let m = mask.alpha(diameter, x as f64 + 0.5 - cx, y as f64 + 0.5 - cy);
                if m == 0 {
                    continue;
                }
                let i = y * self.w + x;
                self.a[i] = Self::alpha_darken(self.a[i], m, o, f, avg, flow >= 1.0);
            }
        }
    }

    /// Merge into an empty layer at the preset's opacity (COMPOSITE_OVER at
    /// opacity O onto transparent: alpha = mul(stroke, O)); 0..1 per pixel.
    pub fn merged(&self, opacity: f64) -> Vec<f32> {
        let o = u8_from_f(opacity);
        self.a.iter().map(|&a| u8_mult(a, o) as f32 / 255.0).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hard(d: f64, fade: f64, aa: bool) -> CircleMask {
        CircleMask::new(d, 1.0, 2, aa, Shape::Default { hfade: fade, vfade: fade })
    }

    #[test]
    fn u8_maths_round_like_krita() {
        assert_eq!(u8_mult(255, 255), 255);
        assert_eq!(u8_mult(255, 128), 128);
        assert_eq!(u8_mult(128, 128), 64);
        assert_eq!(u8_blend(255, 0, 128), 128);
        assert_eq!(u8_lerp(128, 255, 128), 192);
        assert_eq!(u8_from_f(0.5), 128);
        assert_eq!(u8_divide(128, 255), 128);
    }

    #[test]
    fn spline_natural_cubic_values() {
        // Collinear points stay linear.
        let s = Spline::new(&[(0.0, 0.0), (0.5, 0.5), (1.0, 1.0)]);
        assert!((s.value(0.25) - 0.25).abs() < 1e-12);
        // (0,0),(0.5,1),(1,0) by hand: c1 = -12, b0 = 3, d0 = -24,
        // s(0.25) = 0.75 - 0.0625 = 0.6875.
        let s = Spline::new(&[(0.0, 0.0), (0.5, 1.0), (1.0, 0.0)]);
        assert!((s.eval(0.25) - 0.6875).abs() < 1e-12, "{}", s.eval(0.25));
        assert!((s.eval(0.5) - 1.0).abs() < 1e-12);
        // Two points: a straight line; value clamps y to 0..1.
        let s = Spline::new(&[(0.0, 0.2), (1.0, 1.4)]);
        assert!((s.value(0.5) - 0.8).abs() < 1e-12);
        assert_eq!(s.value(1.0), 1.0);
        assert_eq!(parse_curve("0,0.495496;0.253012,0.198198;1,0;").len(), 3);
    }

    #[test]
    fn default_circle_centre_edge_and_fade() {
        let m = hard(30.0, 0.9, true);
        assert_eq!(m.value_at(30.0, 0.0, 0.0), 0, "centre opaque");
        assert_eq!(m.value_at(30.0, 15.1, 0.0), 255, "outside the radius");
        // Hand value, d 20, fade 0.5, no antialias: xcoef 0.1, fade coef 0.2;
        // at x 7.5: n = 0.5625, nf = 2.25,
        // 255 * 0.5625 * 1.25 / 1.6875 = 106.25 -> 106.
        let m = hard(20.0, 0.5, false);
        assert_eq!(m.value_at(20.0, 7.5, 0.0), 106);
        assert_eq!(m.value_at(20.0, 0.0, 7.5), 106, "round: same along y");
        assert_eq!(m.alpha(20.0, 7.5, 0.0), 149);
        // Inside the fade start (r < fade * d / 2 = 5) is fully opaque.
        assert_eq!(m.value_at(20.0, 4.9, 0.0), 0);
        // Antialias shifts by +1: at x 4.0 nf = (5 * 0.2)^2 = 1.0, still opaque
        // only below; at x 4.5 it fades.
        let a = hard(20.0, 0.5, true);
        assert!(a.value_at(20.0, 4.5, 0.0) > 0);
    }

    #[test]
    fn small_antialiased_dabs_supersample() {
        // d 6 < 10 with antialias: 3x3 samples at +0, +1/3, +2/3.
        let m = hard(6.0, 0.9, true);
        let want: u32 = (0..3)
            .flat_map(|sy| (0..3).map(move |sx| (sx, sy)))
            .map(|(sx, sy)| m.value_at(6.0, 2.0 + sx as f64 / 3.0, sy as f64 / 3.0))
            .sum::<u32>()
            / 9;
        assert_eq!(m.alpha(6.0, 2.0, 0.0), 255 - want);
        let big = hard(30.0, 0.9, true);
        assert_eq!(big.alpha(30.0, 2.0, 0.0), 255 - big.value_at(30.0, 2.0, 0.0));
    }

    #[test]
    fn soft_curve_mask_follows_its_curve() {
        // Linear curve (0,1)-(1,0), d 20, no antialias: resolution 80, transfer
        // of 82 samples at i / 81. At dist 0.5: index 40, data 1 - 40/81,
        // value (40/81) * 255 = 125.9 -> 125.
        let m = CircleMask::new(20.0, 1.0, 2, false, Shape::Soft { curve: vec![(0.0, 1.0), (1.0, 0.0)] });
        let x = (0.5f64).sqrt() * 10.0;
        assert_eq!(m.value_at(20.0, x, 0.0), 125);
        assert_eq!(m.value_at(20.0, 0.0, 0.0), 0);
        assert_eq!(m.value_at(20.0, 10.01, 0.0), 255);
        // The airbrush preset's curve: centre alpha is its first point, 0.4955.
        let air = CircleMask::new(
            300.0,
            1.0,
            2,
            false,
            Shape::Soft { curve: parse_curve("0,0.495496;0.253012,0.198198;0.506024,0.0726474;1,0;") },
        );
        assert_eq!(air.alpha(300.0, 0.0, 0.0), 255 - ((1.0 - 0.495496) * 255.0) as u32);
        // Antialiased: the last stretch ramps linearly to 255 at the edge.
        let aa = CircleMask::new(20.0, 1.0, 2, true, Shape::Soft { curve: vec![(0.0, 1.0), (1.0, 0.0)] });
        assert_eq!(aa.value_at(20.0, 9.99, 0.0), 254);
        assert_eq!(aa.value_at(20.0, 0.0, 0.0), 0);
    }

    #[test]
    fn spikes_fold_before_the_antialias_offset() {
        // Without antialias a round circle is the same at any spike count.
        let a = CircleMask::new(10.0, 1.0, 9, false, Shape::Default { hfade: 0.9, vfade: 0.9 });
        let b = hard(10.0, 0.9, false);
        for &(x, y) in &[(3.0, 2.0), (4.2, -1.0), (-2.0, 4.5)] {
            let d = a.value_at(10.0, x, y) as i32 - b.value_at(10.0, x, y) as i32;
            assert!(d.abs() <= 1, "({x},{y}) {d}");
        }
        // With antialias Krita adds 1 to |x| and |y| after the fold, so the
        // 9-spike "Ink gpen 10" tip fades differently off the axes.
        let a = CircleMask::new(10.0, 1.0, 9, true, Shape::Default { hfade: 0.9, vfade: 0.9 });
        let b = hard(10.0, 0.9, true);
        assert_ne!(a.value_at(10.0, 3.0, 2.0), b.value_at(10.0, 3.0, 2.0));
        assert_eq!(a.value_at(10.0, 4.0, 0.0), b.value_at(10.0, 4.0, 0.0), "on the axis no fold");
    }

    #[test]
    fn spacing_values() {
        assert_eq!(effective_spacing(40.0, 40.0, false, 0.1, false, 1.0), (4.0, 4.0));
        assert_eq!(effective_spacing(40.0, 40.0, false, 0.25, false, 1.0), (10.0, 10.0));
        let (s, _) = effective_spacing(40.0, 40.0, false, 0.1, true, 0.8);
        assert!((s - 0.8 * 40f64.sqrt()).abs() < 1e-12, "auto: coeff * sqrt(size)");
        assert!((auto_spacing(0.5, 0.8) - 0.4).abs() < 1e-12, "auto below 1 px is linear");
        assert_eq!(effective_spacing(40.0, 20.0, true, 0.1, false, 1.0), (4.0, 4.0));
        assert_eq!(effective_spacing(40.0, 20.0, false, 0.1, false, 1.0), (4.0, 2.0));
    }

    #[test]
    fn dab_walk_places_dabs_at_spacing_across_segments() {
        let p = |x: f64| Pt { x, y: 0.0, p: x / 100.0 };
        let a = dab_walk(&[p(0.0), p(100.0)], |_| (10.0, 10.0));
        let b = dab_walk(&[p(0.0), p(3.0), p(25.0), p(100.0)], |_| (10.0, 10.0));
        assert_eq!(a.len(), 11);
        assert_eq!(b.len(), 11);
        for (d, e) in a.iter().zip(&b) {
            assert!((d.x - e.x).abs() < 1e-9 && (d.p - e.p).abs() < 1e-9);
        }
        assert!((a[3].x - 30.0).abs() < 1e-9 && (a[3].p - 0.3).abs() < 1e-9);
        // Spacing clamps to 0.5 px.
        assert_eq!(dab_walk(&[p(0.0), p(10.0)], |_| (0.1, 0.1)).len(), 21);
        // Spacing follows the last dab's pressure: 1 + 10 * p.
        let c = dab_walk(&[p(0.0), p(100.0)], |pr| (1.0 + 10.0 * pr, 1.0 + 10.0 * pr));
        assert!((c[1].x - 1.0).abs() < 1e-9);
        assert!((c[2].x - 2.1).abs() < 1e-9, "{}", c[2].x);
        // Anisotropic ellipse, moving along x: step = spacing x.
        let d = dab_walk(&[p(0.0), p(20.0)], |_| (4.0, 2.0));
        assert!((d[1].x - 4.0).abs() < 1e-9);
    }

    #[test]
    fn flow_builds_per_dab_opacity_caps_the_stroke() {
        // Two full-mask dabs on one pixel.
        let d1 = StrokeBuffer::alpha_darken(0, 255, 255, 128, 255, false);
        let d2 = StrokeBuffer::alpha_darken(d1, 255, 255, 128, 255, false);
        assert_eq!((d1, d2), (128, 192), "flow 0.5: 1 - 0.5^n");
        // Opacity 0.5 flow 1: the first dab reaches 0.5, the second adds nothing.
        let e1 = StrokeBuffer::alpha_darken(0, 255, 128, 255, 128, true);
        let e2 = StrokeBuffer::alpha_darken(e1, 255, 128, 255, 128, true);
        assert_eq!((e1, e2), (128, 128));
        // Through the buffer: preset opacity 0.5 at merge, flow 0.5 in dabs.
        let m = hard(20.0, 1.0, false);
        let mut s = StrokeBuffer::new(40, 40);
        s.dab(&m, 20.0, 20.0, 20.0, 1.0, 0.5);
        s.dab(&m, 20.0, 20.0, 20.0, 1.0, 0.5);
        let out = s.merged(0.5);
        assert_eq!(s.a[20 * 40 + 20], 192);
        assert!((out[20 * 40 + 20] - 96.0 / 255.0).abs() < 1e-6);
        assert_eq!(out[0], 0.0);
    }

    #[test]
    fn falling_opacity_aims_at_the_running_average() {
        // KisDabRenderingQueue: avg = opacity on a rise, else 0.1 o + 0.9 avg.
        let m = hard(20.0, 1.0, false);
        let mut s = StrokeBuffer::new(40, 40);
        s.dab(&m, 20.0, 20.0, 20.0, 1.0, 1.0);
        assert_eq!(s.a[20 * 40 + 20], 255);
        s.dab(&m, 5.0, 5.0, 20.0, 0.2, 1.0);
        // avg = 0.1 * 0.2 + 0.9 = 0.92 > 0.2: the fresh pixel strives to src
        // alpha 0.2 lerped toward 0.92 by d / avg = 0, so 0.2 (51).
        assert!((s.avg - 0.92).abs() < 1e-12);
        assert_eq!(s.a[5 * 40 + 5], 51);
    }
}
