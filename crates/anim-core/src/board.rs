//! GM15 EDITORIAL (編集): what the henshū room reads and edits across the
//! cuts of a scene — each cut's production status (`prod.*` cut meta, D3),
//! the episode's running time (TRT) and frame map, the pinned CMX3600 EDL,
//! the colour designation sheet (色指定表) for the paint team, cuts made
//! from storyboard panels, and retake notes. Pure: the app's Board and
//! Episode panes, the render queue's Episode job and the sheet's note
//! markers all go through here.

use std::collections::BTreeMap;

use crate::color::ColorLaw;
use crate::command::{Command, CutRef};
use crate::deliver::{self, Names};
use crate::fx::Image;
use crate::ids::*;
use crate::model::{Cut, Project, Scene};
use crate::text::{self, Face};
use crate::xsheet::{Column, ColumnDef, ColumnKind, Exposure};

/// The cut's production status: a JSON string, a [`CutStatus::key`].
pub const STATUS_KEY: &str = "prod.status";
/// Who has the cut: a JSON string.
pub const ASSIGNEE_KEY: &str = "prod.assignee";
/// The cut's retake notes: a JSON list of [`RetakeNote`].
pub const NOTES_KEY: &str = "prod.notes";
/// The episode's target running time, in frames (a JSON number), in
/// `Project::app_meta` beside the palettes.
pub const TARGET_KEY: &str = "episode.target";

// ---- Production status -----------------------------------------------------

/// Where a cut stands in production, in pipeline order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum CutStatus {
    Lo,
    Genga,
    Sakkan,
    Douga,
    Paint,
    Composite,
    Done,
    Retake,
}

impl CutStatus {
    pub const ALL: [CutStatus; 8] = [
        CutStatus::Lo,
        CutStatus::Genga,
        CutStatus::Sakkan,
        CutStatus::Douga,
        CutStatus::Paint,
        CutStatus::Composite,
        CutStatus::Done,
        CutStatus::Retake,
    ];

    /// Stable persisted key (the `prod.status` JSON). Never rename these.
    pub fn key(self) -> &'static str {
        match self {
            CutStatus::Lo => "lo",
            CutStatus::Genga => "genga",
            CutStatus::Sakkan => "sakkan",
            CutStatus::Douga => "douga",
            CutStatus::Paint => "paint",
            CutStatus::Composite => "composite",
            CutStatus::Done => "done",
            CutStatus::Retake => "retake",
        }
    }

    pub fn from_key(key: &str) -> Option<CutStatus> {
        CutStatus::ALL.into_iter().find(|s| s.key() == key)
    }

    /// The board's word for it: the studio's term plus English (D7).
    pub fn label(self) -> &'static str {
        match self {
            CutStatus::Lo => "LO (layout)",
            CutStatus::Genga => "genga (key)",
            CutStatus::Sakkan => "sakkan (AD check)",
            CutStatus::Douga => "douga (inbetween)",
            CutStatus::Paint => "shiage (paint)",
            CutStatus::Composite => "satsuei (composite)",
            CutStatus::Done => "done",
            CutStatus::Retake => "retake",
        }
    }
}

/// One retake note on a cut: the frame it is about (0-based), what to fix,
/// who said it and whether it is done.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct RetakeNote {
    pub frame: u32,
    pub text: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub resolved: bool,
}

/// A cut's production info, read from and written to its meta (D3).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProdInfo {
    pub status: Option<CutStatus>,
    /// Empty = nobody yet.
    pub assignee: String,
    /// In stored order ([`add_note`] keeps it in frame order); read only —
    /// [`ProdInfo::commands`] leaves the notes key alone.
    pub notes: Vec<RetakeNote>,
}

fn meta_text(cut: &Cut, key: &str) -> Option<String> {
    cut.meta
        .get(key)
        .and_then(|v| serde_json::from_str::<String>(v).ok())
}

fn json_text(s: &str) -> String {
    serde_json::to_string(s).expect("a string serialises")
}

/// One stored note as this build reads it: an object with a frame; a
/// missing text, author or resolved reads as empty.
fn note_of(v: &serde_json::Value) -> Option<RetakeNote> {
    let text = |key: &str| v.get(key).and_then(|t| t.as_str()).unwrap_or_default().to_string();
    Some(RetakeNote {
        frame: note_frame(v)?,
        text: text("text"),
        author: text("author"),
        resolved: v.get("resolved").and_then(|r| r.as_bool()).unwrap_or(false),
    })
}

/// The cut's notes list as stored, as JSON values so a field a newer build
/// added survives an edit (D3): empty when the key is unset, None when the
/// text is not a list — an edit then refuses rather than overwrite it.
pub fn stored_notes(cut: &Cut) -> Option<Vec<serde_json::Value>> {
    match cut.meta.get(NOTES_KEY) {
        None => Some(Vec::new()),
        Some(text) => note_values(text),
    }
}

/// The cut's retake notes in stored order (none when the key is missing or
/// is not a list; an entry without a frame is skipped, the rest still read).
pub fn notes_of(cut: &Cut) -> Vec<RetakeNote> {
    stored_notes(cut).unwrap_or_default().iter().filter_map(note_of).collect()
}

/// The one SetCutMeta that stores `list` as the cut's notes (an empty list
/// removes the key).
fn notes_command(cut: &Cut, at: CutRef, list: &[serde_json::Value]) -> Command {
    Command::SetCutMeta {
        at,
        key: NOTES_KEY.into(),
        before: cut.meta.get(NOTES_KEY).cloned(),
        after: (!list.is_empty()).then(|| serde_json::to_string(list).expect("notes serialise")),
    }
}

impl ProdInfo {
    /// Read from the cut's meta; a key that does not read is as if unset.
    pub fn of(cut: &Cut) -> ProdInfo {
        ProdInfo {
            status: meta_text(cut, STATUS_KEY).and_then(|k| CutStatus::from_key(&k)),
            assignee: meta_text(cut, ASSIGNEE_KEY).unwrap_or_default(),
            notes: notes_of(cut),
        }
    }

    /// The SetCutMeta commands that make the cut's status and assignee say
    /// this: one per field that differs from what the cut says now (no
    /// status or an empty assignee removes its key). A key whose value this
    /// build cannot read is left alone unless its field changes. The notes
    /// are not written here: [`add_note`] and [`set_note_resolved`] edit
    /// them in place, keeping fields this build does not know. Empty when
    /// nothing changes; apply them together as one undo step.
    pub fn commands(&self, cut: &Cut, at: CutRef) -> Vec<Command> {
        let now = ProdInfo::of(cut);
        let mut out = Vec::new();
        let mut set = |key: &str, after: Option<String>| {
            out.push(Command::SetCutMeta {
                at,
                key: key.into(),
                before: cut.meta.get(key).cloned(),
                after,
            });
        };
        if self.status != now.status {
            set(STATUS_KEY, self.status.map(|s| json_text(s.key())));
        }
        if self.assignee != now.assignee {
            set(
                ASSIGNEE_KEY,
                (!self.assignee.is_empty()).then(|| json_text(&self.assignee)),
            );
        }
        out
    }

    /// The notes not yet resolved (the count the board shows).
    pub fn open_notes(&self) -> usize {
        self.notes.iter().filter(|n| !n.resolved).count()
    }
}

/// The commands that set the cut's status (None clears it).
pub fn set_status(cut: &Cut, at: CutRef, status: Option<CutStatus>) -> Vec<Command> {
    ProdInfo {
        status,
        ..ProdInfo::of(cut)
    }
    .commands(cut, at)
}

/// The commands that hand the cut to `assignee` (empty = nobody).
pub fn set_assignee(cut: &Cut, at: CutRef, assignee: &str) -> Vec<Command> {
    ProdInfo {
        assignee: assignee.trim().to_string(),
        ..ProdInfo::of(cut)
    }
    .commands(cut, at)
}

/// The commands that add `note` (before the first note on a later frame,
/// so the list stays in frame order) and, with `retake`, set the cut's
/// status to Retake — one undo step together. The other notes keep every
/// field they have (D3). Empty when the stored notes are not a list, so
/// nothing overwrites what this build cannot read.
pub fn add_note(cut: &Cut, at: CutRef, note: RetakeNote, retake: bool) -> Vec<Command> {
    let Some(mut list) = stored_notes(cut) else {
        return Vec::new();
    };
    let i = list
        .iter()
        .position(|v| note_frame(v).is_some_and(|f| f > note.frame))
        .unwrap_or(list.len());
    list.insert(i, serde_json::to_value(&note).expect("a note serialises"));
    let mut out = vec![notes_command(cut, at, &list)];
    if retake {
        out.extend(set_status(cut, at, Some(CutStatus::Retake)));
    }
    out
}

/// The command that marks the cut's `nth` note (in [`notes_of`] order)
/// resolved or open again, keeping its other fields; empty when there is
/// no such note, it already says so, or the stored notes do not read.
pub fn set_note_resolved(cut: &Cut, at: CutRef, nth: usize, resolved: bool) -> Vec<Command> {
    let Some(mut list) = stored_notes(cut) else {
        return Vec::new();
    };
    let Some(v) = list.iter_mut().filter(|v| note_of(v).is_some()).nth(nth) else {
        return Vec::new();
    };
    if note_of(v).is_some_and(|n| n.resolved == resolved) {
        return Vec::new();
    }
    v["resolved"] = serde_json::Value::Bool(resolved);
    vec![notes_command(cut, at, &list)]
}

/// The cuts of `scene` the board shows under a filter, as indices in
/// order: `status` None = any status; `assignee` None = anyone, Some("")
/// = nobody yet.
pub fn filter_cuts(scene: &Scene, status: Option<CutStatus>, assignee: Option<&str>) -> Vec<usize> {
    scene
        .cuts
        .iter()
        .enumerate()
        .filter(|(_, c)| {
            let p = ProdInfo::of(c);
            status.is_none_or(|s| p.status == Some(s))
                && assignee.is_none_or(|a| p.assignee == a)
        })
        .map(|(i, _)| i)
        .collect()
}

/// The notes list as JSON values, so the ripple keeps any field a newer
/// build added (D3: what this build does not know round-trips).
fn note_values(text: &str) -> Option<Vec<serde_json::Value>> {
    serde_json::from_str(text).ok()
}

fn note_frame(v: &serde_json::Value) -> Option<u32> {
    v.get("frame")?.as_u64().and_then(|f| u32::try_from(f).ok())
}

/// D4: move every retake note in `text` to `shift(frame)` — the frame
/// ripple's share of `prod.notes`. None when the text does not read as
/// notes or no note moves (the key then stays byte for byte).
pub(crate) fn ripple_note_text(text: &str, shift: impl Fn(u32) -> u32) -> Option<String> {
    let mut notes = note_values(text)?;
    let mut moved = false;
    for n in &mut notes {
        let Some(f) = note_frame(n) else {
            continue;
        };
        let to = shift(f);
        if to != f {
            moved = true;
            n["frame"] = serde_json::json!(to);
        }
    }
    moved.then(|| serde_json::to_string(&notes).expect("notes serialise"))
}

/// The last frame a retake note of `cut` sits on (an insert must not push
/// it past the last frame number).
pub(crate) fn last_note_frame(cut: &Cut) -> Option<u32> {
    note_values(cut.meta.get(NOTES_KEY)?)?
        .iter()
        .filter_map(note_frame)
        .max()
}

/// D4: InsertFrames' share of the ripple — notes at or after `frame` move
/// `count` later. Returns what the inverse needs after its DeleteFrames to
/// put the notes back byte for byte (None when the delete does it alone).
pub(crate) fn ripple_notes_insert(cut: &mut Cut, at: CutRef, frame: u32, count: u32) -> Option<Command> {
    ripple_notes(
        cut,
        at,
        |f| if f >= frame { f + count } else { f },
        |f| delete_shift(f, frame, count),
    )
}

/// D4: DeleteFrames' share — notes after the range move `count` earlier
/// and notes inside it move to `frame` (kept, unlike memos: a note is
/// about the shot, not the drawing). Returns what the inverse needs after
/// its InsertFrames to put them back where they were.
pub(crate) fn ripple_notes_delete(cut: &mut Cut, at: CutRef, frame: u32, count: u32) -> Option<Command> {
    ripple_notes(
        cut,
        at,
        |f| delete_shift(f, frame, count),
        |f| if f >= frame { f + count } else { f },
    )
}

fn delete_shift(f: u32, frame: u32, count: u32) -> u32 {
    if f >= frame + count {
        f - count
    } else if f >= frame {
        frame
    } else {
        f
    }
}

fn ripple_notes(
    cut: &mut Cut,
    at: CutRef,
    there: impl Fn(u32) -> u32,
    back: impl Fn(u32) -> u32,
) -> Option<Command> {
    let orig = cut.meta.get(NOTES_KEY)?.clone();
    let moved = ripple_note_text(&orig, there);
    let now = moved.clone().unwrap_or_else(|| orig.clone());
    let undone = ripple_note_text(&now, back).unwrap_or_else(|| now.clone());
    if let Some(text) = moved {
        cut.meta.insert(NOTES_KEY.to_string(), text);
    }
    (undone != orig).then(|| Command::SetCutMeta {
        at,
        key: NOTES_KEY.to_string(),
        before: Some(undone),
        after: Some(orig),
    })
}

// ---- Episode: running time and frame map ------------------------------------

/// The episode's total running time: every cut of the scene, in frames.
pub fn trt_frames(scene: &Scene) -> u64 {
    scene.cuts.iter().map(|c| c.frame_count as u64).sum()
}

/// A running time as `MM:SS+FF` (minutes, seconds and the frames left
/// over): 30,972 frames at 24 fps is "21:30+12".
pub fn format_trt(frames: u64, fps: u32) -> String {
    let fps = fps.max(1) as u64;
    let secs = frames / fps;
    format!("{:02}:{:02}+{:02}", secs / 60, secs % 60, frames % fps)
}

/// Where the running time stands against the target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrtMark {
    Under,
    OnTarget,
    Over,
}

pub fn trt_mark(frames: u64, target: u64) -> TrtMark {
    match frames.cmp(&target) {
        std::cmp::Ordering::Less => TrtMark::Under,
        std::cmp::Ordering::Equal => TrtMark::OnTarget,
        std::cmp::Ordering::Greater => TrtMark::Over,
    }
}

/// The difference from the target: "+MM:SS+FF" over, "-MM:SS+FF" under,
/// "±00:00+00" on it.
pub fn format_trt_diff(frames: u64, target: u64, fps: u32) -> String {
    match trt_mark(frames, target) {
        TrtMark::Over => format!("+{}", format_trt(frames - target, fps)),
        TrtMark::Under => format!("-{}", format_trt(target - frames, fps)),
        TrtMark::OnTarget => format!("±{}", format_trt(0, fps)),
    }
}

/// Read a running time typed as "MM:SS+FF", "MM:SS" or plain frames.
/// None when it does not read (or the frames are not under one second).
pub fn parse_trt(text: &str, fps: u32) -> Option<u64> {
    let fps = fps.max(1) as u64;
    let t = text.trim();
    if let Ok(n) = t.parse::<u64>() {
        return Some(n);
    }
    let (time, ff) = match t.split_once('+') {
        Some((a, b)) => (a.trim(), b.trim().parse::<u64>().ok()?),
        None => (t, 0),
    };
    let (mm, ss) = time.split_once(':')?;
    let (mm, ss) = (mm.trim().parse::<u64>().ok()?, ss.trim().parse::<u64>().ok()?);
    (ss < 60 && ff < fps).then(|| (mm * 60 + ss) * fps + ff)
}

/// The project's episode target in frames (None when unset or unread).
pub fn episode_target(project: &Project) -> Option<u64> {
    project
        .app_meta
        .get(TARGET_KEY)
        .and_then(|v| serde_json::from_str::<u64>(v).ok())
}

/// Every frame of the episode, in order: (the cut's index in the scene,
/// the frame within that cut).
pub fn episode_frame_map(scene: &Scene) -> Vec<(usize, u32)> {
    scene
        .cuts
        .iter()
        .enumerate()
        .flat_map(|(i, c)| (0..c.frame_count).map(move |f| (i, f)))
        .collect()
}

/// Each cut's first frame in the episode, in order.
pub fn cut_starts(scene: &Scene) -> Vec<u64> {
    let mut at = 0u64;
    scene
        .cuts
        .iter()
        .map(|c| {
            let start = at;
            at += c.frame_count as u64;
            start
        })
        .collect()
}

/// Episode frame `frame` as (cut index, frame within the cut), without the
/// whole map; None past the episode's end.
pub fn locate(scene: &Scene, frame: u64) -> Option<(usize, u32)> {
    let mut start = 0u64;
    for (i, c) in scene.cuts.iter().enumerate() {
        let end = start + c.frame_count as u64;
        if frame < end {
            return Some((i, (frame - start) as u32));
        }
        start = end;
    }
    None
}

// ---- EDL ---------------------------------------------------------------------

/// One CMX3600 event line: number, reel (AX: a file, not a tape), track,
/// a cut (C), then source in/out and record in/out.
fn edl_event(n: usize, track: &str, src_out: u32, rec_in: u32, rec_out: u32, fps: u32) -> String {
    let tc = |f: u32| deliver::timecode(f, fps);
    format!(
        "{n:03}  {:<8} {track:<5} C        {} {} {} {}",
        "AX",
        tc(0),
        tc(src_out),
        tc(rec_in),
        tc(rec_out)
    )
}

/// The episode as a CMX3600 EDL for the online editor: non-drop, record
/// starting at 01:00:00:00, one video event per cut in order (plus an
/// audio event, AA, when the cut sounds). Each event's source is the cut's
/// render, named as the render queue names it (`pattern` without its frame
/// token, plus `ext`; the audio's is the mixdown WAV beside it), from
/// 00:00:00:00, and carries a `* FROM CLIP NAME:` line.
pub fn edl_cmx3600(project: &str, scene: &Scene, fps: u32, pattern: &str, ext: &str) -> String {
    let fps = fps.max(1);
    let title = deliver::sanitize(&format!("{project} {}", scene.name));
    let mut out = format!("TITLE: {title}\nFCM: NON-DROP FRAME\n");
    let record = 3600 * fps;
    let mut n = 0;
    let mut at = record;
    for cut in &scene.cuts {
        let names = Names {
            project: project.to_string(),
            scene: scene.name.clone(),
            cut: cut.name.clone(),
        };
        let len = cut.frame_count;
        let (rec_in, rec_out) = (at, at.saturating_add(len));
        n += 1;
        out.push('\n');
        out.push_str(&edl_event(n, "V", len, rec_in, rec_out, fps));
        out.push_str(&format!(
            "\n* FROM CLIP NAME: {}\n",
            deliver::video_name(pattern, &names, ext)
        ));
        if crate::audio::has_sound(cut) {
            n += 1;
            out.push('\n');
            out.push_str(&edl_event(n, "AA", len, rec_in, rec_out, fps));
            out.push_str(&format!(
                "\n* FROM CLIP NAME: {}\n",
                deliver::video_name(pattern, &names, "wav")
            ));
        }
        at = rec_out;
    }
    out
}

// ---- Colour designation sheet (色指定表) ---------------------------------------

/// One chip as the colour designation sheet lists it.
#[derive(Debug, Clone, PartialEq)]
pub struct SheetChip {
    pub name: String,
    /// The cel layer it paints (a role's label, "kage (shadow)").
    pub role: String,
    /// The chip as picked (straight RGBA bytes).
    pub color: [u8; 4],
}

/// A palette variant (day, evening, night): one colour per chip, in the
/// chips' order; a chip it has no colour for keeps the model's.
#[derive(Debug, Clone, PartialEq)]
pub struct SheetVariant {
    pub name: String,
    pub colors: Vec<[u8; 4]>,
}

/// A character's palette as the sheet lists it (the app hands its own
/// palettes over as this).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SheetPalette {
    pub name: String,
    pub chips: Vec<SheetChip>,
    pub variants: Vec<SheetVariant>,
}

impl SheetPalette {
    /// The colour of chip `i` in column `col` (0 = the model, then each
    /// variant).
    pub fn color(&self, i: usize, col: usize) -> [u8; 4] {
        let model = self.chips[i].color;
        match col {
            0 => model,
            v => self
                .variants
                .get(v - 1)
                .and_then(|v| v.colors.get(i).copied())
                .unwrap_or(model),
        }
    }
}

/// A pixel rectangle on the sheet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

impl Rect {
    pub fn center(&self) -> (u32, u32) {
        (self.x + self.w / 2, self.y + self.h / 2)
    }
}

/// One chip's row: its name and role, then a swatch and its hex per colour
/// column (the model, then each variant).
#[derive(Debug, Clone, PartialEq)]
pub struct ChipRow {
    pub name: Rect,
    pub role: Rect,
    pub swatches: Vec<Rect>,
    pub hex: Vec<Rect>,
}

/// Where everything on a colour designation sheet sits.
#[derive(Debug, Clone, PartialEq)]
pub struct SheetLayout {
    pub width: u32,
    pub height: u32,
    pub title: Rect,
    /// The column heads: chip, role, then "model" and each variant's name.
    pub heads: Vec<Rect>,
    pub rows: Vec<ChipRow>,
}

pub const SHEET_MARGIN: u32 = 16;
pub const SHEET_TITLE_H: u32 = 36;
pub const SHEET_HEAD_H: u32 = 24;
pub const SHEET_ROW_H: u32 = 40;
pub const SHEET_NAME_W: u32 = 160;
pub const SHEET_ROLE_W: u32 = 140;
pub const SHEET_SWATCH: u32 = 32;
pub const SHEET_HEX_W: u32 = 88;
/// A colour column: swatch, gap, hex, gap.
pub const SHEET_COLOR_W: u32 = SHEET_SWATCH + 8 + SHEET_HEX_W + 8;
const LABEL_H: u32 = 24;

/// The sheet's layout for `palette` (fixed metrics, so the sheet of a
/// palette is always the same picture).
pub fn color_sheet_layout(palette: &SheetPalette) -> SheetLayout {
    let m = SHEET_MARGIN;
    let cols = 1 + palette.variants.len() as u32;
    let width = 2 * m + SHEET_NAME_W + SHEET_ROLE_W + SHEET_COLOR_W * cols;
    let height = 2 * m + SHEET_TITLE_H + SHEET_HEAD_H + SHEET_ROW_H * palette.chips.len() as u32;
    let color_x = |c: u32| m + SHEET_NAME_W + SHEET_ROLE_W + SHEET_COLOR_W * c;
    let title = Rect {
        x: m,
        y: m,
        w: width - 2 * m,
        h: SHEET_TITLE_H - 8,
    };
    let head_y = m + SHEET_TITLE_H;
    let mut heads = vec![
        Rect {
            x: m,
            y: head_y,
            w: SHEET_NAME_W - 8,
            h: SHEET_HEAD_H - 4,
        },
        Rect {
            x: m + SHEET_NAME_W,
            y: head_y,
            w: SHEET_ROLE_W - 8,
            h: SHEET_HEAD_H - 4,
        },
    ];
    heads.extend((0..cols).map(|c| Rect {
        x: color_x(c),
        y: head_y,
        w: SHEET_COLOR_W - 8,
        h: SHEET_HEAD_H - 4,
    }));
    let rows = (0..palette.chips.len() as u32)
        .map(|i| {
            let y = head_y + SHEET_HEAD_H + SHEET_ROW_H * i;
            let label_y = y + (SHEET_ROW_H - LABEL_H) / 2;
            ChipRow {
                name: Rect {
                    x: m,
                    y: label_y,
                    w: SHEET_NAME_W - 8,
                    h: LABEL_H,
                },
                role: Rect {
                    x: m + SHEET_NAME_W,
                    y: label_y,
                    w: SHEET_ROLE_W - 8,
                    h: LABEL_H,
                },
                swatches: (0..cols)
                    .map(|c| Rect {
                        x: color_x(c),
                        y: y + (SHEET_ROW_H - SHEET_SWATCH) / 2,
                        w: SHEET_SWATCH,
                        h: SHEET_SWATCH,
                    })
                    .collect(),
                hex: (0..cols)
                    .map(|c| Rect {
                        x: color_x(c) + SHEET_SWATCH + 8,
                        y: label_y,
                        w: SHEET_HEX_W,
                        h: LABEL_H,
                    })
                    .collect(),
            }
        })
        .collect();
    SheetLayout {
        width,
        height,
        title,
        heads,
        rows,
    }
}

/// A chip as the sheet writes it: "#RRGGBB", with the alpha when it is not
/// opaque.
pub fn hex(c: [u8; 4]) -> String {
    if c[3] == 255 {
        format!("#{:02X}{:02X}{:02X}", c[0], c[1], c[2])
    } else {
        format!("#{:02X}{:02X}{:02X}{:02X}", c[0], c[1], c[2], c[3])
    }
}

/// The sheet's ink, paper and rules (stored values; the same bytes under
/// either colour law).
const INK: [u8; 4] = [24, 24, 28, 255];
const RULE: [f32; 4] = [0.82, 0.82, 0.84, 1.0];
const FRAME: [f32; 4] = [0.35, 0.35, 0.38, 1.0];

/// `text` set in `rect` (clipped to it), vertically centred.
fn put_text(img: &mut Image, rect: Rect, text: &str, px: f32) {
    let line = text::raster_line(Face::Sans, text, px, INK);
    let dy = rect.h.saturating_sub(line.h) / 2;
    for y in 0..line.h.min(rect.h) {
        for x in 0..line.w.min(rect.w) {
            let s = line.pixel(x, y);
            if s[3] <= 0.0 {
                continue;
            }
            let (ox, oy) = (rect.x + x, rect.y + dy + y);
            if ox >= img.w || oy >= img.h {
                continue;
            }
            let d = img.pixel(ox, oy);
            let keep = 1.0 - s[3];
            img.set_pixel(ox, oy, [0, 1, 2, 3].map(|k| s[k] + d[k] * keep));
        }
    }
}

fn fill(img: &mut Image, r: Rect, p: [f32; 4]) {
    for y in r.y..(r.y + r.h).min(img.h) {
        for x in r.x..(r.x + r.w).min(img.w) {
            img.set_pixel(x, y, p);
        }
    }
}

/// The colour designation sheet (色指定表) of one palette: a title, the
/// column heads, then a row per chip — its name, the role it paints, and a
/// swatch with its hex for the model and for each variant. Text is set
/// with [`crate::text`]; each swatch is the chip exactly as an Srgb
/// project stores it (D8: `c/255`, premultiplied), so the PNG's bytes are
/// the chip's. Opaque white paper.
pub fn render_color_sheet(palette: &SheetPalette) -> Image {
    let l = color_sheet_layout(palette);
    let mut img = Image::from_px(l.width, l.height, vec![1.0; l.width as usize * l.height as usize * 4]);
    put_text(
        &mut img,
        l.title,
        &format!("Color sheet — {}", palette.name),
        20.0,
    );
    let mut heads = vec!["chip".to_string(), "color name".to_string(), "model".to_string()];
    heads.extend(palette.variants.iter().map(|v| v.name.clone()));
    for (r, t) in l.heads.iter().zip(&heads) {
        put_text(&mut img, *r, t, 13.0);
    }
    for (i, row) in l.rows.iter().enumerate() {
        // A rule over each row.
        let y = row.swatches[0].y - (SHEET_ROW_H - SHEET_SWATCH) / 2;
        fill(
            &mut img,
            Rect {
                x: SHEET_MARGIN,
                y,
                w: l.width - 2 * SHEET_MARGIN,
                h: 1,
            },
            RULE,
        );
        let chip = &palette.chips[i];
        put_text(&mut img, row.name, &chip.name, 14.0);
        put_text(&mut img, row.role, &chip.role, 14.0);
        for (c, (sw, hx)) in row.swatches.iter().zip(&row.hex).enumerate() {
            let color = palette.color(i, c);
            // A frame just outside the swatch, so white chips show.
            fill(
                &mut img,
                Rect {
                    x: sw.x - 1,
                    y: sw.y - 1,
                    w: sw.w + 2,
                    h: sw.h + 2,
                },
                FRAME,
            );
            let s = ColorLaw::Srgb.encode_rgba(color);
            fill(&mut img, *sw, [s[0] * s[3], s[1] * s[3], s[2] * s[3], s[3]]);
            put_text(&mut img, *hx, &hex(color), 13.0);
        }
    }
    img
}

// ---- Animatic: cuts from storyboard panels ------------------------------------

/// One storyboard panel: a drawing exposure on the BOARD column, from its
/// key to the next key (or the cut's end), with the cut number it belongs
/// to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Panel {
    pub drawing: DrawingId,
    pub start: u32,
    pub frames: u32,
    pub cut: u32,
}

/// The panels of `column` in `cut`, in order. A panel's cut number is its
/// drawing's (`DrawingDef::cut`, set from the cell's menu), else its order
/// (the first panel is 1). An empty key ends a panel and starts none.
pub fn panels(cut: &Cut, column: ColumnId) -> Vec<Panel> {
    let Some(col) = cut.xsheet.column(column) else {
        return Vec::new();
    };
    let keys: Vec<(u32, Exposure)> = col.keys().filter(|(f, _)| *f < cut.frame_count).collect();
    let mut out = Vec::new();
    for (i, (start, e)) in keys.iter().enumerate() {
        let Exposure::Drawing(d) = e else {
            continue;
        };
        let end = keys.get(i + 1).map_or(cut.frame_count, |(f, _)| *f);
        let order = out.len() as u32 + 1;
        out.push(Panel {
            drawing: *d,
            start: *start,
            frames: end - start,
            cut: cut.drawing_def(*d).cut.unwrap_or(order),
        });
    }
    out
}

/// "Make cuts from panels": for each distinct cut number of `column`'s
/// panels, in ascending order, a new cut right after the animatic's, as
/// long as its panels together, with the panel pictures exposed in order
/// on a Ref column (BOARD) and an empty cel column A beside it. Each is
/// named the app's way, `CUT{number:02}`, moved on past any name the scene
/// already has (a second run makes CUT03, CUT04 ...), so names never
/// repeat. The pictures are copies with fresh ids (ids are project-wide);
/// the counter is the only thing this touches. The AddCut commands go to
/// `Engine::apply` together: one undo step.
pub fn cuts_from_panels(
    project: &mut Project,
    at: CutRef,
    column: ColumnId,
) -> Result<Vec<Command>, String> {
    let scene = project.scene(at.scene).ok_or("the scene is gone")?;
    let index = scene
        .cuts
        .iter()
        .position(|c| c.id == at.cut)
        .ok_or("the shot is gone")?;
    let mut used: std::collections::BTreeSet<String> =
        scene.cuts.iter().map(|c| c.name.clone()).collect();
    let source = scene.cuts[index].clone();
    let panels = panels(&source, column);
    if panels.is_empty() {
        return Err("the track has no panels (no drawing exposures)".into());
    }
    let mut groups: BTreeMap<u32, Vec<Panel>> = BTreeMap::new();
    for p in &panels {
        groups.entry(p.cut).or_default().push(*p);
    }
    let mut cmds = Vec::new();
    // The lowest number the next name may take: names rise with the groups.
    let mut next = 1;
    for (k, (number, group)) in groups.into_iter().enumerate() {
        let frames: u32 = group.iter().map(|p| p.frames).sum();
        let mut n = number.max(next);
        while used.contains(&format!("CUT{n:02}")) {
            n += 1;
        }
        next = n + 1;
        let name = format!("CUT{n:02}");
        used.insert(name.clone());
        let mut cut = Cut::new(CutId(project.alloc_id()), name, frames);
        let mut board = Column::new(ColumnId(project.alloc_id()), "BOARD");
        let mut copies: BTreeMap<DrawingId, DrawingId> = BTreeMap::new();
        let mut t = 0;
        for p in &group {
            let copy = match copies.get(&p.drawing) {
                Some(c) => *c,
                None => {
                    let src = source
                        .drawing(p.drawing)
                        .ok_or_else(|| format!("panel drawing {} is gone", p.drawing))?;
                    let mut d = src.clone();
                    d.id = DrawingId(project.alloc_id());
                    for l in &mut d.layers {
                        l.id = LayerId(project.alloc_id());
                    }
                    copies.insert(p.drawing, d.id);
                    cut.drawings.push(d);
                    cut.drawings.last().expect("pushed").id
                }
            };
            board.set_key(t, Exposure::Drawing(copy));
            t += p.frames;
        }
        cut.set_column_def(
            board.id,
            ColumnDef {
                kind: ColumnKind::Ref,
                ..ColumnDef::default()
            },
        );
        cut.xsheet.columns.push(board);
        cut.xsheet
            .columns
            .push(Column::new(ColumnId(project.alloc_id()), "A"));
        cmds.push(Command::AddCut {
            scene: at.scene,
            index: index + 1 + k,
            cut: Box::new(cut),
        });
    }
    Ok(cmds)
}
