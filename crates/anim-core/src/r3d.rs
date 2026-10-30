//! GM17: 3D layout and toon render — the 3D artist's bridge into the cut.
//! Models (glTF, OBJ) are embedded in the cut (`cut_models`, D15) and drawn
//! once, here, on the CPU: a deterministic rasteriser with an f32 z-buffer,
//! stepped toon shading, outlines and passes. The CPU golden renderer's
//! Model3D arm calls [`render`] and the GPU compositor shows its upload
//! (D9, NEVER-DO 2). Also here: the View3D pane's navigation maths
//! ([`Orbit`]) and the reading of image sequences rendered by other apps.
//!
//! Space: right-handed, Y up (glTF's), units as the file's (metres for
//! glTF). The front view looks from +Z toward −Z, the right view from +X,
//! the top view down from +Y (screen up is then −Z).
//!
//! Laws (pinned by tests/r3d_tests.rs — change them together):
//! - **Camera.** A 36 mm film back across the frame's width: the horizontal
//!   angle of view is `2·atan(18 / focal)` (50 mm → 39.6°); the vertical
//!   follows the frame's aspect. The target projects to the frame's centre.
//!   Roll turns the camera about its view axis.
//! - **Rasterising.** Pixel centres at (x + ½, y + ½); edge functions with
//!   the top-left rule (a centre on a shared edge belongs to one triangle);
//!   triangles clipped at the near plane; depth (distance along the view
//!   axis) interpolated as 1/z, perspective-correct; the nearer surface wins
//!   (ties keep the first drawn), nothing past the far plane. Both sides of
//!   a face render (models are often open); its normal turns toward the
//!   camera. Triangles draw in file order: renders are deterministic.
//! - **Toon.** Half-Lambert `t = (n·l + 1)/2` cut into `steps` even bands;
//!   band 0 is the base colour times the shadow chip, the top band the base
//!   colour, the bands between mixed evenly; `n·l` at or above the highlight
//!   threshold (1 = none) lifts the colour halfway to white. Each band is one
//!   exact chip, entering through the colour law like paint (D8).
//! - **Outline.** An edge pixel is a covered pixel beside empty space or a
//!   farther object (silhouette), beside its own surface more than 10% of
//!   its depth farther (a fold), or across a crease sharper than the crease
//!   angle (its right or lower neighbour). The width rounds to whole px (at
//!   least one); the line is every pixel within `(width − 1)/2` px of the
//!   edge — of an edge pixel's centre for an odd width, of the crack
//!   between it and the neighbour that made it an edge for an even one —
//!   so it is `width` px across either way; width 0 draws none.
//! - **Passes.** Beauty (toon + outline), Lines (the outline alone), Shadow
//!   matte (white where any band below the lit one shows), Depth (grey
//!   `(d − near)/(far − near)`, so it grows with distance), ID matte (one
//!   colour per object; each mesh of a model is an object).

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use crate::color::ColorLaw;
use crate::command::{Command, CutRef};
use crate::fx::{self, FxBinds, FxParams, FxValue, Image, ParamKind, ParamSpec, choice, color, float};
use crate::graph::NodeKind;
use crate::ids::{NodeId, ParamId};
use crate::model::{Cut, ModelAsset};
use crate::xsheet::{ParamColumn, ParamInterp, ParamKey};

// ---- Vectors -----------------------------------------------------------------

pub type V3 = [f32; 3];

pub fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

pub fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

pub fn mul(a: V3, s: f32) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}

pub fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

pub fn cross(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

pub fn length(a: V3) -> f32 {
    dot(a, a).sqrt()
}

pub fn normalize(a: V3) -> V3 {
    let l = length(a);
    if l > 0.0 { mul(a, 1.0 / l) } else { a }
}

// ---- Models --------------------------------------------------------------------

/// A face with no material: a light neutral grey chip.
pub const DEFAULT_COLOR: [u8; 4] = [204, 204, 204, 255];

/// One object: triangles over its vertices, each with its material's base
/// colour as a chip (straight sRGB bytes).
#[derive(Debug, Clone, PartialEq)]
pub struct Mesh {
    pub name: String,
    pub positions: Vec<V3>,
    pub triangles: Vec<[u32; 3]>,
    pub colors: Vec<[u8; 4]>,
}

/// A model file's meshes, in the file's order, laid out by its node
/// transforms.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Model {
    pub meshes: Vec<Mesh>,
}

impl Model {
    pub fn triangle_count(&self) -> usize {
        self.meshes.iter().map(|m| m.triangles.len()).sum()
    }

    /// The box around every vertex a triangle uses (None when empty).
    pub fn bbox(&self) -> Option<(V3, V3)> {
        let mut out: Option<(V3, V3)> = None;
        for m in &self.meshes {
            for t in &m.triangles {
                for &i in t {
                    let p = m.positions[i as usize];
                    let (lo, hi) = out.get_or_insert((p, p));
                    for k in 0..3 {
                        lo[k] = lo[k].min(p[k]);
                        hi[k] = hi[k].max(p[k]);
                    }
                }
            }
        }
        out
    }
}

/// The formats File ▸ Import ▸ 3D model takes, by extension.
pub const FORMATS: &[&str] = &["glb", "gltf", "obj"];

/// A model file's format from its extension (any case), if supported.
pub fn format_for_extension(ext: &str) -> Option<&'static str> {
    let ext = ext.to_ascii_lowercase();
    FORMATS.iter().copied().find(|f| *f == ext)
}

/// Read a model's meshes from its file's bytes.
pub fn load(format: &str, bytes: &[u8]) -> Result<Model, String> {
    match format {
        "glb" | "gltf" => load_gltf(bytes),
        "obj" => load_obj(bytes),
        other => Err(format!("unknown model format '{other}'")),
    }
}

/// A column-major 4×4 matrix (glTF's layout: `m[column][row]`).
type M4 = [[f32; 4]; 4];

const M4_IDENTITY: M4 = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
    [0.0, 0.0, 0.0, 1.0],
];

fn m4_mul(a: &M4, b: &M4) -> M4 {
    let mut r = [[0.0f32; 4]; 4];
    for (c, col) in r.iter_mut().enumerate() {
        for (row, v) in col.iter_mut().enumerate() {
            *v = (0..4).map(|k| a[k][row] * b[c][k]).sum();
        }
    }
    r
}

fn m4_point(m: &M4, p: V3) -> V3 {
    [
        m[0][0] * p[0] + m[1][0] * p[1] + m[2][0] * p[2] + m[3][0],
        m[0][1] * p[0] + m[1][1] * p[1] + m[2][1] * p[2] + m[3][1],
        m[0][2] * p[0] + m[1][2] * p[1] + m[2][2] * p[2] + m[3][2],
    ]
}

/// A glTF base colour factor (linear) as a chip.
fn linear_chip(f: [f32; 4]) -> [u8; 4] {
    let l = |v: f32| crate::export::linear_to_srgb(v.clamp(0.0, 1.0));
    [l(f[0]), l(f[1]), l(f[2]), (f[3].clamp(0.0, 1.0) * 255.0).round() as u8]
}

/// glTF 2.0, binary (.glb) or JSON (.gltf) with its buffers embedded as
/// data URIs: every triangle primitive of the default scene's meshes, placed
/// by its nodes' transforms, with its material's base colour factor. A
/// buffer kept in a file beside it cannot be read from the bytes alone.
pub fn load_gltf(bytes: &[u8]) -> Result<Model, String> {
    let gltf = gltf::Gltf::from_slice(bytes).map_err(|e| format!("not a glTF file ({e})"))?;
    let buffers = gltf::import_buffers(&gltf.document, None, gltf.blob.clone()).map_err(|e| {
        format!("its buffers do not read ({e}); save it as .glb, or with its buffers embedded")
    })?;
    let doc = &gltf.document;
    let mut model = Model::default();
    match doc.default_scene().or_else(|| doc.scenes().next()) {
        Some(scene) => gltf_nodes(scene.nodes(), &buffers, &mut model),
        None => {
            for mesh in doc.meshes() {
                let name = mesh.name().unwrap_or("mesh").to_string();
                gltf_mesh(&mesh, &M4_IDENTITY, name, &buffers, &mut model);
            }
        }
    }
    if model.triangle_count() == 0 {
        return Err("it holds no triangles".into());
    }
    Ok(model)
}

/// Every node under `roots`, depth first in file order, each placed by its
/// parents' transforms. The reader checks only that `children` name nodes,
/// not that they form a tree, so a node listed under itself or an ancestor
/// (a malformed file) is entered once; and the walk keeps its own stack, so
/// no depth of nesting can overflow the call stack.
fn gltf_nodes<'a>(
    roots: impl Iterator<Item = gltf::Node<'a>>,
    buffers: &[gltf::buffer::Data],
    model: &mut Model,
) {
    let mut entered = std::collections::HashSet::new();
    let mut stack: Vec<(gltf::Node<'a>, M4)> = roots.map(|n| (n, M4_IDENTITY)).collect();
    stack.reverse();
    while let Some((node, parent)) = stack.pop() {
        if !entered.insert(node.index()) {
            continue;
        }
        let m = m4_mul(&parent, &node.transform().matrix());
        if let Some(mesh) = node.mesh() {
            let name = node.name().or(mesh.name()).unwrap_or("mesh").to_string();
            gltf_mesh(&mesh, &m, name, buffers, model);
        }
        let children: Vec<gltf::Node<'a>> = node.children().collect();
        stack.extend(children.into_iter().rev().map(|c| (c, m)));
    }
}

fn gltf_mesh(mesh: &gltf::Mesh, m: &M4, name: String, buffers: &[gltf::buffer::Data], model: &mut Model) {
    let mut out = Mesh {
        name,
        positions: Vec::new(),
        triangles: Vec::new(),
        colors: Vec::new(),
    };
    for prim in mesh.primitives() {
        if prim.mode() != gltf::mesh::Mode::Triangles {
            continue;
        }
        let reader = prim.reader(|b| buffers.get(b.index()).map(|d| d.0.as_slice()));
        let Some(positions) = reader.read_positions() else {
            continue;
        };
        let base = out.positions.len() as u32;
        out.positions.extend(positions.map(|p| m4_point(m, p)));
        let n = out.positions.len() as u32 - base;
        let indices: Vec<u32> = match reader.read_indices() {
            Some(i) => i.into_u32().collect(),
            None => (0..n).collect(),
        };
        let chip = linear_chip(prim.material().pbr_metallic_roughness().base_color_factor());
        for t in indices.chunks_exact(3) {
            if t.iter().all(|&i| i < n) {
                out.triangles.push([base + t[0], base + t[1], base + t[2]]);
                out.colors.push(chip);
            }
        }
    }
    if !out.triangles.is_empty() {
        model.meshes.push(out);
    }
}

/// An OBJ carries its material library inside itself as comment lines with
/// this prefix (written by [`embed_mtl`] at import), so the embedded bytes
/// stay one valid OBJ file and still know their colours.
pub const EMBEDDED_MTL: &str = "#mtl ";

/// `obj` with the .mtl text `mtl` appended as [`EMBEDDED_MTL`] comments.
pub fn embed_mtl(obj: &[u8], mtl: &str) -> Vec<u8> {
    let mut out = obj.to_vec();
    if !out.ends_with(b"\n") {
        out.push(b'\n');
    }
    out.extend_from_slice(b"# gaban studio: the material library, embedded\n");
    for line in mtl.lines() {
        out.extend_from_slice(EMBEDDED_MTL.as_bytes());
        out.extend_from_slice(line.as_bytes());
        out.push(b'\n');
    }
    out
}

/// Wavefront OBJ: every object's faces triangulated, each with its
/// material's diffuse colour when the library is embedded, else the
/// default grey.
pub fn load_obj(bytes: &[u8]) -> Result<Model, String> {
    let text = std::str::from_utf8(bytes).map_err(|_| "it is not OBJ text".to_string())?;
    let mtl: String = text
        .lines()
        .filter_map(|l| l.strip_prefix(EMBEDDED_MTL))
        .collect::<Vec<_>>()
        .join("\n");
    let opts = tobj::LoadOptions {
        triangulate: true,
        single_index: true,
        ..Default::default()
    };
    // The reader asks for a library only where the file names one: an
    // embedded library with no `mtllib` line is named here, in memory.
    let named = text.lines().any(|l| l.trim_start().starts_with("mtllib"));
    let source = if !mtl.is_empty() && !named {
        format!("mtllib embedded.mtl\n{text}")
    } else {
        text.to_string()
    };
    let (models, materials) = tobj::load_obj_buf(&mut std::io::Cursor::new(source.as_bytes()), &opts, |_| {
        if mtl.is_empty() {
            Ok((Vec::new(), HashMap::new()))
        } else {
            tobj::load_mtl_buf(&mut std::io::Cursor::new(mtl.as_bytes()))
        }
    })
    .map_err(|e| format!("the OBJ does not read ({e})"))?;
    let materials = materials.unwrap_or_default();
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    let mut model = Model::default();
    for m in models {
        let mesh = &m.mesh;
        let chip = mesh
            .material_id
            .and_then(|i| materials.get(i))
            .and_then(|mat| mat.diffuse)
            .map(|d| [byte(d[0]), byte(d[1]), byte(d[2]), 255])
            .unwrap_or(DEFAULT_COLOR);
        let positions: Vec<V3> = mesh.positions.chunks_exact(3).map(|p| [p[0], p[1], p[2]]).collect();
        let n = positions.len() as u32;
        let triangles: Vec<[u32; 3]> = mesh
            .indices
            .chunks_exact(3)
            .filter(|t| t.iter().all(|&i| i < n))
            .map(|t| [t[0], t[1], t[2]])
            .collect();
        if triangles.is_empty() {
            continue;
        }
        let colors = vec![chip; triangles.len()];
        model.meshes.push(Mesh {
            name: m.name.clone(),
            positions,
            triangles,
            colors,
        });
    }
    if model.triangle_count() == 0 {
        return Err("the OBJ holds no faces".into());
    }
    Ok(model)
}

/// An embedded model's meshes, read once per content (renders read their
/// model every frame).
pub fn model_of(asset: &ModelAsset) -> Result<Arc<Model>, String> {
    type Cache = Mutex<HashMap<(u64, usize, String), Arc<Model>>>;
    static CACHE: OnceLock<Cache> = OnceLock::new();
    let key = (asset.content_hash, asset.bytes.len(), asset.format.clone());
    let cache = CACHE.get_or_init(Default::default);
    if let Some(m) = cache.lock().ok().and_then(|c| c.get(&key).cloned()) {
        return Ok(m);
    }
    let m = Arc::new(load(&asset.format, &asset.bytes)?);
    if let Ok(mut c) = cache.lock() {
        if c.len() >= 16 {
            c.clear();
        }
        c.insert(key, m.clone());
    }
    Ok(m)
}

// ---- Camera ----------------------------------------------------------------------

/// The film back's width in mm (full frame 35 mm).
pub const FILM_BACK_MM: f32 = 36.0;

/// The horizontal angle of view (radians) of a lens on the 36 mm back.
pub fn focal_to_fov(focal_mm: f32) -> f32 {
    2.0 * (FILM_BACK_MM * 0.5 / focal_mm.max(1e-3)).atan()
}

/// A camera's frame: unit right, up and forward (the view direction).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Basis {
    pub right: V3,
    pub up: V3,
    pub forward: V3,
}

/// The basis looking from `eye` at `target`, Y up, turned `roll_deg` about
/// the view axis (positive turns the camera clockwise as it looks, so the
/// picture turns the other way). Looking straight down, screen up is −Z;
/// straight up, +Z.
pub fn look_at(eye: V3, target: V3, roll_deg: f32) -> Basis {
    let f = normalize(sub(target, eye));
    let mut r = cross(f, [0.0, 1.0, 0.0]);
    if length(r) < 1e-4 {
        r = cross(f, if f[1] < 0.0 { [0.0, 0.0, -1.0] } else { [0.0, 0.0, 1.0] });
    }
    let r = normalize(r);
    let u = cross(r, f);
    let (s, c) = roll_deg.to_radians().sin_cos();
    Basis {
        right: add(mul(r, c), mul(u, -s)),
        up: add(mul(u, c), mul(r, s)),
        forward: f,
    }
}

/// A camera: where it stands, what it looks at, its roll, its lens on the
/// 36 mm back, and its near and far planes. `ortho` (the View3D pane's
/// numpad 5) frames what the lens frames at the target, without
/// perspective.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera3 {
    pub eye: V3,
    pub target: V3,
    pub roll_deg: f32,
    pub focal_mm: f32,
    pub near: f32,
    pub far: f32,
    pub ortho: bool,
}

impl Camera3 {
    pub fn basis(&self) -> Basis {
        look_at(self.eye, self.target, self.roll_deg)
    }

    /// The tangents of half the horizontal and vertical angles of view in
    /// a `w × h` frame.
    pub fn perspective(&self, w: u32, h: u32) -> (f32, f32) {
        let tx = (focal_to_fov(self.focal_mm) * 0.5).tan();
        (tx, tx * h.max(1) as f32 / w.max(1) as f32)
    }

    /// A world point in camera space: x right, y up, z its depth in front.
    pub fn to_view(&self, b: &Basis, p: V3) -> V3 {
        let d = sub(p, self.eye);
        [dot(d, b.right), dot(d, b.up), dot(d, b.forward)]
    }

    /// Where a camera-space point lands in a `w × h` frame (px, y down).
    pub fn screen(&self, v: V3, w: u32, h: u32) -> (f32, f32) {
        let (tx, ty) = self.perspective(w, h);
        let (nx, ny) = if self.ortho {
            let d = length(sub(self.target, self.eye)).max(1e-6);
            (v[0] / (d * tx), v[1] / (d * ty))
        } else {
            (v[0] / (v[2] * tx), v[1] / (v[2] * ty))
        };
        ((nx + 1.0) * 0.5 * w as f32, (1.0 - ny) * 0.5 * h as f32)
    }

    /// Where a world point lands in a `w × h` frame, and its depth; None in
    /// front of the near plane.
    pub fn project(&self, p: V3, w: u32, h: u32) -> Option<(f32, f32, f32)> {
        let v = self.to_view(&self.basis(), p);
        if v[2] < self.near {
            return None;
        }
        let (x, y) = self.screen(v, w, h);
        Some((x, y, v[2]))
    }
}

// ---- Scenes and the rasteriser -----------------------------------------------------

/// An object's placement: scale, then rotate X, Y, Z (degrees, Blender's
/// XYZ Euler), then translate.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Xform {
    pub translate: V3,
    pub rotate_deg: V3,
    pub scale: f32,
}

impl Xform {
    pub const IDENTITY: Xform = Xform {
        translate: [0.0; 3],
        rotate_deg: [0.0; 3],
        scale: 1.0,
    };

    pub fn apply(&self, p: V3) -> V3 {
        let mut q = mul(p, self.scale);
        let (sx, cx) = self.rotate_deg[0].to_radians().sin_cos();
        q = [q[0], q[1] * cx - q[2] * sx, q[1] * sx + q[2] * cx];
        let (sy, cy) = self.rotate_deg[1].to_radians().sin_cos();
        q = [q[0] * cy + q[2] * sy, q[1], -q[0] * sy + q[2] * cy];
        let (sz, cz) = self.rotate_deg[2].to_radians().sin_cos();
        q = [q[0] * cz - q[1] * sz, q[0] * sz + q[1] * cz, q[2]];
        add(q, self.translate)
    }
}

/// One triangle of a scene, in world space, with its chip and object id.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tri {
    pub p: [V3; 3],
    pub color: [u8; 4],
    pub id: u32,
}

/// What the rasteriser draws: triangles in order.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Scene3 {
    pub tris: Vec<Tri>,
}

impl Scene3 {
    /// `model` placed by `xf`, its meshes numbered as objects from
    /// `first_id` (0 is reserved for empty space).
    pub fn add_model(&mut self, model: &Model, xf: &Xform, first_id: u32) {
        for (k, m) in model.meshes.iter().enumerate() {
            let id = first_id + k as u32;
            let world: Vec<V3> = m.positions.iter().map(|p| xf.apply(*p)).collect();
            for (t, color) in m.triangles.iter().zip(&m.colors) {
                self.tris.push(Tri {
                    p: t.map(|i| world[i as usize]),
                    color: *color,
                    id,
                });
            }
        }
    }
}

/// The rasteriser's per-pixel surface: the nearest surface's base chip,
/// its depth along the view axis, its face normal (world space, toward the
/// camera) and its object id. Empty pixels hold `[0; 4]`, infinity, zero
/// and 0.
#[derive(Debug, Clone, PartialEq)]
pub struct Buffers {
    pub w: u32,
    pub h: u32,
    pub color: Vec<[u8; 4]>,
    pub depth: Vec<f32>,
    pub normal: Vec<V3>,
    pub id: Vec<u32>,
}

impl Buffers {
    /// How many pixels a surface covers.
    pub fn covered(&self) -> usize {
        self.id.iter().filter(|&&i| i != 0).count()
    }
}

fn edge(a: (f32, f32), b: (f32, f32), p: (f32, f32)) -> f32 {
    (b.0 - a.0) * (p.1 - a.1) - (b.1 - a.1) * (p.0 - a.0)
}

/// The top-left rule for a positively wound triangle in y-down px: a top
/// edge runs exactly rightward, a left edge runs upward.
fn top_left(a: (f32, f32), b: (f32, f32)) -> bool {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    (dy == 0.0 && dx > 0.0) || dy < 0.0
}

/// A camera-space triangle clipped to `z ≥ near` (Sutherland–Hodgman): 0,
/// 3 or 4 corners.
fn clip_near(v: &[V3; 3], near: f32) -> Vec<V3> {
    let mut out = Vec::with_capacity(4);
    for i in 0..3 {
        let (a, c) = (v[i], v[(i + 1) % 3]);
        let (a_in, c_in) = (a[2] >= near, c[2] >= near);
        if a_in {
            out.push(a);
        }
        if a_in != c_in {
            let t = (near - a[2]) / (c[2] - a[2]);
            out.push([a[0] + (c[0] - a[0]) * t, a[1] + (c[1] - a[1]) * t, near]);
        }
    }
    out
}

/// Draw `scene` through `cam` into a `w × h` frame's buffers.
pub fn rasterize(scene: &Scene3, cam: &Camera3, w: u32, h: u32) -> Buffers {
    let n = w as usize * h as usize;
    let mut b = Buffers {
        w,
        h,
        color: vec![[0; 4]; n],
        depth: vec![f32::INFINITY; n],
        normal: vec![[0.0; 3]; n],
        id: vec![0; n],
    };
    if n == 0 {
        return b;
    }
    let basis = cam.basis();
    let near = cam.near.max(1e-6);
    let far = cam.far.max(near);
    for tri in &scene.tris {
        let face = cross(sub(tri.p[1], tri.p[0]), sub(tri.p[2], tri.p[0]));
        if !(length(face) > 0.0) {
            continue;
        }
        let mut nrm = normalize(face);
        let toward = if cam.ortho {
            mul(basis.forward, -1.0)
        } else {
            sub(cam.eye, tri.p[0])
        };
        if dot(nrm, toward) < 0.0 {
            nrm = mul(nrm, -1.0);
        }
        let view = tri.p.map(|p| cam.to_view(&basis, p));
        let poly = clip_near(&view, near);
        if poly.len() < 3 {
            continue;
        }
        let pts: Vec<(f32, f32, f32)> = poly
            .iter()
            .map(|v| {
                let (x, y) = cam.screen(*v, w, h);
                (x, y, v[2])
            })
            .collect();
        for i in 1..pts.len() - 1 {
            let surface = Surface {
                color: tri.color,
                normal: nrm,
                id: tri.id,
            };
            raster_tri(&mut b, [pts[0], pts[i], pts[i + 1]], cam.ortho, far, &surface);
        }
    }
    b
}

struct Surface {
    color: [u8; 4],
    normal: V3,
    id: u32,
}

fn raster_tri(b: &mut Buffers, mut v: [(f32, f32, f32); 3], ortho: bool, far: f32, s: &Surface) {
    let xy = |p: (f32, f32, f32)| (p.0, p.1);
    let mut area = edge(xy(v[0]), xy(v[1]), xy(v[2]));
    if area == 0.0 || !area.is_finite() {
        return;
    }
    if area < 0.0 {
        v.swap(1, 2);
        area = -area;
    }
    let (p0, p1, p2) = (xy(v[0]), xy(v[1]), xy(v[2]));
    // Perspective-correct: 1/z is affine across the screen (z itself in
    // ortho).
    let q = |z: f32| if ortho { z } else { 1.0 / z };
    let (q0, q1, q2) = (q(v[0].2), q(v[1].2), q(v[2].2));
    let (wi, hi) = (b.w as i64, b.h as i64);
    let lo = |a: f32, c: f32, d: f32| a.min(c).min(d).floor().max(0.0) as i64;
    let hi_ = |a: f32, c: f32, d: f32, cap: i64| (a.max(c).max(d).ceil() as i64).min(cap - 1);
    let (x0, x1) = (lo(p0.0, p1.0, p2.0), hi_(p0.0, p1.0, p2.0, wi));
    let (y0, y1) = (lo(p0.1, p1.1, p2.1), hi_(p0.1, p1.1, p2.1, hi));
    let (tl0, tl1, tl2) = (top_left(p1, p2), top_left(p2, p0), top_left(p0, p1));
    let inside = |e: f32, tl: bool| e > 0.0 || (e == 0.0 && tl);
    for y in y0..=y1 {
        for x in x0..=x1 {
            let c = (x as f32 + 0.5, y as f32 + 0.5);
            let (e0, e1, e2) = (edge(p1, p2, c), edge(p2, p0, c), edge(p0, p1, c));
            if !(inside(e0, tl0) && inside(e1, tl1) && inside(e2, tl2)) {
                continue;
            }
            let qi = (e0 * q0 + e1 * q1 + e2 * q2) / area;
            let z = if ortho { qi } else { 1.0 / qi };
            let i = y as usize * b.w as usize + x as usize;
            if z > far || !(z < b.depth[i]) {
                continue;
            }
            b.depth[i] = z;
            b.color[i] = s.color;
            b.normal[i] = s.normal;
            b.id[i] = s.id;
        }
    }
}

// ---- Toon shading, outlines and passes ---------------------------------------------

/// The toon look: the direction toward the light, the bands, the shadow
/// chip that multiplies band 0, and the highlight threshold on `n·l`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Toon {
    pub light: V3,
    pub steps: u32,
    pub shadow: [u8; 4],
    pub highlight: f32,
}

/// The band a normal shades to: 0 (darkest) to `steps − 1` (lit), or
/// `steps` for a highlight.
pub fn band(n: V3, t: &Toon) -> u32 {
    let steps = t.steps.max(1);
    let nl = dot(n, normalize(t.light));
    if t.highlight < 1.0 && nl >= t.highlight {
        return steps;
    }
    let b = (((nl + 1.0) * 0.5) * steps as f32).floor();
    (b.max(0.0) as u32).min(steps - 1)
}

/// `base` shaded to band `b`: band 0 multiplied by the shadow chip, the top
/// band the base itself, the bands between mixed evenly; the highlight
/// lifted halfway to white. Alpha stays the base's.
pub fn band_chip(base: [u8; 4], b: u32, t: &Toon) -> [u8; 4] {
    let steps = t.steps.max(1);
    let mut out = base;
    if b >= steps {
        for c in out.iter_mut().take(3) {
            *c = ((*c as f32 + 255.0) * 0.5).round() as u8;
        }
        return out;
    }
    let mix = if steps > 1 { b as f32 / (steps - 1) as f32 } else { 1.0 };
    for (c, s) in out.iter_mut().zip(t.shadow).take(3) {
        let k = s as f32 / 255.0 + (1.0 - s as f32 / 255.0) * mix;
        *c = (*c as f32 * k).round().clamp(0.0, 255.0) as u8;
    }
    out
}

/// Every pixel's toon chip and band (`u32::MAX` where empty).
pub fn toon_shade(b: &Buffers, t: &Toon) -> (Vec<[u8; 4]>, Vec<u32>) {
    let mut chips = vec![[0u8; 4]; b.id.len()];
    let mut bands = vec![u32::MAX; b.id.len()];
    for i in 0..b.id.len() {
        if b.id[i] == 0 {
            continue;
        }
        let k = band(b.normal[i], t);
        bands[i] = k;
        chips[i] = band_chip(b.color[i], k, t);
    }
    (chips, bands)
}

/// A fold: the neighbour is the same object, this much of the depth farther.
pub const DEPTH_JUMP: f32 = 0.1;

/// The outline mask (see the module laws): edge pixels from silhouettes,
/// folds and creases, dilated to `width` px (whole px, at least one).
/// Width 0 draws none.
pub fn outline(b: &Buffers, width: f32, crease_deg: f32) -> Vec<bool> {
    let (w, h) = (b.w as i64, b.h as i64);
    let n = b.id.len();
    let mut out = vec![false; n];
    if !(width > 0.0) || n == 0 {
        return out;
    }
    let px = width.min(1024.0).round().max(1.0) as i64;
    let cos_crease = crease_deg.clamp(0.0, 180.0).to_radians().cos();
    // Seeds in doubled px, so the crack between two pixels is a whole
    // number: an odd width centres the line on the edge pixel, an even one
    // on the crack toward each neighbour that made it an edge.
    let mut seeds = Vec::new();
    for y in 0..h {
        for x in 0..w {
            let i = (y * w + x) as usize;
            if b.id[i] == 0 {
                continue;
            }
            let hit = |dx: i64, dy: i64| {
                let (nx, ny) = (x + dx, y + dy);
                if nx < 0 || ny < 0 || nx >= w || ny >= h {
                    return false;
                }
                let j = (ny * w + nx) as usize;
                if b.id[j] != b.id[i] {
                    return b.id[j] == 0 || b.depth[i] < b.depth[j];
                }
                if b.depth[j] - b.depth[i] > DEPTH_JUMP * b.depth[i] {
                    return true;
                }
                // A crease marks one side only (the left or upper pixel).
                dx + dy > 0 && dot(b.normal[i], b.normal[j]) < cos_crease
            };
            for (dx, dy) in [(1, 0), (0, 1), (-1, 0), (0, -1)] {
                if !hit(dx, dy) {
                    continue;
                }
                if px % 2 == 1 {
                    seeds.push((2 * x, 2 * y));
                    break;
                }
                seeds.push((2 * x + dx, 2 * y + dy));
            }
        }
    }
    // Within (px − 1)/2 px of a seed: (2X − sx)² + (2Y − sy)² ≤ (px − 1)².
    let r2 = (px - 1) * (px - 1);
    let reach = px / 2 + 1;
    for (sx, sy) in seeds {
        let (cx, cy) = (sx.div_euclid(2), sy.div_euclid(2));
        for y in (cy - reach).max(0)..=(cy + reach).min(h - 1) {
            for x in (cx - reach).max(0)..=(cx + reach).min(w - 1) {
                let (ex, ey) = (2 * x - sx, 2 * y - sy);
                if ex * ex + ey * ey <= r2 {
                    out[(y * w + x) as usize] = true;
                }
            }
        }
    }
    out
}

/// What a Model3D node outputs.
pub const PASSES: &[&str] = &["beauty", "lines", "shadow matte", "depth", "id matte"];

/// Object `id`'s ID matte chip: hues a golden angle apart, so neighbours
/// differ.
pub fn id_chip(id: u32) -> [u8; 4] {
    let hue = (id as f32 * 0.618_034).fract() * 6.0;
    let (s, v) = (0.8f32, 1.0f32);
    let i = hue.floor() as u32 % 6;
    let f = hue - hue.floor();
    let (p, q, t) = (v * (1.0 - s), v * (1.0 - s * f), v * (1.0 - s * (1.0 - f)));
    let (r, g, b) = match i {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    };
    let byte = |c: f32| (c * 255.0).round() as u8;
    [byte(r), byte(g), byte(b), 255]
}

/// The pass `pass` (an index into [`PASSES`]) of rasterised `b`, as
/// premultiplied stored values (D8): chips through `law`, the depth and
/// the shadow matte as plain values.
pub fn shade(b: &Buffers, v: &ModelValues, law: ColorLaw) -> Image {
    let mut img = Image::new(b.w, b.h);
    let toon = v.toon();
    let cam = v.camera();
    let lines = outline(b, v.float("outline"), v.float("crease"));
    let ink = law.premultiplied(v.color("outline_color"));
    let pass = v.choice("pass");
    let (chips, bands) = if pass == 0 || pass == 2 {
        toon_shade(b, &toon)
    } else {
        (Vec::new(), Vec::new())
    };
    let span = (cam.far - cam.near).max(1e-6);
    for (i, px) in img.px.chunks_exact_mut(4).enumerate() {
        let covered = b.id[i] != 0;
        let out: [f32; 4] = match pass {
            0 if lines[i] => ink,
            0 if covered => law.premultiplied(chips[i]),
            1 if lines[i] => ink,
            2 if covered && bands[i] + 1 < toon.steps.max(1) => [1.0; 4],
            3 if covered => {
                let d = ((b.depth[i] - cam.near) / span).clamp(0.0, 1.0);
                [d, d, d, 1.0]
            }
            4 if covered => law.premultiplied(id_chip(b.id[i])),
            _ => [0.0; 4],
        };
        px.copy_from_slice(&out);
    }
    img
}

/// A Model3D node's picture: `model` placed and shot as `v` says, in a
/// `w × h` frame, its objects numbered from 1.
pub fn render(model: &Model, v: &ModelValues, law: ColorLaw, w: u32, h: u32) -> Image {
    render_as(model, v, law, w, h, 1)
}

/// [`render`] with the model's objects numbered from `first_id` (see
/// [`first_object_id`]), so two nodes' ID mattes never share a chip.
pub fn render_as(model: &Model, v: &ModelValues, law: ColorLaw, w: u32, h: u32, first_id: u32) -> Image {
    let mut scene = Scene3::default();
    scene.add_model(model, &v.xform(), first_id.max(1));
    let b = rasterize(&scene, &v.camera(), w, h);
    shade(&b, v, law)
}

/// Object ids a Model3D node reserves in the ID matte.
pub const OBJECTS_PER_NODE: u32 = 4096;

/// The first object id of Model3D node `node`: 1 + [`OBJECTS_PER_NODE`] ×
/// its place among the cut's Model3D nodes (in id order), so every node's
/// objects get chips of their own.
pub fn first_object_id(cut: &Cut, node: NodeId) -> u32 {
    let place = cut
        .graph
        .nodes()
        .filter(|n| matches!(n.kind, NodeKind::Model3D { .. }))
        .position(|n| n.id == node)
        .unwrap_or(0) as u32;
    1 + OBJECTS_PER_NODE * place
}

/// The box around `model` placed by `xf` (its eight corners moved); None
/// when it is empty.
pub fn placed_bbox(model: &Model, xf: &Xform) -> Option<(V3, V3)> {
    let (lo, hi) = model.bbox()?;
    let corner = |i: usize| {
        xf.apply([
            if i & 1 == 0 { lo[0] } else { hi[0] },
            if i & 2 == 0 { lo[1] } else { hi[1] },
            if i & 4 == 0 { lo[2] } else { hi[2] },
        ])
    };
    let mut out = (corner(0), corner(0));
    for i in 1..8 {
        let p = corner(i);
        for k in 0..3 {
            out.0[k] = out.0[k].min(p[k]);
            out.1[k] = out.1[k].max(p[k]);
        }
    }
    Some(out)
}

/// The way the default camera looks from (its eye less its target).
const FRAME_FROM: V3 = [3.5, 2.0, 5.0];

/// A camera that frames `bbox` (min, max) whole in a frame `aspect` wide
/// per tall through a `focal_mm` lens: aimed at its centre from the default
/// camera's direction, `radius / tan(half the narrower angle of view) ×
/// 1.2` away (`radius` the box's bounding sphere's), near a thousandth of
/// that distance and far four radii past the centre.
pub fn frame_box(bbox: (V3, V3), focal_mm: f32, aspect: f32) -> Camera3 {
    let (lo, hi) = bbox;
    let target = mul(add(lo, hi), 0.5);
    let radius = (length(sub(hi, lo)) * 0.5).max(1e-3);
    let tx = (focal_to_fov(focal_mm) * 0.5).tan();
    let half = tx.min(tx / aspect.max(1e-3)).max(1e-6);
    let distance = radius / half * 1.2;
    Camera3 {
        eye: add(target, mul(normalize(FRAME_FROM), distance)),
        target,
        roll_deg: 0.0,
        focal_mm,
        near: distance / 1000.0,
        far: distance + 4.0 * radius,
        ortho: false,
    }
}

/// `params` with the camera written as `cam` — eye, target, roll, lens,
/// near and far — each held to its spec's range.
pub fn set_camera_params(params: &mut FxParams, cam: &Camera3) {
    let values = [
        ("cam_x", cam.eye[0]),
        ("cam_y", cam.eye[1]),
        ("cam_z", cam.eye[2]),
        ("target_x", cam.target[0]),
        ("target_y", cam.target[1]),
        ("target_z", cam.target[2]),
        ("roll", cam.roll_deg),
        ("focal", cam.focal_mm),
        ("near", cam.near),
        ("far", cam.far),
    ];
    for (key, value) in values {
        let value = match spec(key).map(|s| &s.kind) {
            Some(ParamKind::Float { min, max, .. }) => value.clamp(*min, *max),
            _ => value,
        };
        params.set(key, FxValue::Float(value));
    }
}

// ---- The Model3D node's parameters -------------------------------------------------

const POS_HELP: &str = "In the model's units (metres for glTF).";

/// The camera's parameters, in the order "Set cut camera from view" keys
/// them.
pub const CAMERA_KEYS: &[&str] = &[
    "cam_x", "cam_y", "cam_z", "target_x", "target_y", "target_z", "roll", "focal",
];

pub const MODEL_SPECS: &[ParamSpec] = &[
    float("tx", "move x", "Moves the model along X (right). In the model's units.", -1.0e6, 1.0e6, 0.0, 0.01),
    float("ty", "move y", "Moves the model along Y (up). In the model's units.", -1.0e6, 1.0e6, 0.0, 0.01),
    float("tz", "move z", "Moves the model along Z (toward the front view). In the model's units.", -1.0e6, 1.0e6, 0.0, 0.01),
    float("rx", "rotate x", "Turns the model about X, in degrees (applied first).", -36000.0, 36000.0, 0.0, 0.1),
    float("ry", "rotate y", "Turns the model about Y, in degrees (a turntable turns this).", -36000.0, 36000.0, 0.0, 0.1),
    float("rz", "rotate z", "Turns the model about Z, in degrees (applied last).", -36000.0, 36000.0, 0.0, 0.1),
    float("scale", "scale", "Scales the model evenly: 2 is twice its size.", 0.0001, 10000.0, 1.0, 0.01),
    float("cam_x", "camera x", POS_HELP, -1.0e6, 1.0e6, 3.5, 0.01),
    float("cam_y", "camera y", POS_HELP, -1.0e6, 1.0e6, 2.5, 0.01),
    float("cam_z", "camera z", POS_HELP, -1.0e6, 1.0e6, 5.0, 0.01),
    float("target_x", "target x", "Where the camera looks, along X; it lands at the frame's centre.", -1.0e6, 1.0e6, 0.0, 0.01),
    float("target_y", "target y", "Where the camera looks, along Y; it lands at the frame's centre.", -1.0e6, 1.0e6, 0.5, 0.01),
    float("target_z", "target z", "Where the camera looks, along Z; it lands at the frame's centre.", -1.0e6, 1.0e6, 0.0, 0.01),
    float("roll", "roll", "Turns the camera about its view axis, in degrees (a dutch angle).", -360.0, 360.0, 0.0, 0.1),
    float("focal", "focal length (mm)", "The lens on a 36 mm film back: 50 mm sees 39.6° across, longer lenses less.", 1.0, 2000.0, 35.0, 0.1),
    float("near", "near", "Nothing nearer the camera than this is drawn. In the model's units.", 0.0001, 1.0e6, 0.05, 0.001),
    float("far", "far", "Nothing farther than this is drawn; the depth pass reaches white here.", 0.001, 1.0e7, 100.0, 0.1),
    float("light_x", "light x", "The direction toward the light, along X (it is normalised).", -1.0, 1.0, -0.4, 0.01),
    float("light_y", "light y", "The direction toward the light, along Y.", -1.0, 1.0, 0.8, 0.01),
    float("light_z", "light z", "The direction toward the light, along Z.", -1.0, 1.0, 0.45, 0.01),
    float("steps", "shade steps", "Two bands (lit and shadow) or three (lit, half and shadow).", 2.0, 3.0, 2.0, 1.0),
    color("shadow", "shadow colour", "Multiplies the base colour in the darkest band (the kage colour).", [168, 160, 196, 255]),
    float("highlight", "highlight", "Where the light falls this square on (n·l, 0–1) the colour lifts toward white; 1 is none.", 0.0, 1.0, 1.0, 0.005),
    float("outline", "outline width", "The outline's width in px (silhouettes, folds and creases); 0 is none.", 0.0, 64.0, 2.0, 0.05),
    color("outline_color", "outline colour", "The outline's colour.", [40, 32, 40, 255]),
    float("crease", "crease angle", "Faces meeting sharper than this many degrees get a line; 180 draws silhouettes only.", 0.0, 180.0, 60.0, 0.5),
    choice("pass", "pass", "Beauty (toon and lines), Lines alone, the Shadow matte, Depth (near black to far white) or an ID matte per object.", PASSES),
];

/// The Model3D spec with `key`.
pub fn spec(key: &str) -> Option<&'static ParamSpec> {
    MODEL_SPECS.iter().find(|s| s.key == key)
}

/// Every spec written out at its default (a new node).
pub fn default_params() -> FxParams {
    FxParams(
        MODEL_SPECS
            .iter()
            .map(|s| (s.key.to_string(), s.default_value()))
            .collect(),
    )
}

/// A Model3D node's parameters resolved at one frame, in spec order.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelValues {
    pub values: Vec<FxValue>,
}

impl ModelValues {
    /// A bound column with a value at the frame wins, else the stored
    /// value, else the spec default (the Effect's law).
    pub fn resolve(params: &FxParams, binds: &FxBinds, column: impl Fn(ParamId) -> Option<f32>) -> Self {
        Self {
            values: fx::resolve_specs(MODEL_SPECS, params, binds, column),
        }
    }

    /// The stored values alone (no binds).
    pub fn of(params: &FxParams) -> Self {
        Self::resolve(params, &FxBinds::default(), |_| None)
    }

    fn value(&self, key: &str) -> &FxValue {
        let i = MODEL_SPECS
            .iter()
            .position(|s| s.key == key)
            .unwrap_or_else(|| panic!("Model3D has no parameter '{key}'"));
        &self.values[i]
    }

    pub fn float(&self, key: &str) -> f32 {
        match self.value(key) {
            FxValue::Float(x) => *x,
            v => panic!("'{key}' is not a number: {v:?}"),
        }
    }

    /// Set number `key` for this one picture (View3D draws the outline at
    /// its own render's scale); the node is untouched.
    pub fn set_float(&mut self, key: &str, x: f32) {
        let i = MODEL_SPECS
            .iter()
            .position(|s| s.key == key)
            .unwrap_or_else(|| panic!("Model3D has no parameter '{key}'"));
        match &mut self.values[i] {
            FxValue::Float(v) => *v = x,
            v => panic!("'{key}' is not a number: {v:?}"),
        }
    }

    pub fn color(&self, key: &str) -> [u8; 4] {
        match self.value(key) {
            FxValue::Color(c) => *c,
            v => panic!("'{key}' is not a colour: {v:?}"),
        }
    }

    /// A choice's index into its spec's options.
    pub fn choice(&self, key: &str) -> usize {
        let (FxValue::Choice(s), Some(spec)) = (self.value(key), spec(key)) else {
            panic!("'{key}' is not a choice");
        };
        match spec.kind {
            ParamKind::Choice { options, default, .. } => options.iter().position(|o| o == s).unwrap_or(default),
            _ => 0,
        }
    }

    /// The node's camera (always perspective).
    pub fn camera(&self) -> Camera3 {
        let near = self.float("near");
        Camera3 {
            eye: [self.float("cam_x"), self.float("cam_y"), self.float("cam_z")],
            target: [self.float("target_x"), self.float("target_y"), self.float("target_z")],
            roll_deg: self.float("roll"),
            focal_mm: self.float("focal"),
            near,
            far: self.float("far").max(near),
            ortho: false,
        }
    }

    /// The object's placement.
    pub fn xform(&self) -> Xform {
        Xform {
            translate: [self.float("tx"), self.float("ty"), self.float("tz")],
            rotate_deg: [self.float("rx"), self.float("ry"), self.float("rz")],
            scale: self.float("scale"),
        }
    }

    pub fn toon(&self) -> Toon {
        Toon {
            light: [self.float("light_x"), self.float("light_y"), self.float("light_z")],
            steps: self.float("steps").round().clamp(2.0, 3.0) as u32,
            shadow: self.color("shadow"),
            highlight: self.float("highlight"),
        }
    }

    /// `key=value,...` in spec order: the eval hash's part of the recipe.
    pub fn recipe(&self) -> String {
        MODEL_SPECS
            .iter()
            .zip(&self.values)
            .map(|(s, v)| format!("{}={v}", s.key))
            .collect::<Vec<_>>()
            .join(",")
    }
}

/// The column a camera parameter keys into: its bind, when that column
/// still exists.
fn camera_column(cut: &Cut, binds: &FxBinds, key: &str) -> Option<ParamId> {
    binds.get(key).filter(|p| cut.xsheet.param(*p).is_some())
}

/// The cut camera's column for camera parameter `key`: the first live bind
/// among the cut's Model3D nodes (in id order).
fn cut_camera_column(cut: &Cut, key: &str) -> Option<ParamId> {
    cut.graph.nodes().find_map(|n| match &n.kind {
        NodeKind::Model3D { binds, .. } => camera_column(cut, binds, key),
        _ => None,
    })
}

/// The column a camera parameter keys into from Model3D node `binds`: its
/// own live bind, else the cut camera's.
fn shared_camera_column(cut: &Cut, binds: &FxBinds, key: &str) -> Option<ParamId> {
    camera_column(cut, binds, key).or_else(|| cut_camera_column(cut, key))
}

/// How many new param columns [`key_camera`] needs for Model3D node `node`:
/// one per camera parameter neither it nor any other Model3D node binds to
/// a column.
pub fn camera_columns_needed(cut: &Cut, node: NodeId) -> usize {
    match cut.graph.node(node).map(|n| &n.kind) {
        Ok(NodeKind::Model3D { binds, .. }) => CAMERA_KEYS
            .iter()
            .filter(|k| shared_camera_column(cut, binds, k).is_none())
            .count(),
        _ => 0,
    }
}

/// A cut camera column's name: "cut camera x", "cut camera target x", …
fn cut_camera_name(spec: &ParamSpec) -> String {
    let label = spec.label.split(" (").next().unwrap_or(spec.label);
    match label.strip_prefix("camera ") {
        Some(axis) => format!("cut camera {axis}"),
        None => format!("cut camera {label}"),
    }
}

/// "Set cut camera from view": the edits that key the cut's one camera to
/// `cam` at `frame`, seen from Model3D node `node` — each camera parameter
/// keys (Linear) the column `node` binds it to, else the one another
/// Model3D node binds, else a new param column named for it ("cut camera
/// x"; ids from `fresh`, [`camera_columns_needed`] of them); and every
/// Model3D node in the cut is bound to those columns, so all of them shoot
/// through the same camera. Apply them as one batch: one undo step.
pub fn key_camera(
    cut: &Cut,
    at: CutRef,
    node: NodeId,
    cam: &Camera3,
    frame: u32,
    fresh: &mut impl Iterator<Item = ParamId>,
) -> Result<Vec<Command>, String> {
    let n = cut.graph.node(node).map_err(|e| e.to_string())?;
    let NodeKind::Model3D { binds: bound, .. } = &n.kind else {
        return Err("not a Model3D node".into());
    };
    let values = [
        cam.eye[0],
        cam.eye[1],
        cam.eye[2],
        cam.target[0],
        cam.target[1],
        cam.target[2],
        cam.roll_deg,
        cam.focal_mm,
    ];
    let mut columns = Vec::with_capacity(CAMERA_KEYS.len());
    let mut cmds = Vec::new();
    let mut index = cut.xsheet.params.len();
    for (key, value) in CAMERA_KEYS.iter().zip(values) {
        let spec = spec(key).expect("camera keys are specs");
        let value = match spec.kind {
            ParamKind::Float { min, max, .. } => value.clamp(min, max),
            _ => value,
        };
        let column = match shared_camera_column(cut, bound, key) {
            Some(p) => p,
            None => {
                let id = fresh.next().ok_or("no param column id left")?;
                cmds.push(Command::AddParamColumn {
                    at,
                    index,
                    column: ParamColumn::new(id, cut_camera_name(spec)),
                });
                index += 1;
                id
            }
        };
        columns.push((*key, column));
        cmds.push(Command::SetParamKey {
            at,
            column,
            frame,
            key: Some(ParamKey {
                value,
                interp: ParamInterp::Linear,
                handles: None,
            }),
        });
    }
    for n in cut.graph.nodes() {
        let NodeKind::Model3D { model, params, binds: bound } = &n.kind else {
            continue;
        };
        let mut binds = bound.clone();
        for (key, column) in &columns {
            binds.set(key, Some(*column));
        }
        if binds != *bound {
            cmds.push(Command::SetNodeKind {
                at,
                id: n.id,
                kind: NodeKind::Model3D {
                    model: *model,
                    params: params.clone(),
                    binds,
                },
            });
        }
    }
    Ok(cmds)
}

// ---- View3D navigation ----------------------------------------------------------------

/// A numpad view (Blender's keys: 1 front, 3 right, 7 top; Ctrl for the
/// opposite side).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Front,
    Back,
    Right,
    Left,
    Top,
    Bottom,
}

/// The View3D pane's camera (pure navigation maths): the eye circles
/// `target` at `distance`; `yaw_deg` turns about Y (0 looks from +Z, 90 from
/// +X) and `pitch_deg` rises above the horizon (90 looks straight down).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Orbit {
    pub target: V3,
    pub yaw_deg: f32,
    pub pitch_deg: f32,
    pub distance: f32,
    pub focal_mm: f32,
    pub ortho: bool,
}

impl Default for Orbit {
    fn default() -> Self {
        Self::from_camera(&ModelValues::of(&FxParams::default()).camera())
    }
}

/// Degrees the view orbits per px dragged.
pub const ORBIT_DEG_PER_PX: f32 = 0.4;

impl Orbit {
    pub fn eye(&self) -> V3 {
        let (sy, cy) = self.yaw_deg.to_radians().sin_cos();
        let (sp, cp) = self.pitch_deg.to_radians().sin_cos();
        add(self.target, mul([cp * sy, sp, cp * cy], self.distance))
    }

    /// The orbit whose eye is `eye` around `target` (the inverse of
    /// [`Orbit::eye`] for pitches inside ±90°).
    pub fn from_eye(eye: V3, target: V3, focal_mm: f32) -> Self {
        let d = sub(eye, target);
        let distance = length(d).max(1e-6);
        Self {
            target,
            yaw_deg: d[0].atan2(d[2]).to_degrees(),
            pitch_deg: (d[1] / distance).clamp(-1.0, 1.0).asin().to_degrees(),
            distance,
            focal_mm,
            ortho: false,
        }
    }

    /// Looking through a camera (numpad 0); its roll is not kept.
    pub fn from_camera(cam: &Camera3) -> Self {
        Self::from_eye(cam.eye, cam.target, cam.focal_mm)
    }

    pub fn camera(&self) -> Camera3 {
        Camera3 {
            eye: self.eye(),
            target: self.target,
            roll_deg: 0.0,
            focal_mm: self.focal_mm,
            near: (self.distance * 0.001).max(1e-4),
            far: self.distance * 1000.0,
            ortho: self.ortho,
        }
    }

    pub fn basis(&self) -> Basis {
        look_at(self.eye(), self.target, 0.0)
    }

    /// Middle-drag: `dx` px turns about Y, `dy` px tilts (held within the
    /// poles).
    pub fn orbit(&mut self, dx: f32, dy: f32) {
        self.yaw_deg = (self.yaw_deg - dx * ORBIT_DEG_PER_PX).rem_euclid(360.0);
        if self.yaw_deg > 180.0 {
            self.yaw_deg -= 360.0;
        }
        self.pitch_deg = (self.pitch_deg + dy * ORBIT_DEG_PER_PX).clamp(-90.0, 90.0);
    }

    /// Shift+middle-drag: the target slides with the pointer, a px of drag
    /// moving it a px's worth at the target in a frame `frame_h` px tall.
    pub fn pan(&mut self, dx: f32, dy: f32, frame_h: f32) {
        let b = self.basis();
        let half_h = self.distance * (focal_to_fov(self.focal_mm) * 0.5).tan();
        let per_px = 2.0 * half_h / frame_h.max(1.0);
        self.target = add(self.target, add(mul(b.right, -dx * per_px), mul(b.up, dy * per_px)));
    }

    /// The wheel or Ctrl+middle-drag: each step in comes 10% closer.
    pub fn zoom(&mut self, steps: f32) {
        self.distance = (self.distance * 0.9f32.powf(steps)).clamp(1e-3, 1e6);
    }

    /// "Frame all" (Blender's Home): the view keeps its direction and lens
    /// and closes on `bbox` (min, max) in a frame `aspect` wide per tall,
    /// as [`frame_box`] places a camera.
    pub fn frame(&mut self, bbox: (V3, V3), aspect: f32) {
        let cam = frame_box(bbox, self.focal_mm, aspect);
        self.target = cam.target;
        self.distance = length(sub(cam.eye, cam.target));
    }

    /// A numpad view: exact yaw and pitch, the target and distance kept.
    pub fn set_view(&mut self, v: View) {
        let (yaw, pitch) = match v {
            View::Front => (0.0, 0.0),
            View::Back => (180.0, 0.0),
            View::Right => (90.0, 0.0),
            View::Left => (-90.0, 0.0),
            View::Top => (0.0, 90.0),
            View::Bottom => (0.0, -90.0),
        };
        self.yaw_deg = yaw;
        self.pitch_deg = pitch;
    }
}

// ---- Image sequences and stills from other apps (D15) -------------------------------

/// The file number cut frame `frame` shows: file `first` at frame
/// `offset`, counting up a frame at a time; None before `offset`.
pub fn sequence_number(first: u32, offset: i32, frame: u32) -> Option<u32> {
    let n = frame as i64 - offset as i64;
    (n >= 0).then(|| (first as i64 + n).min(u32::MAX as i64) as u32)
}

/// `pattern` with its first run of `#` replaced by `n`, zero-padded to the
/// run's length (a longer number is written whole). A pattern with no `#`
/// names one still for every frame.
pub fn expand_hashes(pattern: &str, n: u32) -> String {
    let Some(start) = pattern.find('#') else {
        return pattern.to_string();
    };
    let run = pattern[start..].chars().take_while(|c| *c == '#').count();
    format!("{}{:0run$}{}", &pattern[..start], n, &pattern[start + run..])
}

/// The file a name reads: relative ones from `base` (the project's folder)
/// when there is one.
pub fn resolve_path(name: &str, base: Option<&Path>) -> PathBuf {
    let p = Path::new(name);
    match base {
        Some(b) if p.is_relative() => b.join(p),
        _ => p.to_path_buf(),
    }
}

/// A decoded still: straight values as the file holds them, and whether
/// they are scene-linear (EXR).
#[derive(Debug, Clone, PartialEq)]
pub struct Still {
    pub w: u32,
    pub h: u32,
    pub px: Vec<f32>,
    pub linear: bool,
}

/// Decode a PNG, TIFF, EXR or JPEG from its bytes.
pub fn decode_still(bytes: &[u8]) -> Result<Still, String> {
    let format = image::guess_format(bytes).map_err(|e| format!("not an image this build reads ({e})"))?;
    let img = image::load_from_memory_with_format(bytes, format).map_err(|e| format!("it does not decode ({e})"))?;
    let rgba = img.to_rgba32f();
    Ok(Still {
        w: rgba.width(),
        h: rgba.height(),
        px: rgba.into_raw(),
        linear: format == image::ImageFormat::OpenExr,
    })
}

/// An imported still as the PNG an ImageSource stores: a PNG's own bytes,
/// verbatim; anything else (JPEG, TIFF) decoded and written as 8-bit RGBA
/// PNG.
pub fn still_to_png(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let format = image::guess_format(bytes).map_err(|e| format!("not an image this build reads ({e})"))?;
    if format == image::ImageFormat::Png {
        return Ok(bytes.to_vec());
    }
    let img = image::load_from_memory_with_format(bytes, format).map_err(|e| format!("it does not decode ({e})"))?;
    let rgba = img.to_rgba8();
    let mut out = Vec::new();
    image::codecs::png::PngEncoder::new(&mut out)
        .write_image(rgba.as_raw(), rgba.width(), rgba.height(), image::ExtendedColorType::Rgba8)
        .map_err(|e| format!("it does not re-encode ({e})"))?;
    Ok(out)
}

use image::ImageEncoder as _;

/// The file an ImageSequence shows at `frame`, if it starts by then: with
/// `hold_last`, a missing file falls back to the nearest earlier one that
/// exists (so the last frame holds past the end).
pub fn sequence_file(
    pattern: &str,
    first: u32,
    offset: i32,
    hold_last: bool,
    frame: u32,
    base: Option<&Path>,
) -> Option<PathBuf> {
    let n = sequence_number(first, offset, frame)?;
    let path = resolve_path(&expand_hashes(pattern, n), base);
    if !hold_last || path.is_file() || !pattern.contains('#') {
        return Some(path);
    }
    // At most a long cut's worth back: a pattern with nothing on disk
    // must not scan forever.
    let lowest = first.max(n.saturating_sub(100_000));
    (lowest..n)
        .rev()
        .map(|k| resolve_path(&expand_hashes(pattern, k), base))
        .find(|p| p.is_file())
        .or(Some(path))
}

/// GM22: what an ImageSequence's eval value folds of the file it shows —
/// its length and modified time (ns since the epoch): a stat, never a read.
/// None when there is no such file.
pub fn sequence_stamp(path: &Path) -> Option<(u64, u128)> {
    let meta = std::fs::metadata(path).ok().filter(|m| m.is_file())?;
    let ns = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_nanos());
    Some((meta.len(), ns))
}

/// GM22: the file stats one ImageSequence node's eval values make, kept
/// as long as its values are (the evaluator drops both together): each
/// file number is stat'ed once, and a held frame's search back skips the
/// runs already known empty — so a pass over a cut held past its
/// sequence's end, or over a missing folder, stats each number once
/// instead of every earlier one for every frame. It picks the file
/// [`sequence_file`] picks.
#[derive(Debug, Default)]
pub struct SeqStats {
    /// The pattern and folder the numbers were stat'ed for.
    pattern: String,
    base: Option<PathBuf>,
    /// The numbers whose file exists, with its stamp.
    found: BTreeMap<u32, (u64, u128)>,
    /// Runs of numbers with no file: first → last (inclusive).
    gaps: BTreeMap<u32, u32>,
}

impl SeqStats {
    /// The stamp ([`sequence_stamp`]) of the file the sequence shows at
    /// `frame`, None when there is none; `stats` counts the stats made.
    #[allow(clippy::too_many_arguments)]
    pub fn shown(
        &mut self,
        pattern: &str,
        first: u32,
        offset: i32,
        hold_last: bool,
        frame: u32,
        base: Option<&Path>,
        stats: &mut u64,
    ) -> Option<(u64, u128)> {
        if self.pattern != pattern || self.base.as_deref() != base {
            *self = SeqStats {
                pattern: pattern.to_string(),
                base: base.map(Path::to_path_buf),
                ..SeqStats::default()
            };
        }
        let n = sequence_number(first, offset, frame)?;
        if let Some(stamp) = self.stat(n, stats) {
            return Some(stamp);
        }
        if !hold_last || !pattern.contains('#') {
            return None;
        }
        // `sequence_file`'s search: the nearest earlier file, at most a
        // long cut's worth back.
        let lowest = first.max(n.saturating_sub(100_000));
        let mut k = n;
        while k > lowest {
            k -= 1;
            match self.gap_start(k) {
                // A run known empty: on from below it.
                Some(start) => k = start,
                None => {
                    if let Some(stamp) = self.stat(k, stats) {
                        return Some(stamp);
                    }
                }
            }
        }
        None
    }

    /// File `k`'s stamp: known, or one stat.
    fn stat(&mut self, k: u32, stats: &mut u64) -> Option<(u64, u128)> {
        if let Some(stamp) = self.found.get(&k) {
            return Some(*stamp);
        }
        if self.gap_start(k).is_some() {
            return None;
        }
        *stats += 1;
        let stamp = sequence_stamp(&resolve_path(&expand_hashes(&self.pattern, k), self.base.as_deref()));
        match stamp {
            Some(s) => {
                self.found.insert(k, s);
            }
            None => self.add_gap(k),
        }
        stamp
    }

    /// The first number of the empty run that holds `k`.
    fn gap_start(&self, k: u32) -> Option<u32> {
        self.gaps
            .range(..=k)
            .next_back()
            .filter(|(_, last)| **last >= k)
            .map(|(start, _)| *start)
    }

    /// `k` has no file: one run with the runs just below and above it.
    fn add_gap(&mut self, k: u32) {
        let start = k.checked_sub(1).and_then(|below| self.gap_start(below)).unwrap_or(k);
        let last = k.checked_add(1).and_then(|above| self.gaps.remove(&above)).unwrap_or(k);
        self.gaps.insert(start, last);
    }
}

/// The pattern and first number for a sequence picked by one of its files:
/// the last run of digits in the file's name becomes as many `#`
/// (`shot_0001.png` is `shot_####.png` from 1), and a file under `base`
/// (the project's folder) is named relative to it, with `/`. A name with
/// no digits is one still, from 1.
pub fn pattern_from_file(path: &Path, base: Option<&Path>) -> (String, u32) {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let stem_len = match name.rfind('.') {
        Some(dot) if dot > 0 => dot,
        _ => name.len(),
    };
    let stem = &name[..stem_len];
    let (pattern_name, first) = match stem.rfind(|c: char| c.is_ascii_digit()) {
        Some(last) => {
            let start = stem[..=last]
                .rfind(|c: char| !c.is_ascii_digit())
                .map_or(0, |i| i + 1);
            let digits = &stem[start..=last];
            let first = digits.parse::<u64>().unwrap_or(1).min(u32::MAX as u64) as u32;
            let hashes = "#".repeat(digits.len());
            (format!("{}{hashes}{}", &name[..start], &name[last + 1..]), first)
        }
        None => (name.clone(), 1),
    };
    let folder = path.parent().unwrap_or(Path::new(""));
    let full = folder.join(&pattern_name);
    let shown = match base.and_then(|b| full.strip_prefix(b).ok()) {
        Some(rel) => rel.to_string_lossy().replace('\\', "/"),
        None => full.to_string_lossy().into_owned(),
    };
    (shown, first)
}

/// What a relative pattern with no project folder reports.
pub const NO_FOLDER: &str = "save the project first: relative frames read from its folder";

/// An ImageSequence's picture at `frame` over `rect` (x, y, w, h in the
/// frame's px), premultiplied stored values (D8), its values as the file
/// holds them (an imported PNG's law); an EXR's scene-linear values
/// encoded for an sRGB project. A file the size of the frame (`frame_size`)
/// — or any file without `fit` — sits at its native size from the frame's
/// top-left; with `fit` one of another size is scaled to fit the frame,
/// centred, sampled bilinearly. A missing or unreadable file, or a relative
/// pattern with no project folder (`base`), is transparent and reported
/// through `warn`.
#[allow(clippy::too_many_arguments)]
pub fn sequence_frame(
    pattern: &str,
    first: u32,
    offset: i32,
    hold_last: bool,
    fit: bool,
    frame: u32,
    base: Option<&Path>,
    law: ColorLaw,
    frame_size: (u32, u32),
    rect: (i32, i32, u32, u32),
    warn: &mut dyn FnMut(String),
) -> Vec<f32> {
    let (rx, ry, rw, rh) = rect;
    let mut out = vec![0.0f32; rw as usize * rh as usize * 4];
    if pattern.trim().is_empty() {
        return out;
    }
    if base.is_none() && Path::new(pattern).is_relative() {
        warn(NO_FOLDER.to_string());
        return out;
    }
    let Some(path) = sequence_file(pattern, first, offset, hold_last, frame, base) else {
        return out;
    };
    let still = match std::fs::read(&path) {
        Ok(bytes) => match decode_still(&bytes) {
            Ok(s) => s,
            Err(e) => {
                warn(format!("{}: {e}", path.display()));
                return out;
            }
        },
        Err(_) => {
            warn(format!("missing frame {}", path.display()));
            return out;
        }
    };
    let encode = |v: f32| {
        if still.linear && law == ColorLaw::Srgb {
            crate::export::linear_to_srgb_f(v)
        } else {
            v
        }
    };
    let (fw, fh) = frame_size;
    if fit && (still.w, still.h) != (fw, fh) && still.w > 0 && still.h > 0 && fw > 0 && fh > 0 {
        // Contain: the whole file, as large as fits, centred.
        let s = (fw as f32 / still.w as f32).min(fh as f32 / still.h as f32);
        let (bw, bh) = (still.w as f32 * s, still.h as f32 * s);
        let (ox, oy) = ((fw as f32 - bw) * 0.5, (fh as f32 - bh) * 0.5);
        // One texel premultiplied, its coordinates held to the file.
        let texel = |tx: i64, ty: i64| -> [f32; 4] {
            let tx = tx.clamp(0, still.w as i64 - 1);
            let ty = ty.clamp(0, still.h as i64 - 1);
            let i = ((ty * still.w as i64 + tx) * 4) as usize;
            let a = still.px[i + 3].clamp(0.0, 1.0);
            [encode(still.px[i]) * a, encode(still.px[i + 1]) * a, encode(still.px[i + 2]) * a, a]
        };
        for y in 0..rh as i64 {
            let fy = (y + ry as i64) as f32 + 0.5;
            if fy < oy || fy >= oy + bh {
                continue;
            }
            let v = (fy - oy) / s - 0.5;
            let (ty, wy) = (v.floor() as i64, v - v.floor());
            for x in 0..rw as i64 {
                let fx = (x + rx as i64) as f32 + 0.5;
                if fx < ox || fx >= ox + bw {
                    continue;
                }
                let u = (fx - ox) / s - 0.5;
                let (tx, wx) = (u.floor() as i64, u - u.floor());
                let (p00, p10) = (texel(tx, ty), texel(tx + 1, ty));
                let (p01, p11) = (texel(tx, ty + 1), texel(tx + 1, ty + 1));
                let d = ((y * rw as i64 + x) * 4) as usize;
                for c in 0..4 {
                    let top = p00[c] + (p10[c] - p00[c]) * wx;
                    let bottom = p01[c] + (p11[c] - p01[c]) * wx;
                    out[d + c] = top + (bottom - top) * wy;
                }
            }
        }
        return out;
    }
    for y in 0..rh as i64 {
        let sy = y + ry as i64;
        if sy < 0 || sy >= still.h as i64 {
            continue;
        }
        for x in 0..rw as i64 {
            let sx = x + rx as i64;
            if sx < 0 || sx >= still.w as i64 {
                continue;
            }
            let s = ((sy * still.w as i64 + sx) * 4) as usize;
            let a = still.px[s + 3].clamp(0.0, 1.0);
            let d = ((y * rw as i64 + x) * 4) as usize;
            for c in 0..3 {
                out[d + c] = encode(still.px[s + c]) * a;
            }
            out[d + 3] = a;
        }
    }
    out
}
