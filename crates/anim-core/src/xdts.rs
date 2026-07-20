//! XDTS (exchangeDigitalTimeSheet) interop — the digital exposure-sheet
//! exchange format Toei drove and OpenToonz/CSP speak. Timing only: cell
//! NUMBERS on tracks, no artwork. Format pinned against the OpenToonz
//! implementation (toonz/sources/toonz/xdtsio.{h,cpp}):
//!
//! ```text
//! exchangeDigitalTimeSheet Save Data
//! {"header":{"cut":"1","scene":"1"},
//!  "timeTables":[{"name":..., "duration":N,
//!    "fields":[{"fieldId":0,"tracks":[{"trackNo":0,
//!      "frames":[{"frame":0,"data":[{"id":0,"values":["1"]}]}, ...]}]}],
//!    "timeTableHeaders":[{"fieldId":0,"names":[...]}]}],
//!  "version":5}
//! ```
//!
//! fieldId 0 = CELL. Values are strings: a cell number ("1", "2a"…) or
//! "SYMBOL_NULL_CELL" for explicit emptiness — exactly our Exposure
//! semantics (keys hold until the next key). Pure text↔struct functions:
//! no file I/O here (headless-engine law); the app owns dialogs and disk.

use crate::model::Cut;
use crate::xsheet::Exposure;

pub const XDTS_SIGNATURE: &str = "exchangeDigitalTimeSheet Save Data";
const NULL_CELL: &str = "SYMBOL_NULL_CELL";
/// The version OpenToonz writes (Ver_2018_11_29).
const VERSION: i64 = 5;

/// One imported track: keys as (frame, Some(cell-number) | None=empty).
pub struct XdtsColumn {
    pub name: String,
    pub keys: Vec<(u32, Option<String>)>,
}

/// A parsed sheet (first timeTable only — one cut per file in practice).
pub struct XdtsSheet {
    pub name: String,
    pub duration: u32,
    pub columns: Vec<XdtsColumn>,
}

/// Trailing digits of a scene/cut name ("SC02" → "2", "CUT01" → "1"),
/// matching the header's \d{1,4} contract; "1" when there are none.
fn trailing_number(name: &str) -> String {
    let digits: String = name
        .chars()
        .rev()
        .take_while(|c| c.is_ascii_digit())
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    let trimmed = digits.trim_start_matches('0');
    if trimmed.is_empty() {
        if digits.is_empty() { "1".into() } else { "0".into() }
    } else {
        trimmed.chars().take(4).collect()
    }
}

/// Serialize a cut's drawing-column timing as an .xdts file's full text.
/// Cell numbers are the drawing's LIBRARY position (1-based) — stable,
/// numeric, and what a paper sheet would say; the receiving tool maps
/// numbers to its own cels anyway.
pub fn export(cut: &Cut, scene_name: &str) -> String {
    use serde_json::json;

    let cell_number = |d: crate::ids::DrawingId| -> String {
        cut.drawings
            .iter()
            .position(|dr| dr.id == d)
            .map(|i| (i + 1).to_string())
            .unwrap_or_else(|| "1".into())
    };

    let mut tracks = Vec::new();
    for (track_no, col) in cut.xsheet.columns.iter().enumerate() {
        let frames: Vec<serde_json::Value> = col
            .keys()
            .map(|(frame, exp)| {
                let value = match exp {
                    Exposure::Drawing(d) => cell_number(d),
                    Exposure::Empty => NULL_CELL.into(),
                };
                json!({
                    "frame": frame,
                    "data": [{ "id": 0, "values": [value] }],
                })
            })
            .collect();
        tracks.push(json!({ "trackNo": track_no, "frames": frames }));
    }
    let names: Vec<&str> = cut.xsheet.columns.iter().map(|c| c.name.as_str()).collect();

    let doc = json!({
        "header": {
            "cut": trailing_number(&cut.name),
            "scene": trailing_number(scene_name),
        },
        "timeTables": [{
            "name": cut.name,
            "duration": cut.frame_count,
            "fields": [{ "fieldId": 0, "tracks": tracks }],
            "timeTableHeaders": [{ "fieldId": 0, "names": names }],
        }],
        "version": VERSION,
    });
    format!("{XDTS_SIGNATURE}\n{doc}")
}

fn get<'a>(v: &'a serde_json::Value, key: &str) -> Result<&'a serde_json::Value, String> {
    v.get(key).ok_or_else(|| format!("xdts: missing '{key}'"))
}

/// Parse .xdts text into a neutral sheet (fieldId 0 / CELL tracks only —
/// dialog and camerawork fields are other tools' annotations, skipped).
/// Tolerant of a missing signature line; strict about the JSON shape.
pub fn parse(text: &str) -> Result<XdtsSheet, String> {
    let json_text = match text.split_once('\n') {
        Some((first, rest)) if first.trim() == XDTS_SIGNATURE => rest,
        _ => text,
    };
    let doc: serde_json::Value =
        serde_json::from_str(json_text).map_err(|e| format!("xdts: {e}"))?;

    let table = get(&doc, "timeTables")?
        .as_array()
        .and_then(|a| a.first())
        .ok_or("xdts: no timeTables")?;
    let name = get(table, "name")?.as_str().unwrap_or("XDTS").to_string();
    let duration = get(table, "duration")?
        .as_u64()
        .ok_or("xdts: bad duration")? as u32;

    // Track names from the CELL header, when present.
    let header_names: Vec<String> = table
        .get("timeTableHeaders")
        .and_then(|h| h.as_array())
        .and_then(|hs| {
            hs.iter()
                .find(|h| h.get("fieldId").and_then(|f| f.as_i64()) == Some(0))
        })
        .and_then(|h| h.get("names"))
        .and_then(|n| n.as_array())
        .map(|n| {
            n.iter()
                .map(|s| s.as_str().unwrap_or_default().to_string())
                .collect()
        })
        .unwrap_or_default();

    let cell_field = get(table, "fields")?
        .as_array()
        .and_then(|fs| {
            fs.iter()
                .find(|f| f.get("fieldId").and_then(|i| i.as_i64()) == Some(0))
        })
        .ok_or("xdts: no CELL field")?;

    let mut columns = Vec::new();
    for track in cell_field
        .get("tracks")
        .and_then(|t| t.as_array())
        .ok_or("xdts: no tracks")?
    {
        let track_no = track.get("trackNo").and_then(|n| n.as_u64()).unwrap_or(0) as usize;
        let mut keys = Vec::new();
        for frame_item in track
            .get("frames")
            .and_then(|f| f.as_array())
            .unwrap_or(&Vec::new())
        {
            let frame = frame_item
                .get("frame")
                .and_then(|f| f.as_u64())
                .ok_or("xdts: frame item without 'frame'")? as u32;
            // First data item's first value is the cell; tick symbols and
            // extra data items are annotations we don't model.
            let value = frame_item
                .get("data")
                .and_then(|d| d.as_array())
                .and_then(|d| d.first())
                .and_then(|d| d.get("values"))
                .and_then(|v| v.as_array())
                .and_then(|v| v.first())
                .and_then(|v| v.as_str())
                .unwrap_or(NULL_CELL);
            match value {
                NULL_CELL => keys.push((frame, None)),
                "SYMBOL_TICK_1" | "SYMBOL_TICK_2" | "SYMBOL_HYPHEN" => {}
                cell => keys.push((frame, Some(cell.to_string()))),
            }
        }
        keys.sort_by_key(|(f, _)| *f);
        let name = header_names
            .get(track_no)
            .filter(|n| !n.is_empty())
            .cloned()
            .unwrap_or_else(|| format!("T{}", track_no + 1));
        columns.push(XdtsColumn { name, keys });
    }
    if duration == 0 {
        return Err("xdts: zero duration".into());
    }
    Ok(XdtsSheet {
        name,
        duration,
        columns,
    })
}
