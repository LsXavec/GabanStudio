//! XDTS interop: export shape pinned against the OpenToonz format
//! (signature line, key names, cell-value encoding) and parse/round-trip.

use anim_core::Engine;
use anim_core::command::{Command, CutRef};
use anim_core::xdts;
use anim_core::xsheet::Exposure;

fn rig() -> (Engine, CutRef) {
    let mut engine = Engine::new("xdts test");
    let scene = engine.add_scene("SC02");
    let cut = engine.add_cut(scene, "CUT13", 48).unwrap();
    let at = CutRef { scene, cut };
    let col_a = engine.add_column(at, "A").unwrap();
    let col_b = engine.add_column(at, "B").unwrap();
    let d1 = engine.alloc_drawing_id();
    let d2 = engine.alloc_drawing_id();
    engine
        .apply(
            "rig",
            vec![
                Command::AddDrawing {
                    at,
                    id: d1,
                    index: 0,
                    name: "genga 1".into(),
                    strokes: vec![],
                    layers: vec![],
                },
                Command::AddDrawing {
                    at,
                    id: d2,
                    index: 1,
                    name: "genga 2".into(),
                    strokes: vec![],
                    layers: vec![],
                },
                Command::SetCell { at, column: col_a, frame: 0, key: Some(Exposure::Drawing(d1)) },
                Command::SetCell { at, column: col_a, frame: 6, key: Some(Exposure::Drawing(d2)) },
                Command::SetCell { at, column: col_a, frame: 12, key: Some(Exposure::Empty) },
                Command::SetCell { at, column: col_b, frame: 3, key: Some(Exposure::Drawing(d2)) },
            ],
        )
        .unwrap();
    (engine, at)
}

fn cut_of(engine: &Engine, at: CutRef) -> &anim_core::model::Cut {
    engine
        .project
        .scenes
        .iter()
        .find(|s| s.id == at.scene)
        .unwrap()
        .cuts
        .iter()
        .find(|c| c.id == at.cut)
        .unwrap()
}

#[test]
fn export_matches_the_opentoonz_shape() {
    let (engine, at) = rig();
    let text = xdts::export(cut_of(&engine, at), "SC02");

    let (first, json_text) = text.split_once('\n').unwrap();
    assert_eq!(first, "exchangeDigitalTimeSheet Save Data");
    let doc: serde_json::Value = serde_json::from_str(json_text).unwrap();

    assert_eq!(doc["version"], 5);
    assert_eq!(doc["header"]["scene"], "2");
    assert_eq!(doc["header"]["cut"], "13");

    let table = &doc["timeTables"][0];
    assert_eq!(table["name"], "CUT13");
    assert_eq!(table["duration"], 48);
    assert_eq!(table["timeTableHeaders"][0]["fieldId"], 0);
    assert_eq!(table["timeTableHeaders"][0]["names"][0], "A");
    assert_eq!(table["timeTableHeaders"][0]["names"][1], "B");

    let field = &table["fields"][0];
    assert_eq!(field["fieldId"], 0);
    let track_a = &field["tracks"][0];
    assert_eq!(track_a["trackNo"], 0);
    // Keys: d1 at 0 (library slot 1), d2 at 6 (slot 2), Empty at 12.
    assert_eq!(track_a["frames"][0]["frame"], 0);
    assert_eq!(track_a["frames"][0]["data"][0]["values"][0], "1");
    assert_eq!(track_a["frames"][1]["frame"], 6);
    assert_eq!(track_a["frames"][1]["data"][0]["values"][0], "2");
    assert_eq!(track_a["frames"][2]["frame"], 12);
    assert_eq!(track_a["frames"][2]["data"][0]["values"][0], "SYMBOL_NULL_CELL");
    let track_b = &field["tracks"][1];
    assert_eq!(track_b["frames"][0]["frame"], 3);
    assert_eq!(track_b["frames"][0]["data"][0]["values"][0], "2");
}

#[test]
fn export_parse_round_trip_preserves_timing() {
    let (engine, at) = rig();
    let text = xdts::export(cut_of(&engine, at), "SC02");
    let sheet = xdts::parse(&text).unwrap();

    assert_eq!(sheet.name, "CUT13");
    assert_eq!(sheet.duration, 48);
    assert_eq!(sheet.columns.len(), 2);
    assert_eq!(sheet.columns[0].name, "A");
    assert_eq!(
        sheet.columns[0].keys,
        vec![
            (0, Some("1".into())),
            (6, Some("2".into())),
            (12, None),
        ]
    );
    assert_eq!(sheet.columns[1].name, "B");
    assert_eq!(sheet.columns[1].keys, vec![(3, Some("2".into()))]);
}

#[test]
fn parse_tolerates_missing_signature_and_skips_annotation_fields() {
    // A minimal foreign file: no signature line, a DIALOG field (id 3)
    // before the CELL field, tick symbols inside a track.
    let text = r#"{
      "header": {"cut": "1", "scene": "1"},
      "timeTables": [{
        "name": "cut 1", "duration": 24,
        "fields": [
          {"fieldId": 3, "tracks": [{"trackNo": 0, "frames": [
            {"frame": 0, "data": [{"id": 0, "values": ["A-san"]}]}]}]},
          {"fieldId": 0, "tracks": [{"trackNo": 0, "frames": [
            {"frame": 0, "data": [{"id": 0, "values": ["5"]}]},
            {"frame": 2, "data": [{"id": 0, "values": ["SYMBOL_TICK_1"]}]},
            {"frame": 4, "data": [{"id": 0, "values": ["6a"]}]}]}]}
        ],
        "timeTableHeaders": [{"fieldId": 0, "names": ["CELL A"]}]
      }],
      "version": 5
    }"#;
    let sheet = xdts::parse(text).unwrap();
    assert_eq!(sheet.duration, 24);
    assert_eq!(sheet.columns.len(), 1);
    assert_eq!(sheet.columns[0].name, "CELL A");
    // Tick symbol dropped; lettered cell kept as-is.
    assert_eq!(
        sheet.columns[0].keys,
        vec![(0, Some("5".into())), (4, Some("6a".into()))]
    );
}

#[test]
fn parse_rejects_garbage() {
    assert!(xdts::parse("not json at all").is_err());
    assert!(xdts::parse("{}").is_err());
    assert!(
        xdts::parse(r#"{"timeTables":[{"name":"x","duration":0,"fields":[{"fieldId":0,"tracks":[]}]}]}"#)
            .is_err(),
        "zero duration must be rejected"
    );
}
