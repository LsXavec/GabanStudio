//! Document model: Project -> Scenes -> Cuts.
//!
//! The Cut is the atomic production unit (straight from the anime pipeline):
//! it owns a drawing library, an X-sheet, and a node graph. The X-sheet
//! references drawings by id; the graph references X-sheet columns by id.

use std::sync::Arc;

use crate::graph::Graph;
use crate::ids::*;
use crate::raster::{RasterLayer, TileCoord, TileData};
use crate::xsheet::XSheet;

/// One sampled pen point in paper coordinates (resolution independent).
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct StrokePoint {
    pub x: f32,
    pub y: f32,
    /// 0..1 tablet pressure (0.5 for mouse input).
    pub pressure: f32,
    /// Pen tilt from vertical in radians, [x, y] per octotablet's convention
    /// ([+,+] = right + toward the user). [0, 0] = vertical pen — and also
    /// mouse/legacy input, so old files load with a vertical pen (serde
    /// default). NOT folded into `content_hash`: vector rendering doesn't
    /// consume tilt yet, and the hash law folds only what reaches pixels —
    /// fold it the day a vector renderer draws tilt-shaped ribbons.
    #[serde(default)]
    pub tilt: [f32; 2],
}

/// A single pen stroke. Rendered width = base_width * point pressure.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Stroke {
    pub points: Vec<StrokePoint>,
    pub base_width: f32,
    pub color: [u8; 4],
}

/// What a cel layer IS in the anime pipeline (PSD-cel-layers). The ROLE —
/// never the display name — drives behaviour: stack position, identity
/// colour, the line guard, frame-to-frame follow. Names are labels.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
pub enum LayerRole {
    /// LO / BG plate to draw against.
    Reference,
    /// Shiage base paint.
    Color,
    /// Kage — the shadow colour region.
    Shadow,
    /// Hi — the highlight colour region.
    Highlight,
    /// Blue-pencil rough.
    Rough,
    /// Kage-sen — the shadow boundary trace line.
    ShadowLine,
    /// Hi-sen — the highlight boundary trace line.
    HighlightLine,
    /// The main trace line (genga / douga clean line).
    Line,
    /// Sakkan correction overlay.
    Correction,
    /// Anything else — no pipeline position.
    #[default]
    Custom,
}

impl LayerRole {
    /// Every role, in pipeline order (bottom → top), Custom last.
    pub const ALL: [LayerRole; 10] = [
        LayerRole::Reference,
        LayerRole::Color,
        LayerRole::Shadow,
        LayerRole::Highlight,
        LayerRole::Rough,
        LayerRole::ShadowLine,
        LayerRole::HighlightLine,
        LayerRole::Line,
        LayerRole::Correction,
        LayerRole::Custom,
    ];

    /// Stable persisted key (file + config). Never rename these.
    pub fn key(self) -> &'static str {
        match self {
            LayerRole::Reference => "reference",
            LayerRole::Color => "color",
            LayerRole::Shadow => "shadow",
            LayerRole::Highlight => "highlight",
            LayerRole::Rough => "rough",
            LayerRole::ShadowLine => "shadow_line",
            LayerRole::HighlightLine => "highlight_line",
            LayerRole::Line => "line",
            LayerRole::Correction => "correction",
            LayerRole::Custom => "custom",
        }
    }

    pub fn from_key(key: &str) -> Option<LayerRole> {
        LayerRole::ALL.into_iter().find(|r| r.key() == key)
    }

    /// Display label (the anime term in parentheses where it helps).
    pub fn label(self) -> &'static str {
        match self {
            LayerRole::Reference => "reference (LO)",
            LayerRole::Color => "color",
            LayerRole::Shadow => "shadow (kage)",
            LayerRole::Highlight => "highlight (hi)",
            LayerRole::Rough => "rough",
            LayerRole::ShadowLine => "shadow line (kage-sen)",
            LayerRole::HighlightLine => "highlight line (hi-sen)",
            LayerRole::Line => "line",
            LayerRole::Correction => "correction (sakkan)",
            LayerRole::Custom => "custom",
        }
    }

    /// The name a fresh layer of this role gets (renameable — the role
    /// stays). Chosen so `infer(default_name)` returns the role back.
    pub fn default_name(self) -> &'static str {
        match self {
            LayerRole::Reference => "reference",
            LayerRole::Color => "color",
            LayerRole::Shadow => "shadow",
            LayerRole::Highlight => "highlight",
            LayerRole::Rough => "rough",
            LayerRole::ShadowLine => "shadow line",
            LayerRole::HighlightLine => "highlight line",
            LayerRole::Line => "line",
            LayerRole::Correction => "correction",
            LayerRole::Custom => "layer",
        }
    }

    /// Pipeline stack rank, bottom → top. Custom has none (it inserts
    /// beside the active layer instead).
    pub fn rank(self) -> Option<u8> {
        match self {
            LayerRole::Reference => Some(0),
            LayerRole::Color => Some(1),
            LayerRole::Shadow => Some(2),
            LayerRole::Highlight => Some(3),
            LayerRole::Rough => Some(4),
            LayerRole::ShadowLine => Some(5),
            LayerRole::HighlightLine => Some(6),
            LayerRole::Line => Some(7),
            LayerRole::Correction => Some(8),
            LayerRole::Custom => None,
        }
    }

    /// Trace-line roles — what the line guard protects.
    pub fn is_line(self) -> bool {
        matches!(
            self,
            LayerRole::Line | LayerRole::ShadowLine | LayerRole::HighlightLine
        )
    }

    /// The role a legacy layer NAME implies — applied ONCE, when a file
    /// saved before roles existed loads (and when code creates a layer by
    /// name). Unknown names are Custom: guessing wrong is worse.
    pub fn infer(name: &str) -> LayerRole {
        let n = name.trim().to_lowercase();
        match n.as_str() {
            "line" | "lines" | "genga" | "douga" | "clean" | "trace" | "ink" => LayerRole::Line,
            "color" | "colour" | "paint fill" | "fill" | "base" => LayerRole::Color,
            "shadow" | "kage" => LayerRole::Shadow,
            "highlight" | "hi" => LayerRole::Highlight,
            "rough" | "roughs" | "sketch" => LayerRole::Rough,
            "shadow line" | "shadow_line" | "kage-sen" | "kagesen" => LayerRole::ShadowLine,
            "highlight line" | "highlight_line" | "hi-sen" | "hisen" => {
                LayerRole::HighlightLine
            }
            "correction" | "sakkan" | "corrections" => LayerRole::Correction,
            "reference" | "ref" | "lo" | "layout" | "bg" => LayerRole::Reference,
            _ => LayerRole::Custom,
        }
    }
}

fn yes() -> bool {
    true
}

/// Per-layer properties. Kept as ONE struct so `SetCelLayerProps` replaces
/// it wholesale — the exact inverse is trivially the prior struct.
/// PSD-cel-layers: role / chip / locked / onion DEFINE the layer; none of
/// them changes composited pixels, so `content_hash` excludes them (the
/// same exclusion the name has).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct LayerProps {
    /// Display label — free text. Behaviour keys off `role`, never this.
    pub name: String,
    pub visible: bool,
    /// 0..=1, applied when compositing this layer into the cel.
    pub opacity: f32,
    #[serde(default)]
    pub role: LayerRole,
    /// Identity colour override (None = the role's convention colour).
    #[serde(default)]
    pub chip: Option<[u8; 4]>,
    /// Edit-lock: every painting tool refuses on a locked layer.
    #[serde(default)]
    pub locked: bool,
    /// Participates in onion ghosts.
    #[serde(default = "yes")]
    pub onion: bool,
}

impl Default for LayerProps {
    fn default() -> Self {
        Self {
            name: "paint".into(),
            visible: true,
            opacity: 1.0,
            role: LayerRole::Custom,
            chip: None,
            locked: false,
            onion: true,
        }
    }
}

/// One paint layer INSIDE a cel (the anime separation: douga line art, shiage
/// color under it, shadow between, sakkan correction above). Orthogonal to
/// X-sheet columns, which are layers ACROSS TIME. Id-addressed so commands
/// stay exact under any undo/reorder interleaving.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CelLayer {
    pub id: LayerId,
    pub props: LayerProps,
    pub raster: RasterLayer,
}

impl CelLayer {
    /// A fresh layer; its role is inferred from the name.
    pub fn new(id: LayerId, name: impl Into<String>) -> Self {
        let name = name.into();
        Self {
            id,
            props: LayerProps {
                role: LayerRole::infer(&name),
                name,
                ..LayerProps::default()
            },
            raster: RasterLayer::empty(),
        }
    }

    /// A fresh layer with an explicit role.
    pub fn with_role(id: LayerId, name: impl Into<String>, role: LayerRole) -> Self {
        Self {
            id,
            props: LayerProps {
                name: name.into(),
                role,
                ..LayerProps::default()
            },
            raster: RasterLayer::empty(),
        }
    }

    /// Folds the properties that affect the composited PIXELS — visibility,
    /// opacity, and the raster content. The NAME is deliberately excluded so
    /// renaming a layer never invalidates the eval cache or forces a GPU
    /// re-sync.
    pub fn content_hash(&self) -> u64 {
        let mut bytes: Vec<u8> = Vec::with_capacity(16);
        bytes.push(self.props.visible as u8);
        bytes.extend_from_slice(&self.props.opacity.to_bits().to_le_bytes());
        bytes.extend_from_slice(&self.raster.content_hash().to_le_bytes());
        crate::value::fnv1a(&bytes)
    }
}

/// A cel: vector strokes under an ordered stack of raster paint layers.
///
/// `layers[0]` is the BOTTOM of the stack. An empty stack = a vector-only cel
/// (the legacy pre-v5 case); the app keeps raster cels at >= 1 layer by
/// policy, but the ENGINE allows zero (matching the remove-column precedent).
///
/// The store persists drawings field-by-field (strokes as JSON, tiles as
/// BLOBs). GM5: serde only for the session wire, where a cut snapshot
/// (AddCut) carries its drawings — the same payload AddDrawing's layers
/// already carry.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Drawing {
    pub id: DrawingId,
    pub name: String,
    pub strokes: Vec<Stroke>,
    pub layers: Vec<CelLayer>,
}

impl Drawing {
    pub fn new(id: DrawingId, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            strokes: Vec::new(),
            layers: Vec::new(),
        }
    }

    pub fn layer(&self, id: LayerId) -> Option<&CelLayer> {
        self.layers.iter().find(|l| l.id == id)
    }

    pub fn layer_mut(&mut self, id: LayerId) -> Option<&mut CelLayer> {
        self.layers.iter_mut().find(|l| l.id == id)
    }

    pub fn layer_index(&self, id: LayerId) -> Option<usize> {
        self.layers.iter().position(|l| l.id == id)
    }

    /// Stable content hash — folded into the evaluator's recipe so editing
    /// artwork (vector OR raster) invalidates cached values naturally.
    /// Canonical little-endian byte fold; the raster part folds per-layer
    /// hashes IN STACK ORDER (a reorder changes the composite; a rename does
    /// not — see [`CelLayer::content_hash`]), never raw pixels per eval.
    pub fn content_hash(&self) -> u64 {
        let mut bytes: Vec<u8> = Vec::with_capacity(16 + self.strokes.len() * 16);
        for stroke in &self.strokes {
            bytes.extend_from_slice(&stroke.base_width.to_bits().to_le_bytes());
            bytes.extend_from_slice(&stroke.color);
            for p in &stroke.points {
                bytes.extend_from_slice(&p.x.to_bits().to_le_bytes());
                bytes.extend_from_slice(&p.y.to_bits().to_le_bytes());
                bytes.extend_from_slice(&p.pressure.to_bits().to_le_bytes());
                // tilt deliberately excluded — see `StrokePoint::tilt`.
            }
        }
        if !self.layers.is_empty() {
            bytes.extend_from_slice(b"layers");
            for layer in &self.layers {
                bytes.extend_from_slice(&layer.content_hash().to_le_bytes());
            }
        }
        crate::value::fnv1a(&bytes)
    }

    /// CPU composite of the visible layers, bottom -> top, premultiplied-over
    /// with per-layer opacity. Tile values are f16 BIT PATTERNS (the GPU's
    /// Rgba16Float texels — see the encoding law on `TileData`), so the math
    /// decodes to f32, blends linearly, and re-encodes — the same arithmetic
    /// the GPU compositor performs. Plain bytes — the headless law holds.
    ///
    /// This is the GOLDEN REFERENCE the GPU compositor is tested against, and
    /// the future export/eval path. It is NOT the interactive display path.
    pub fn flatten(&self) -> std::collections::BTreeMap<TileCoord, Arc<TileData>> {
        self.flatten_where(&|_| true)
    }

    /// GM11: [`Self::flatten`] as a render purpose sees the cel: Final
    /// leaves out the Reference, Rough and Correction layers, LineTest keeps
    /// the line roles only ([`Purpose::keeps`]). Same arithmetic; View is
    /// exactly `flatten`.
    pub fn flatten_for(
        &self,
        purpose: crate::deliver::Purpose,
    ) -> std::collections::BTreeMap<TileCoord, Arc<TileData>> {
        self.flatten_where(&|role| purpose.keeps(role))
    }

    /// [`Self::flatten`] of only the layers whose role `keep` accepts (GM11's
    /// purposes, GM13's role passes). Same arithmetic.
    pub(crate) fn flatten_where(
        &self,
        keep: &dyn Fn(LayerRole) -> bool,
    ) -> std::collections::BTreeMap<TileCoord, Arc<TileData>> {
        // Collect every coordinate any visible layer touches.
        let mut coords: std::collections::BTreeSet<TileCoord> = std::collections::BTreeSet::new();
        for layer in &self.layers {
            if layer.props.visible && layer.props.opacity > 0.0 && keep(layer.props.role) {
                coords.extend(layer.raster.tiles.keys().copied());
            }
        }

        let mut out = std::collections::BTreeMap::new();
        for coord in coords {
            if let Some(tile) = self.flatten_tile_where(coord, keep) {
                out.insert(coord, Arc::new(tile));
            }
        }
        out
    }

    /// One tile of [`Self::flatten`] (same arithmetic); None where no
    /// visible layer leaves ink.
    pub fn flatten_tile(&self, coord: TileCoord) -> Option<TileData> {
        self.flatten_tile_where(coord, &|_| true)
    }

    fn flatten_tile_where(
        &self,
        coord: TileCoord,
        keep: &dyn Fn(LayerRole) -> bool,
    ) -> Option<TileData> {
        use crate::raster::{TILE_LEN, f16_bits_to_f32, f32_to_f16_bits};

        // f32 working buffer, linear premultiplied.
        let mut acc = vec![0.0f32; TILE_LEN];
        for layer in &self.layers {
            if !layer.props.visible || layer.props.opacity <= 0.0 || !keep(layer.props.role) {
                continue;
            }
            let Some(tile) = layer.raster.tiles.get(&coord) else {
                continue;
            };
            let op = layer.props.opacity.clamp(0.0, 1.0);
            // src-over: out = src*op + out*(1 - src.a*op), premultiplied.
            for (px, spx) in acc.chunks_exact_mut(4).zip(tile.rgba.chunks_exact(4)) {
                let sa = f16_bits_to_f32(spx[3]) * op;
                let keep = 1.0 - sa;
                for c in 0..4 {
                    let s = f16_bits_to_f32(spx[c]) * op;
                    px[c] = s + px[c] * keep;
                }
            }
        }
        let rgba: Vec<u16> = acc
            .iter()
            .map(|v| f32_to_f16_bits(v.clamp(0.0, 1.0)))
            .collect();
        let tile = TileData::from_vec(rgba);
        (!tile.is_empty()).then_some(tile)
    }
}

/// GM6: what a drawing is in the genga → douga hand-off — a Key (原画), a
/// Breakdown or an Inbetween (動画); Unset is a plain drawing. A
/// DEFINITION, never pixels: it lives in `Cut::drawing_defs`, outside
/// `content_hash`.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
pub enum DrawingKind {
    #[default]
    Unset,
    Key,
    Breakdown,
    Inbetween,
}

impl DrawingKind {
    pub const ALL: [DrawingKind; 4] = [
        DrawingKind::Unset,
        DrawingKind::Key,
        DrawingKind::Breakdown,
        DrawingKind::Inbetween,
    ];

    /// Stable persisted key (the drawing_defs JSON). Never rename these.
    pub fn key(self) -> &'static str {
        match self {
            DrawingKind::Unset => "unset",
            DrawingKind::Key => "key",
            DrawingKind::Breakdown => "breakdown",
            DrawingKind::Inbetween => "inbetween",
        }
    }

    pub fn from_key(key: &str) -> Option<DrawingKind> {
        DrawingKind::ALL.into_iter().find(|k| k.key() == key)
    }

    /// The studio term, romaji plus English (D7).
    pub fn label(self) -> &'static str {
        match self {
            DrawingKind::Unset => "unmarked",
            DrawingKind::Key => "genga (key)",
            DrawingKind::Breakdown => "breakdown",
            DrawingKind::Inbetween => "douga (inbetween)",
        }
    }
}

/// GM6: a spacing chart (タイミング指示) on a key, toward the key `to`: the
/// inbetweens' positions between the two keys, each mark in (0, 1) —
/// halves, thirds, or favouring one key.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SpacingChart {
    pub to: DrawingId,
    pub marks: Vec<f32>,
}

impl SpacingChart {
    /// Can the chart be drawn and inbetweened? Every mark finite and
    /// strictly between the keys.
    pub fn is_valid(&self) -> bool {
        self.marks.iter().all(|m| m.is_finite() && *m > 0.0 && *m < 1.0)
    }
}

/// GM6: a drawing's definition — its kind and, on a key, a spacing chart.
/// Lives beside the drawing in `Cut::drawing_defs` (the `drawing_defs`
/// side table, D2), never in `content_hash`; the default is never stored.
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct DrawingDef {
    pub kind: DrawingKind,
    pub chart: Option<SpacingChart>,
    /// GM15: on a storyboard panel, the cut number it belongs to (None =
    /// the panel's order on the BOARD column).
    #[serde(default)]
    pub cut: Option<u32>,
}

impl DrawingDef {
    /// The store's JSON row.
    pub fn to_json(&self) -> String {
        let chart = self.chart.as_ref().map(|c| {
            serde_json::json!({ "to": c.to.0, "marks": c.marks })
        });
        let mut v = serde_json::json!({ "kind": self.kind.key(), "chart": chart });
        if let Some(n) = self.cut {
            v["cut"] = serde_json::json!(n);
        }
        v.to_string()
    }

    /// Read a store row. None = not a JSON object (the caller reports the
    /// file corrupt); an unknown kind (a newer build's) reads as Unset and
    /// a chart that cannot be read is dropped rather than refusing the file.
    pub fn from_json(text: &str) -> Option<DrawingDef> {
        let v: serde_json::Value = serde_json::from_str(text).ok()?;
        let o = v.as_object()?;
        let kind = o
            .get("kind")
            .and_then(|k| k.as_str())
            .and_then(DrawingKind::from_key)
            .unwrap_or_default();
        let chart = o.get("chart").and_then(|c| {
            let to = DrawingId(c.get("to")?.as_u64()?);
            let marks = c
                .get("marks")?
                .as_array()?
                .iter()
                .map(|m| m.as_f64().map(|m| m as f32))
                .collect::<Option<Vec<f32>>>()?;
            let chart = SpacingChart { to, marks };
            chart.is_valid().then_some(chart)
        });
        let cut = o
            .get("cut")
            .and_then(|n| n.as_u64())
            .and_then(|n| u32::try_from(n).ok());
        Some(DrawingDef { kind, chart, cut })
    }
}

/// An image asset owned by a CUT (background plate, reference/storyboard
/// underlay, scanned pencil art — the satsuei "cels over BG" model keeps
/// BGs per cut). The ORIGINAL encoded bytes are the persisted truth (saved
/// verbatim, never re-encoded); premultiplied-f16 tiles are decoded once at
/// construction so every render path speaks the same currency as cel
/// layers. COLOR LAW: file channel values are kept AS-IS (premultiplied but
/// no EOTF applied) — texel space = file space, so importing a PNG and
/// exporting it round-trips bit-clean, consistent with export writing texel
/// values directly as sRGB bytes.
#[derive(Debug, Clone, PartialEq)]
pub struct ImageAsset {
    pub id: ImageId,
    pub name: String,
    pub width: u32,
    pub height: u32,
    /// Encoded source bytes (PNG), persisted verbatim.
    pub bytes: std::sync::Arc<Vec<u8>>,
    /// Decoded premultiplied-f16 tiles anchored at (0,0), derived
    /// deterministically from `bytes`.
    pub tiles: std::collections::BTreeMap<TileCoord, std::sync::Arc<TileData>>,
    /// fnv1a of the encoded bytes — the content address eval recipes fold.
    pub content_hash: u64,
}

/// The audio scratch track (C4): one WAV per cut, frame-0 aligned. Same
/// asset law as ImageAsset — the ORIGINAL encoded bytes are the persisted
/// truth (saved verbatim, never re-encoded); interleaved f32 samples are
/// decoded once at construction. The engine stays headless: hound is a pure
/// decoder, playback lives entirely in the app.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AudioClip {
    pub name: String,
    /// Encoded source bytes (WAV), persisted verbatim.
    pub bytes: std::sync::Arc<Vec<u8>>,
    pub sample_rate: u32,
    pub channels: u16,
    /// Interleaved f32 samples in [-1, 1], derived deterministically from
    /// `bytes`. Arc'd so Cut/Project clones stay cheap.
    pub samples: std::sync::Arc<Vec<f32>>,
}

/// Manual PartialEq: `samples` is a pure function of `bytes`, so comparing
/// it is redundant work (hundreds of MB of floats on a long clip) — and
/// bytes get an Arc pointer fast path clones always hit.
impl PartialEq for AudioClip {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
            && self.sample_rate == other.sample_rate
            && self.channels == other.channels
            && (std::sync::Arc::ptr_eq(&self.bytes, &other.bytes) || self.bytes == other.bytes)
    }
}

/// GM5: on the session wire an image is its id, name and ORIGINAL bytes
/// (the persisted truth); the tiles are re-derived on arrival, exactly as
/// a load does.
impl serde::Serialize for ImageAsset {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        serde::Serialize::serialize(&(self.id, &self.name, self.bytes.as_slice()), s)
    }
}

impl<'de> serde::Deserialize<'de> for ImageAsset {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let (id, name, bytes): (ImageId, String, Vec<u8>) = serde::Deserialize::deserialize(d)?;
        ImageAsset::from_png(id, name, bytes).map_err(serde::de::Error::custom)
    }
}

/// Encoded-size cap for imported audio (a scratch track, not a mix session).
pub const MAX_AUDIO_BYTES: usize = 200 * 1024 * 1024;
/// Decoded-length cap: 30 minutes at 48 kHz stereo — far beyond any cut,
/// enforced BEFORE sample allocation so a crafted header can't drive an OOM.
pub const MAX_AUDIO_SAMPLES: u64 = 48_000 * 60 * 30 * 2;

impl AudioClip {
    /// Decode a WAV into a clip. Int PCM (8/16/24/32-bit) and float WAVs
    /// are normalized to f32 in [-1, 1]; mono and stereo only. Every
    /// failure is an `Err`, never a panic — commands reject atomically and
    /// loads report Corrupt.
    pub fn from_wav(name: impl Into<String>, bytes: Vec<u8>) -> Result<Self, String> {
        let (sample_rate, channels, samples) = Self::wav_samples(&bytes)?;
        Ok(Self {
            name: name.into(),
            bytes: std::sync::Arc::new(bytes),
            sample_rate,
            channels,
            samples: std::sync::Arc::new(samples),
        })
    }

    /// hound's decode of a WAV: (rate, channels, samples).
    fn wav_samples(bytes: &[u8]) -> Result<(u32, u16, Vec<f32>), String> {
        if bytes.len() > MAX_AUDIO_BYTES {
            return Err(format!(
                "wav: too large ({} MB; max {} MB)",
                bytes.len() / (1024 * 1024),
                MAX_AUDIO_BYTES / (1024 * 1024)
            ));
        }
        let mut reader =
            hound::WavReader::new(std::io::Cursor::new(bytes)).map_err(|e| format!("wav: {e}"))?;
        let spec = reader.spec();
        if spec.channels == 0 || spec.channels > 2 {
            return Err(format!(
                "wav: {} channels unsupported (mono or stereo only)",
                spec.channels
            ));
        }
        if spec.sample_rate < 8_000 || spec.sample_rate > 192_000 {
            return Err(format!("wav: sample rate {} Hz out of range", spec.sample_rate));
        }
        // Cap from the HEADER, before allocating sample storage.
        if reader.len() as u64 > MAX_AUDIO_SAMPLES {
            return Err("wav: longer than the 30-minute scratch-track cap".into());
        }
        let samples: Vec<f32> = match (spec.sample_format, spec.bits_per_sample) {
            // Files are a trust boundary: float WAVs can carry NaN/inf or
            // out-of-range values — sanitize at decode (non-finite → 0,
            // clamp to [-1, 1]) so the documented sample contract holds and
            // playback can never blast. Deterministic, so save/load
            // re-decode agrees.
            (hound::SampleFormat::Float, 32) => reader
                .samples::<f32>()
                .map(|s| {
                    s.map(|v| if v.is_finite() { v.clamp(-1.0, 1.0) } else { 0.0 })
                })
                .collect::<Result<_, _>>()
                .map_err(|e| format!("wav: {e}"))?,
            (hound::SampleFormat::Int, bits @ 1..=32) => {
                // Normalize by the format's full scale, not the observed peak.
                let scale = 1.0 / (1u64 << (bits - 1)) as f32;
                reader
                    .samples::<i32>()
                    .map(|s| s.map(|v| v as f32 * scale))
                    .collect::<Result<_, _>>()
                    .map_err(|e| format!("wav: {e}"))?
            }
            (fmt, bits) => return Err(format!("wav: unsupported format {fmt:?}/{bits}-bit")),
        };
        Ok((spec.sample_rate, spec.channels, samples))
    }

    /// GM12: decode any file the sound tracks take — WAV, MP3, OGG Vorbis or
    /// FLAC, told apart by its magic bytes. A WAV takes the `from_wav` path
    /// above (so every v0.3 clip decodes exactly as it always did); an
    /// RF64 reads as the RIFF it would be ([`crate::audio::rf64_as_riff`]);
    /// a WAV hound refuses (A-law, µ-law, 64-bit float) gets symphonia's
    /// reader, and hound's reason stands when both refuse. The rest go
    /// through symphonia ([`crate::audio::decode_shared`], which reads the
    /// clip's own buffer), under the same mono-or-stereo and rate law.
    pub fn from_bytes(name: impl Into<String>, bytes: Vec<u8>) -> Result<Self, String> {
        let name = name.into();
        let bytes = std::sync::Arc::new(bytes);
        let (sample_rate, channels, samples) =
            if matches!(crate::audio::sniff(&bytes), Some(crate::audio::Format::Wav)) {
                let riff = crate::audio::rf64_as_riff(&bytes).map(std::sync::Arc::new);
                let wav = riff.as_ref().unwrap_or(&bytes);
                Self::wav_samples(wav).or_else(|e| Self::decoded(wav, "wav").map_err(|_| e))?
            } else {
                let ext = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
                Self::decoded(&bytes, &ext)?
            };
        Ok(Self {
            name,
            bytes,
            sample_rate,
            channels,
            samples: std::sync::Arc::new(samples),
        })
    }

    /// symphonia's decode of `bytes`, under the clip law.
    fn decoded(bytes: &std::sync::Arc<Vec<u8>>, ext: &str) -> Result<(u32, u16, Vec<f32>), String> {
        let (rate, channels, samples) = crate::audio::decode_shared(std::sync::Arc::clone(bytes), ext)?;
        if channels == 0 || channels > 2 {
            return Err(format!(
                "audio: {channels} channels unsupported (mono or stereo only)"
            ));
        }
        if !(8_000..=192_000).contains(&rate) {
            return Err(format!("audio: sample rate {rate} Hz out of range"));
        }
        Ok((rate, channels, samples))
    }

    /// Clip length in seconds.
    pub fn seconds(&self) -> f32 {
        self.samples.len() as f32 / (self.sample_rate as f32 * self.channels as f32)
    }
}

/// Per-side dimension cap for imported images (BG plates, not gigapixel
/// scans). Enforced BEFORE any pixel allocation so a crafted header can't
/// drive an OOM, and kept small enough that all size arithmetic fits usize.
pub const MAX_IMAGE_DIM: u32 = 16_384;
/// Total pixel cap (~64 Mpx, an 8k×8k plate) — same rationale.
pub const MAX_IMAGE_PIXELS: u64 = 64 * 1024 * 1024;

impl ImageAsset {
    /// Decode a PNG into an asset. EXPAND normalizes palette and sub-8-bit
    /// images to plain 8-bit channels (scanned 1-bit line art, indexed
    /// exports); 16-bit files are rejected as a plain error. Every failure
    /// is an `Err`, never a panic — commands reject atomically and loads
    /// report Corrupt.
    pub fn from_png(id: ImageId, name: impl Into<String>, bytes: Vec<u8>) -> Result<Self, String> {
        use crate::raster::{TILE, TILE_LEN, f32_to_f16_bits};

        let mut decoder = png::Decoder::new(std::io::Cursor::new(&bytes));
        decoder.set_transformations(png::Transformations::EXPAND);
        let mut reader = decoder.read_info().map_err(|e| format!("png: {e}"))?;
        {
            // Caps from the HEADER, before any pixel-sized allocation — a
            // few-hundred-byte file declaring huge dimensions must fail
            // cleanly, not OOM.
            let info = reader.info();
            let (w, h) = (info.width, info.height);
            if w == 0 || h == 0 {
                return Err("png: empty image".into());
            }
            if w > MAX_IMAGE_DIM || h > MAX_IMAGE_DIM
                || (w as u64) * (h as u64) > MAX_IMAGE_PIXELS
            {
                return Err(format!(
                    "png: too large ({w}×{h}; max {MAX_IMAGE_DIM} per side, \
                     {MAX_IMAGE_PIXELS} pixels total)"
                ));
            }
        }
        let mut buf = vec![0u8; reader.output_buffer_size()];
        let info = reader.next_frame(&mut buf).map_err(|e| format!("png: {e}"))?;
        // Depth check BEFORE any slicing: EXPAND leaves 16-bit as-is, and a
        // sub-8-bit buffer would be smaller than the slices below expect.
        if info.bit_depth != png::BitDepth::Eight {
            return Err(format!("png: unsupported bit depth {:?}", info.bit_depth));
        }
        let (w, h) = (info.width, info.height);
        let n = w as usize * h as usize; // fits: capped at 64 Mpx above
        let frame = &buf[..info.buffer_size()];
        let rgba: Vec<u8> = match info.color_type {
            png::ColorType::Rgba => frame[..n * 4].to_vec(),
            png::ColorType::Rgb => frame[..n * 3]
                .chunks_exact(3)
                .flat_map(|p| [p[0], p[1], p[2], 255])
                .collect(),
            png::ColorType::Grayscale => frame[..n]
                .iter()
                .flat_map(|&g| [g, g, g, 255])
                .collect(),
            png::ColorType::GrayscaleAlpha => frame[..n * 2]
                .chunks_exact(2)
                .flat_map(|p| [p[0], p[0], p[0], p[1]])
                .collect(),
            // Indexed cannot appear after EXPAND; anything else is exotic.
            other => return Err(format!("png: unsupported color type {other:?}")),
        };

        // Tile it: straight u8 → premultiplied f16 (no EOTF — see color law).
        let mut tiles = std::collections::BTreeMap::new();
        let t1x = (w as i32 - 1).div_euclid(TILE as i32);
        let t1y = (h as i32 - 1).div_euclid(TILE as i32);
        for ty in 0..=t1y {
            for tx in 0..=t1x {
                let mut px = vec![0u16; TILE_LEN];
                let mut any = false;
                for row in 0..TILE {
                    let sy = ty as i64 * TILE as i64 + row as i64;
                    if sy >= h as i64 {
                        break;
                    }
                    for cx in 0..TILE {
                        let sx = tx as i64 * TILE as i64 + cx as i64;
                        if sx >= w as i64 {
                            break;
                        }
                        let s = &rgba[(sy as usize * w as usize + sx as usize) * 4..][..4];
                        let a = s[3] as f32 / 255.0;
                        if a <= 0.0 {
                            continue;
                        }
                        let i = (row * TILE + cx) * 4;
                        for c in 0..3 {
                            px[i + c] = f32_to_f16_bits(s[c] as f32 / 255.0 * a);
                        }
                        px[i + 3] = f32_to_f16_bits(a);
                        any = true;
                    }
                }
                if any {
                    tiles.insert(
                        (tx, ty),
                        std::sync::Arc::new(TileData::from_vec(px)),
                    );
                }
            }
        }
        let content_hash = crate::value::fnv1a(&bytes);
        Ok(Self {
            id,
            name: name.into(),
            width: w,
            height: h,
            bytes: std::sync::Arc::new(bytes),
            tiles,
            content_hash,
        })
    }
}

/// GM5: serde so a whole cut rides the session wire inside AddCut (the
/// inverse of RemoveCut carries the snapshot); the disk format is still
/// store.rs, field by field.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Cut {
    pub id: CutId,
    pub name: String,
    pub frame_count: u32,
    pub drawings: Vec<Drawing>,
    pub xsheet: XSheet,
    pub graph: Graph,
    /// Image assets owned by this cut (BG plates, references).
    pub images: Vec<ImageAsset>,
    /// The scratch audio track (C4): at most ONE clip per cut, aligned to
    /// frame 0. Timing reference only — audio never enters the render graph.
    /// GM12: this is the FIRST sound track (dialogue); its settings live in
    /// `audio_props`, the tracks after it in `tracks`.
    pub audio: Option<AudioClip>,
    /// GM12: the first track's settings (name, kind, offset, gain, mute,
    /// solo). The default is v0.3's: dialogue from frame 0, as recorded.
    #[serde(default)]
    pub audio_props: crate::audio::TrackProps,
    /// GM12: the sound tracks after the first (SE, music, ...), in order.
    /// Their offsets are frame-indexed, so frame ripples move them (D4).
    #[serde(default)]
    pub tracks: Vec<crate::audio::AudioTrack>,
    /// GM5 (D3): namespaced keys (`camera.paper`, `prod.status`, ...) whose
    /// values are JSON owned by the part that defines the key. Edited only
    /// through SetCutMeta; unknown keys round-trip untouched.
    pub meta: std::collections::BTreeMap<String, String>,
    /// GM5: the memo/dialogue row — text per frame (セリフ, action notes).
    /// Frame-indexed, so InsertFrames/DeleteFrames ripple it (D4).
    pub memos: std::collections::BTreeMap<u32, String>,
    /// GM5: each column's definition (kind, hidden, locked). Only
    /// non-default definitions are stored: a missing entry is a plain cel
    /// column, so legacy cuts are all Cel.
    pub column_defs: std::collections::BTreeMap<ColumnId, crate::xsheet::ColumnDef>,
    /// GM6: each drawing's definition (kind, spacing chart). Only
    /// non-default definitions are stored, so an unmarked drawing has no
    /// entry; never in `content_hash` (D1).
    #[serde(default)]
    pub drawing_defs: std::collections::BTreeMap<DrawingId, DrawingDef>,
    /// GM13 (D15): files a cut renders from, embedded so the cut renders on
    /// its own — a .cube LUT's text, by key (`lut.<name>`). Saved in the
    /// `cut_blobs` side table; edited only through SetCutBlob.
    #[serde(default)]
    pub blobs: std::collections::BTreeMap<String, Arc<Vec<u8>>>,
}

impl Cut {
    /// GM5: an empty cut (no drawings, columns or graph).
    pub fn new(id: CutId, name: impl Into<String>, frame_count: u32) -> Self {
        Self {
            id,
            name: name.into(),
            frame_count,
            drawings: Vec::new(),
            xsheet: XSheet::default(),
            graph: Graph::default(),
            images: Vec::new(),
            audio: None,
            audio_props: Default::default(),
            tracks: Vec::new(),
            meta: Default::default(),
            memos: Default::default(),
            column_defs: Default::default(),
            drawing_defs: Default::default(),
            blobs: Default::default(),
        }
    }

    /// GM6: a drawing's definition (the default for an unmarked one).
    pub fn drawing_def(&self, id: DrawingId) -> DrawingDef {
        self.drawing_defs.get(&id).cloned().unwrap_or_default()
    }

    /// GM6: a drawing's kind (Unset when unmarked).
    pub fn drawing_kind(&self, id: DrawingId) -> DrawingKind {
        self.drawing_defs.get(&id).map_or(DrawingKind::Unset, |d| d.kind)
    }

    /// GM6: set a drawing's definition, keeping the map canonical (the
    /// default is never stored), so undo restores structural equality.
    pub fn set_drawing_def(&mut self, id: DrawingId, def: DrawingDef) {
        if def == DrawingDef::default() {
            self.drawing_defs.remove(&id);
        } else {
            self.drawing_defs.insert(id, def);
        }
    }

    /// GM5: a column's definition (the default for a plain cel column).
    pub fn column_def(&self, id: ColumnId) -> crate::xsheet::ColumnDef {
        self.column_defs.get(&id).copied().unwrap_or_default()
    }

    /// GM5: set a column's definition, keeping the map canonical (the
    /// default is never stored), so undo restores structural equality.
    pub fn set_column_def(&mut self, id: ColumnId, def: crate::xsheet::ColumnDef) {
        if def == crate::xsheet::ColumnDef::default() {
            self.column_defs.remove(&id);
        } else {
            self.column_defs.insert(id, def);
        }
    }

    /// GM5: does this column reach the composite (not hidden, not Ref)?
    /// A missing column renders nothing either way.
    pub fn column_renders(&self, id: ColumnId) -> bool {
        self.column_def(id).renders()
    }

    /// GM5: the column path's order, bottom → top: BG, then Cel, then Book,
    /// each band in sheet order; hidden and Ref columns are skipped. Every
    /// render path (export, eval, the canvas, graphcomp) goes through this.
    pub fn render_order(&self) -> Vec<ColumnId> {
        let mut order: Vec<(u8, usize, ColumnId)> = self
            .xsheet
            .columns
            .iter()
            .enumerate()
            .filter(|(_, c)| self.column_renders(c.id))
            .map(|(i, c)| (self.column_def(c.id).kind.band(), i, c.id))
            .collect();
        order.sort();
        order.into_iter().map(|(_, _, id)| id).collect()
    }

    /// GM5: the render order split around `active`: the columns drawn under
    /// it and those drawn over it. An active column that does not render
    /// (hidden, Ref) still splits where its band puts it.
    pub fn render_split(&self, active: ColumnId) -> (Vec<ColumnId>, Vec<ColumnId>) {
        let rank = |id: ColumnId| {
            let i = self.xsheet.columns.iter().position(|c| c.id == id);
            (self.column_def(id).kind.band(), i.unwrap_or(usize::MAX))
        };
        let me = rank(active);
        let (below, above): (Vec<ColumnId>, Vec<ColumnId>) = self
            .render_order()
            .into_iter()
            .filter(|&id| id != active)
            .partition(|&id| rank(id) < me);
        (below, above)
    }

    pub fn drawing(&self, id: DrawingId) -> Option<&Drawing> {
        self.drawings.iter().find(|d| d.id == id)
    }

    pub fn image(&self, id: ImageId) -> Option<&ImageAsset> {
        self.images.iter().find(|i| i.id == id)
    }

    /// A parameter column's value at `frame` (None = column missing or has
    /// no keys — the caller falls back to its static value).
    pub fn param_value(&self, id: ParamId, frame: u32) -> Option<f32> {
        self.xsheet.param(id).and_then(|p| p.resolve(frame))
    }

    /// A Transform node's EFFECTIVE params at `frame`: each component takes
    /// its bound parameter column's value when bound and resolvable, else
    /// the static value. The single source of truth for all three render
    /// paths (evaluator hash, CPU export, GPU compositor) — they must agree
    /// or the composite view would lie about the export.
    pub fn transform_at(
        &self,
        translate: (f32, f32),
        scale: f32,
        rotate_deg: f32,
        binds: &crate::graph::TransformBinds,
        frame: u32,
    ) -> ((f32, f32), f32, f32) {
        let get = |bind: Option<ParamId>, fallback: f32| {
            bind.and_then(|id| self.param_value(id, frame)).unwrap_or(fallback)
        };
        (
            (get(binds.tx, translate.0), get(binds.ty, translate.1)),
            get(binds.scale, scale),
            get(binds.rotate, rotate_deg),
        )
    }

    /// GM9: an Effect node's EFFECTIVE parameters at `frame` — the
    /// transform_at of FX, read by all three render paths alike.
    pub fn fx_at(
        &self,
        fx: crate::fx::FxKind,
        params: &crate::fx::FxParams,
        binds: &crate::fx::FxBinds,
        frame: u32,
    ) -> crate::fx::FxValues {
        crate::fx::FxValues::resolve(fx, params, binds, |id| self.param_value(id, frame))
    }

    /// GM16: a Generator node's EFFECTIVE parameters at `frame` (fx_at's
    /// law), read by every render path alike.
    pub fn gen_at(
        &self,
        generator: crate::generator::GenKind,
        params: &crate::fx::FxParams,
        binds: &crate::fx::FxBinds,
        frame: u32,
    ) -> crate::generator::GenValues {
        crate::generator::GenValues::resolve(generator, params, binds, |id| self.param_value(id, frame))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Scene {
    pub id: SceneId,
    pub name: String,
    pub cuts: Vec<Cut>,
}

/// GM8: the project-level settings one `SetProjectProps` swaps (the colour
/// law is not here — it changes only through the sRGB conversion).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ProjectProps {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub dpi: f32,
    pub default_cut_frames: Option<u32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Project {
    pub name: String,
    pub fps: u32,
    /// Frame resolution in pixels — the "paper" every cut is drawn against.
    /// Project-wide (a film has one resolution), set at project creation.
    pub width: u32,
    pub height: u32,
    /// Pixels per inch, for print/export scaling. Purely metadata for drawing.
    pub dpi: f32,
    pub scenes: Vec<Scene>,
    /// Monotonic id counter; all entity ids come from here.
    pub next_id: u64,
    /// Opaque, app-owned project data (character color palettes, and
    /// anything similar later — e.g. per-project layout) that the ENGINE
    /// never interprets, only round-trips: string keys/values persisted
    /// verbatim under the `app.` prefix in the store's free-form `meta`
    /// table. No UI/app types leak into anim-core this way, and no schema
    /// version bump is needed to add a new key (the table already accepts
    /// arbitrary rows) — see store.rs's save/load for the exact convention.
    pub app_meta: std::collections::BTreeMap<String, String>,
    /// How chip colours are stored (D8, `meta.color_law`). Legacy for
    /// every file that predates the key, so old projects render as they
    /// always did; the New Project dialog makes Srgb projects.
    pub color_law: crate::color::ColorLaw,
    /// GM8 (D13, `meta.default_cut_frames`): the length new cuts get.
    /// Unset means 48; the templates set 72.
    pub default_cut_frames: Option<u32>,
}

impl Project {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            fps: 24, // anime standard
            width: 1920,
            height: 1080,
            dpi: 300.0,
            scenes: Vec::new(),
            next_id: 1,
            app_meta: std::collections::BTreeMap::new(),
            color_law: crate::color::ColorLaw::Legacy,
            default_cut_frames: None,
        }
    }

    /// The length a new cut gets (D13: 48 when the project never set one).
    pub fn cut_frames(&self) -> u32 {
        self.default_cut_frames.unwrap_or(48)
    }

    /// The project-level settings the Project Settings window edits.
    pub fn props(&self) -> ProjectProps {
        ProjectProps {
            name: self.name.clone(),
            width: self.width,
            height: self.height,
            fps: self.fps,
            dpi: self.dpi,
            default_cut_frames: self.default_cut_frames,
        }
    }

    pub fn alloc_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    pub fn scene(&self, id: SceneId) -> Option<&Scene> {
        self.scenes.iter().find(|s| s.id == id)
    }

    pub fn scene_mut(&mut self, id: SceneId) -> Option<&mut Scene> {
        self.scenes.iter_mut().find(|s| s.id == id)
    }

    pub fn cut(&self, scene: SceneId, cut: CutId) -> Option<&Cut> {
        self.scene(scene)?.cuts.iter().find(|c| c.id == cut)
    }

    pub fn cut_mut(&mut self, scene: SceneId, cut: CutId) -> Option<&mut Cut> {
        self.scene_mut(scene)?.cuts.iter_mut().find(|c| c.id == cut)
    }
}
