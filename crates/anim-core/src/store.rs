//! SQLite project persistence.
//!
//! One project = one SQLite file. Saves are transactional (autosave and crash
//! recovery come free), the format is inspectable with any sqlite tool, and
//! schema_version gates forward migration. M1 does full-rewrite saves;
//! incremental dirty-row saves are an M4+ optimization.

use std::path::Path;

use rusqlite::Connection;

use crate::error::{EngineError, Result};
use crate::graph::{Graph, Node, NodeKind};
use crate::ids::*;
use crate::model::{Cut, Drawing, Project, Scene};
use crate::raster::{RasterLayer, TileData};
use crate::xsheet::{Column, Exposure, XSheet};

const SCHEMA_VERSION: i64 = 4;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS meta(key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS scenes(
    id INTEGER PRIMARY KEY, name TEXT NOT NULL, ord INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS cuts(
    id INTEGER PRIMARY KEY, scene_id INTEGER NOT NULL, name TEXT NOT NULL,
    frame_count INTEGER NOT NULL, output_node INTEGER, ord INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS drawings(
    id INTEGER PRIMARY KEY, cut_id INTEGER NOT NULL, name TEXT NOT NULL,
    payload TEXT NOT NULL, ord INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS columns(
    id INTEGER PRIMARY KEY, cut_id INTEGER NOT NULL, name TEXT NOT NULL,
    ord INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS cells(
    column_id INTEGER NOT NULL, frame INTEGER NOT NULL,
    exposure TEXT NOT NULL, drawing_id INTEGER,
    PRIMARY KEY(column_id, frame));
CREATE TABLE IF NOT EXISTS nodes(
    id INTEGER PRIMARY KEY, cut_id INTEGER NOT NULL, kind TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS links(
    cut_id INTEGER NOT NULL, to_node INTEGER NOT NULL, to_pin INTEGER NOT NULL,
    from_node INTEGER NOT NULL, from_pin INTEGER NOT NULL,
    PRIMARY KEY(to_node, to_pin));
CREATE TABLE IF NOT EXISTS raster_layers(
    drawing_id INTEGER PRIMARY KEY, opacity REAL NOT NULL);
CREATE TABLE IF NOT EXISTS tiles(
    drawing_id INTEGER NOT NULL, tx INTEGER NOT NULL, ty INTEGER NOT NULL,
    bytes BLOB NOT NULL, PRIMARY KEY(drawing_id, tx, ty));
";

pub fn save(project: &Project, path: &Path) -> Result<()> {
    let mut conn = Connection::open(path)?;
    let tx = conn.transaction()?;
    tx.execute_batch(SCHEMA)?;
    // Full rewrite: clear all rows, reinsert.
    tx.execute_batch(
        "DELETE FROM meta; DELETE FROM scenes; DELETE FROM cuts;
         DELETE FROM drawings; DELETE FROM columns; DELETE FROM cells;
         DELETE FROM nodes; DELETE FROM links;
         DELETE FROM raster_layers; DELETE FROM tiles;",
    )?;

    let mut put_meta = tx.prepare("INSERT INTO meta(key, value) VALUES (?1, ?2)")?;
    put_meta.execute(("schema_version", SCHEMA_VERSION.to_string()))?;
    put_meta.execute(("name", project.name.clone()))?;
    put_meta.execute(("fps", project.fps.to_string()))?;
    put_meta.execute(("width", project.width.to_string()))?;
    put_meta.execute(("height", project.height.to_string()))?;
    put_meta.execute(("dpi", project.dpi.to_string()))?;
    put_meta.execute(("next_id", project.next_id.to_string()))?;
    drop(put_meta);

    {
        let mut ins_scene =
            tx.prepare("INSERT INTO scenes(id, name, ord) VALUES (?1, ?2, ?3)")?;
        let mut ins_cut = tx.prepare(
            "INSERT INTO cuts(id, scene_id, name, frame_count, output_node, ord)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )?;
        let mut ins_drawing = tx.prepare(
            "INSERT INTO drawings(id, cut_id, name, payload, ord) VALUES (?1, ?2, ?3, ?4, ?5)",
        )?;
        let mut ins_column =
            tx.prepare("INSERT INTO columns(id, cut_id, name, ord) VALUES (?1, ?2, ?3, ?4)")?;
        let mut ins_cell = tx.prepare(
            "INSERT INTO cells(column_id, frame, exposure, drawing_id) VALUES (?1, ?2, ?3, ?4)",
        )?;
        let mut ins_node =
            tx.prepare("INSERT INTO nodes(id, cut_id, kind) VALUES (?1, ?2, ?3)")?;
        let mut ins_link = tx.prepare(
            "INSERT INTO links(cut_id, to_node, to_pin, from_node, from_pin)
             VALUES (?1, ?2, ?3, ?4, ?5)",
        )?;
        let mut ins_raster =
            tx.prepare("INSERT INTO raster_layers(drawing_id, opacity) VALUES (?1, ?2)")?;
        let mut ins_tile = tx.prepare(
            "INSERT INTO tiles(drawing_id, tx, ty, bytes) VALUES (?1, ?2, ?3, ?4)",
        )?;

        for (s_ord, scene) in project.scenes.iter().enumerate() {
            ins_scene.execute((scene.id.0 as i64, &scene.name, s_ord as i64))?;
            for (c_ord, cut) in scene.cuts.iter().enumerate() {
                ins_cut.execute((
                    cut.id.0 as i64,
                    scene.id.0 as i64,
                    &cut.name,
                    cut.frame_count as i64,
                    cut.graph.output.map(|n| n.0 as i64),
                    c_ord as i64,
                ))?;
                for (d_ord, d) in cut.drawings.iter().enumerate() {
                    let payload = serde_json::to_string(&d.strokes)?;
                    ins_drawing.execute((
                        d.id.0 as i64,
                        cut.id.0 as i64,
                        &d.name,
                        payload,
                        d_ord as i64,
                    ))?;
                    if let Some(raster) = &d.raster {
                        ins_raster.execute((d.id.0 as i64, raster.opacity as f64))?;
                        for ((tx_c, ty_c), tile) in &raster.tiles {
                            ins_tile.execute((
                                d.id.0 as i64,
                                *tx_c as i64,
                                *ty_c as i64,
                                tile.as_bytes(),
                            ))?;
                        }
                    }
                }
                for (col_ord, col) in cut.xsheet.columns.iter().enumerate() {
                    ins_column.execute((col.id.0 as i64, cut.id.0 as i64, &col.name, col_ord as i64))?;
                    for (frame, exposure) in col.keys() {
                        let (kind, drawing_id) = match exposure {
                            Exposure::Drawing(d) => ("drawing", Some(d.0 as i64)),
                            Exposure::Empty => ("empty", None),
                        };
                        ins_cell.execute((col.id.0 as i64, frame as i64, kind, drawing_id))?;
                    }
                }
                for node in cut.graph.nodes() {
                    let kind_json = serde_json::to_string(&node.kind)?;
                    ins_node.execute((node.id.0 as i64, cut.id.0 as i64, kind_json))?;
                    for (pin, input) in node.inputs.iter().enumerate() {
                        if let Some((from, from_pin)) = input {
                            ins_link.execute((
                                cut.id.0 as i64,
                                node.id.0 as i64,
                                pin as i64,
                                from.0 as i64,
                                *from_pin as i64,
                            ))?;
                        }
                    }
                }
            }
        }
    }

    tx.commit()?;
    Ok(())
}

pub fn load(path: &Path) -> Result<Project> {
    let conn = Connection::open(path)?;

    let get_meta = |key: &str| -> Result<String> {
        conn.query_row("SELECT value FROM meta WHERE key = ?1", (key,), |r| {
            r.get::<_, String>(0)
        })
        .map_err(|_| EngineError::Corrupt(format!("missing meta key '{key}'")))
    };
    // Optional key: absent in older schema versions -> caller's default.
    let get_meta_opt = |key: &str| -> Option<String> {
        conn.query_row("SELECT value FROM meta WHERE key = ?1", (key,), |r| {
            r.get::<_, String>(0)
        })
        .ok()
    };

    let version: i64 = get_meta("schema_version")?
        .parse()
        .map_err(|_| EngineError::Corrupt("bad schema_version".into()))?;
    // Accept this version and older (fields added in later versions default);
    // reject only future/unknown versions we can't understand.
    if !(1..=SCHEMA_VERSION).contains(&version) {
        return Err(EngineError::SchemaVersion(version));
    }

    let mut project = Project::new(get_meta("name")?);
    project.fps = get_meta("fps")?
        .parse()
        .map_err(|_| EngineError::Corrupt("bad fps".into()))?;
    project.next_id = get_meta("next_id")?
        .parse()
        .map_err(|_| EngineError::Corrupt("bad next_id".into()))?;
    // Resolution: added in schema v3; default for older files.
    if let Some(w) = get_meta_opt("width").and_then(|s| s.parse().ok()) {
        project.width = w;
    }
    if let Some(h) = get_meta_opt("height").and_then(|s| s.parse().ok()) {
        project.height = h;
    }
    if let Some(d) = get_meta_opt("dpi").and_then(|s| s.parse().ok()) {
        project.dpi = d;
    }

    // Scenes
    let mut stmt = conn.prepare("SELECT id, name FROM scenes ORDER BY ord")?;
    let scenes: Vec<(i64, String)> = stmt
        .query_map((), |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<std::result::Result<_, _>>()?;

    for (scene_id, scene_name) in scenes {
        let mut scene = Scene {
            id: SceneId(scene_id as u64),
            name: scene_name,
            cuts: Vec::new(),
        };

        let mut stmt = conn.prepare(
            "SELECT id, name, frame_count, output_node FROM cuts
             WHERE scene_id = ?1 ORDER BY ord",
        )?;
        let cuts: Vec<(i64, String, i64, Option<i64>)> = stmt
            .query_map((scene_id,), |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            })?
            .collect::<std::result::Result<_, _>>()?;

        for (cut_id, cut_name, frame_count, output_node) in cuts {
            let mut cut = Cut {
                id: CutId(cut_id as u64),
                name: cut_name,
                frame_count: frame_count as u32,
                drawings: Vec::new(),
                xsheet: XSheet::default(),
                graph: Graph::default(),
            };

            let mut stmt = conn
                .prepare("SELECT id, name, payload FROM drawings WHERE cut_id = ?1 ORDER BY ord")?;
            let rows: Vec<(i64, String, String)> = stmt
                .query_map((cut_id,), |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
                .collect::<std::result::Result<_, _>>()?;
            for (id, name, payload) in rows {
                // Raster layer for this drawing, if any.
                let opacity: Option<f64> = conn
                    .query_row(
                        "SELECT opacity FROM raster_layers WHERE drawing_id = ?1",
                        (id,),
                        |r| r.get(0),
                    )
                    .ok();
                let raster = if let Some(opacity) = opacity {
                    let mut layer = RasterLayer {
                        opacity: opacity as f32,
                        ..RasterLayer::empty()
                    };
                    let mut tstmt = conn.prepare(
                        "SELECT tx, ty, bytes FROM tiles WHERE drawing_id = ?1",
                    )?;
                    let tile_rows: Vec<(i64, i64, Vec<u8>)> = tstmt
                        .query_map((id,), |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
                        .collect::<std::result::Result<_, _>>()?;
                    for (tx_c, ty_c, bytes) in tile_rows {
                        let tile = TileData::from_bytes(&bytes).ok_or_else(|| {
                            EngineError::Corrupt(format!(
                                "tile ({tx_c},{ty_c}) of drawing {id} has wrong byte length"
                            ))
                        })?;
                        layer
                            .tiles
                            .insert((tx_c as i32, ty_c as i32), std::sync::Arc::new(tile));
                    }
                    Some(layer)
                } else {
                    None
                };
                cut.drawings.push(Drawing {
                    id: DrawingId(id as u64),
                    name,
                    strokes: serde_json::from_str(&payload)?,
                    raster,
                });
            }

            let mut stmt =
                conn.prepare("SELECT id, name FROM columns WHERE cut_id = ?1 ORDER BY ord")?;
            let cols: Vec<(i64, String)> = stmt
                .query_map((cut_id,), |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<std::result::Result<_, _>>()?;
            for (col_id, col_name) in cols {
                let mut column = Column::new(ColumnId(col_id as u64), col_name);
                let mut stmt = conn.prepare(
                    "SELECT frame, exposure, drawing_id FROM cells WHERE column_id = ?1",
                )?;
                let cells: Vec<(i64, String, Option<i64>)> = stmt
                    .query_map((col_id,), |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
                    .collect::<std::result::Result<_, _>>()?;
                for (frame, kind, drawing_id) in cells {
                    let exposure = match kind.as_str() {
                        "drawing" => Exposure::Drawing(DrawingId(drawing_id.ok_or_else(
                            || EngineError::Corrupt("drawing cell without drawing_id".into()),
                        )? as u64)),
                        "empty" => Exposure::Empty,
                        other => {
                            return Err(EngineError::Corrupt(format!(
                                "unknown exposure kind '{other}'"
                            )));
                        }
                    };
                    column.set_key(frame as u32, exposure);
                }
                cut.xsheet.columns.push(column);
            }

            let mut stmt = conn.prepare("SELECT id, kind FROM nodes WHERE cut_id = ?1")?;
            let nodes: Vec<(i64, String)> = stmt
                .query_map((cut_id,), |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<std::result::Result<_, _>>()?;
            for (node_id, kind_json) in nodes {
                let kind: NodeKind = serde_json::from_str(&kind_json)?;
                cut.graph.add_node(Node::new(NodeId(node_id as u64), kind));
            }

            let mut stmt = conn.prepare(
                "SELECT to_node, to_pin, from_node, from_pin FROM links WHERE cut_id = ?1",
            )?;
            let links: Vec<(i64, i64, i64, i64)> = stmt
                .query_map((cut_id,), |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
                })?
                .collect::<std::result::Result<_, _>>()?;
            for (to, to_pin, from, from_pin) in links {
                let to_id = NodeId(to as u64);
                let node = cut.graph.node_mut(to_id)?;
                let pin = to_pin as usize;
                if pin >= node.inputs.len() {
                    return Err(EngineError::Corrupt(format!(
                        "link pin {pin} out of range on node {to_id}"
                    )));
                }
                node.inputs[pin] = Some((NodeId(from as u64), from_pin as u16));
            }

            cut.graph.output = output_node.map(|n| NodeId(n as u64));
            scene.cuts.push(cut);
        }
        project.scenes.push(scene);
    }

    Ok(project)
}
