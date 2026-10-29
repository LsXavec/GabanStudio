//! GM12 sound tracks: decoding by magic bytes, the mix (offset, gain, mute,
//! solo), resampling, the first track and the `audio_tracks` table, the
//! per-frame envelope, the 3-mouth lip flap, the track commands and the
//! frame ripple, and the mixdown deliveries carry.
//!
//! The fixtures in `tests/fixtures/` were made ONCE, by hand, with the
//! installed ffmpeg 8.1.2 (full build, winget Gyan.FFmpeg) — the tests only
//! read them and never spawn ffmpeg:
//!
//! ```text
//! ffmpeg -hide_banner -loglevel error -y -f lavfi -i "sine=frequency=440:sample_rate=16000:duration=0.5" -ac 1 -c:a pcm_s16le -bitexact tone.wav
//! ffmpeg -hide_banner -loglevel error -y -i tone.wav -c:a flac -bitexact tone.flac
//! ffmpeg -hide_banner -loglevel error -y -i tone.wav -c:a libmp3lame -b:a 32k tone.mp3
//! ffmpeg -hide_banner -loglevel error -y -i tone.wav -c:a libvorbis -q:a 2 tone.ogg
//! ```
//!
//! (16 kHz keeps the 0.5 s, 16-bit mono WAV under the 20 KB fixture cap.)

use anim_core::Engine;
use anim_core::audio::{self, AudioTrack, Format, Mouth, TrackKind, TrackProps};
use anim_core::command::{Command, CutRef};
use anim_core::ids::DrawingId;
use anim_core::model::AudioClip;
use anim_core::xsheet::Exposure;

fn fixture(name: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// A 16-bit PCM WAV of `frames` sample frames, `f(i, channel)` each.
fn wav(rate: u32, channels: u16, frames: u32, f: impl Fn(u32, u16) -> f32) -> Vec<u8> {
    let spec = hound::WavSpec {
        channels,
        sample_rate: rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut out = std::io::Cursor::new(Vec::new());
    {
        let mut w = hound::WavWriter::new(&mut out, spec).unwrap();
        for i in 0..frames {
            for c in 0..channels {
                w.write_sample((f(i, c) * 32768.0).round().clamp(-32768.0, 32767.0) as i16)
                    .unwrap();
            }
        }
        w.finalize().unwrap();
    }
    out.into_inner()
}

fn clip(name: &str, bytes: Vec<u8>) -> AudioClip {
    AudioClip::from_bytes(name, bytes).unwrap()
}

fn engine_with_cut(frames: u32) -> (Engine, CutRef) {
    let mut engine = Engine::new("sound test");
    let scene = engine.add_scene("SC01");
    let cut = engine.add_cut(scene, "CUT01", frames).unwrap();
    (engine, CutRef { scene, cut })
}

fn track(name: &str, kind: TrackKind, clip: AudioClip) -> AudioTrack {
    AudioTrack {
        props: TrackProps::new(name, kind),
        clip,
    }
}

/// Zero crossings per second / 2 — the tone's frequency.
fn zcr_hz(samples: &[f32], rate: u32) -> f32 {
    let crossings = samples
        .windows(2)
        .filter(|w| (w[0] >= 0.0) != (w[1] >= 0.0))
        .count();
    crossings as f32 / 2.0 / (samples.len() as f32 / rate as f32)
}

fn close(a: &[f32], b: &[f32], tol: f32) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x - y).abs() <= tol)
}

#[test]
fn a_wav_made_with_hound_decodes_to_its_samples() {
    let f = |i: u32, c: u16| ((i as f32 * 0.37).sin() * 0.6) * if c == 0 { 1.0 } else { -0.5 };
    let bytes = wav(22_050, 2, 1000, f);
    let (rate, channels, samples) = audio::decode(&bytes, "wav").unwrap();
    assert_eq!((rate, channels, samples.len()), (22_050, 2, 2000));
    let want: Vec<f32> = (0..1000u32)
        .flat_map(|i| [f(i, 0), f(i, 1)])
        .collect();
    assert!(close(&samples, &want, 1e-4), "decoded samples drift past 1e-4");
}

#[test]
fn the_flac_fixture_decodes_to_the_wav_samples_losslessly() {
    let (wr, wc, w) = audio::decode(&fixture("tone.wav"), "wav").unwrap();
    let (fr, fc, f) = audio::decode(&fixture("tone.flac"), "flac").unwrap();
    assert_eq!((wr, wc), (16_000, 1));
    assert_eq!((fr, fc), (wr, wc));
    assert_eq!(w.len(), 8000, "0.5 s at 16 kHz");
    assert!(close(&f, &w, 1e-4), "FLAC is lossless");
    // The tone itself: 440 Hz.
    assert!((zcr_hz(&w, wr) - 440.0).abs() < 440.0 * 0.02);
}

#[test]
fn the_mp3_and_ogg_fixtures_last_half_a_second_at_440_hz() {
    for name in ["tone.mp3", "tone.ogg"] {
        let (rate, channels, s) = audio::decode(&fixture(name), "").unwrap();
        assert_eq!(channels, 1, "{name}");
        let secs = s.len() as f32 / rate as f32;
        assert!((secs - 0.5).abs() <= 0.030, "{name} lasts {secs} s");
        let hz = zcr_hz(&s, rate);
        assert!((hz - 440.0).abs() <= 440.0 * 0.02, "{name} reads {hz} Hz");
    }
}

#[test]
fn the_router_goes_by_magic_bytes_not_by_extension() {
    let pairs = [
        ("tone.wav", Format::Wav),
        ("tone.flac", Format::Flac),
        ("tone.mp3", Format::Mp3),
        ("tone.ogg", Format::Ogg),
    ];
    for (name, format) in pairs {
        let bytes = fixture(name);
        assert_eq!(audio::sniff(&bytes), Some(format), "{name}");
        // A lying extension changes nothing: every file decodes as itself.
        for lie in ["wav", "mp3", "ogg", "flac", "txt"] {
            let (rate, _, s) = audio::decode(&bytes, lie)
                .unwrap_or_else(|e| panic!("{name} named .{lie}: {e}"));
            assert_eq!(rate, 16_000, "{name} named .{lie}");
            assert!(s.len() > 7000, "{name} named .{lie}");
        }
        // And a clip named for another format loads all the same.
        let c = AudioClip::from_bytes("renamed.wav", bytes).unwrap();
        assert_eq!(c.sample_rate, 16_000, "{name}");
    }
    assert_eq!(audio::sniff(b"not audio at all"), None);
    assert!(audio::decode(b"not audio at all", "mp3").is_err());
    assert!(AudioClip::from_bytes("x.mp3", b"garbage".to_vec()).is_err());
}

#[test]
fn a_mix_of_two_tracks_is_the_pinned_sum_of_offsets_and_gains() {
    // 48 kHz at 24 fps: 2000 samples a frame. Values exact in 16-bit.
    let (mut engine, at) = engine_with_cut(3);
    let a = clip("a.wav", wav(48_000, 1, 2000, |_, _| 0.125));
    let b = clip("b.wav", wav(48_000, 2, 2000, |_, c| if c == 0 { 0.25 } else { -0.25 }));
    engine
        .apply("dialogue", vec![Command::SetCutAudio { at, audio: Some(a) }])
        .unwrap();
    let mut se = track("se", TrackKind::Effects, b);
    se.props.offset = 1;
    se.props.gain_db = -6.020_6; // × 0.5
    engine
        .apply("se", vec![Command::AddTrack { at, index: 1, track: se }])
        .unwrap();
    let cut = engine.project.cut(at.scene, at.cut).unwrap();
    let m = audio::mix(cut, 24, 3, 48_000);
    assert_eq!(m.len(), 3 * 2000 * 2);
    let frame = |f: usize| &m[f * 4000..(f + 1) * 4000];
    // Frame 1: the dialogue alone; 2: dialogue gone, SE at half; 3: silence.
    assert!(frame(0).chunks(2).all(|p| (p[0] - 0.125).abs() < 1e-4 && (p[1] - 0.125).abs() < 1e-4));
    assert!(frame(1).chunks(2).all(|p| (p[0] - 0.125).abs() < 1e-4 && (p[1] + 0.125).abs() < 1e-4));
    assert!(frame(2).iter().all(|s| s.abs() < 1e-6));
    // A negative offset starts the clip before the cut.
    let mut moved = cut.clone();
    moved.audio_props.offset = -1;
    let m = audio::mix(&moved, 24, 3, 48_000);
    assert!(m[..4000].iter().all(|s| s.abs() < 1e-6), "the dialogue's frame fell before frame 1");
}

#[test]
fn mute_and_solo_follow_the_rules() {
    let (mut engine, at) = engine_with_cut(1);
    let a = clip("a.wav", wav(48_000, 1, 2000, |_, _| 0.125));
    let b = clip("b.wav", wav(48_000, 1, 2000, |_, _| 0.25));
    engine
        .apply("a", vec![Command::SetCutAudio { at, audio: Some(a) }])
        .unwrap();
    engine
        .apply("b", vec![Command::AddTrack { at, index: 1, track: track("bgm", TrackKind::Music, b) }])
        .unwrap();
    let level = |engine: &Engine| {
        let cut = engine.project.cut(at.scene, at.cut).unwrap();
        audio::mix(cut, 24, 1, 48_000)[100]
    };
    let set = |engine: &mut Engine, index: usize, f: &dyn Fn(&mut TrackProps)| {
        let cut = engine.project.cut(at.scene, at.cut).unwrap();
        let before = audio::track_props(cut, index).unwrap().clone();
        let mut after = before.clone();
        f(&mut after);
        engine
            .apply("props", vec![Command::SetTrackProps { at, index, before, after }])
            .unwrap();
    };
    assert!((level(&engine) - 0.375).abs() < 1e-4, "both sound");
    set(&mut engine, 0, &|p| p.mute = true);
    assert!((level(&engine) - 0.25).abs() < 1e-4, "a muted track is silent");
    set(&mut engine, 0, &|p| p.mute = false);
    set(&mut engine, 1, &|p| p.solo = true);
    assert!((level(&engine) - 0.25).abs() < 1e-4, "a solo silences the rest");
    set(&mut engine, 0, &|p| p.solo = true);
    assert!((level(&engine) - 0.375).abs() < 1e-4, "two solos both sound");
    set(&mut engine, 1, &|p| p.mute = true);
    assert!((level(&engine) - 0.125).abs() < 1e-4, "mute wins over solo");
    let cut = engine.project.cut(at.scene, at.cut).unwrap();
    assert!(audio::has_sound(cut));
    set(&mut engine, 0, &|p| p.mute = true);
    let cut = engine.project.cut(at.scene, at.cut).unwrap();
    assert!(!audio::has_sound(cut));
    assert!(anim_core::deliver::mixdown(cut, 24, 0, 0).is_none());
}

#[test]
fn resampling_keeps_the_duration_within_one_sample() {
    let mono: Vec<f32> = (0..44_100).map(|i| (i as f32 * 0.01).sin()).collect();
    let up = audio::resample_linear(&mono, 1, 44_100, 48_000);
    assert!((up.len() as i64 - 48_000).abs() <= 1, "{}", up.len());
    let stereo: Vec<f32> = (0..2 * 22_050).map(|i| i as f32 / 44_100.0).collect();
    let up = audio::resample_linear(&stereo, 2, 44_100, 48_000);
    assert_eq!(up.len() % 2, 0);
    assert!((up.len() as i64 / 2 - 24_000).abs() <= 1, "{}", up.len() / 2);
    let down = audio::resample_linear(&mono, 1, 44_100, 22_050);
    assert!((down.len() as i64 - 22_050).abs() <= 1);
    // A clip at 44.1 kHz in a 48 kHz mix lands at its full length.
    let (mut engine, at) = engine_with_cut(24);
    let c = clip("c.wav", wav(44_100, 1, 44_100 / 2, |_, _| 0.25));
    engine
        .apply("c", vec![Command::SetCutAudio { at, audio: Some(c) }])
        .unwrap();
    let m = audio::mix(engine.project.cut(at.scene, at.cut).unwrap(), 24, 24, 48_000);
    let sounding = m.chunks(2).filter(|p| p[0].abs() > 0.1).count() as i64;
    assert!((sounding - 24_000).abs() <= 1, "{sounding}");
}

#[test]
fn a_v9_file_with_cut_audio_loads_as_the_first_track() {
    let (mut engine, at) = engine_with_cut(24);
    let c = clip("line.wav", wav(48_000, 1, 4800, |i, _| (i as f32 * 0.1).sin() * 0.3));
    engine
        .apply("audio", vec![Command::SetCutAudio { at, audio: Some(c.clone()) }])
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("v9.animproj");
    engine.save(&path).unwrap();
    {
        // Forge the v9 shape: the version back to 9 and no v11 side tables.
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(
            "UPDATE meta SET value = '9' WHERE key = 'schema_version';
             DROP TABLE cut_meta; DROP TABLE column_defs; DROP TABLE cut_memos;
             DROP TABLE drawing_defs; DROP TABLE audio_tracks;",
        )
        .unwrap();
    }
    let loaded = Engine::load(&path).unwrap();
    let cut = loaded.project.cut(at.scene, at.cut).unwrap();
    assert_eq!(cut.audio.as_ref(), Some(&c));
    assert_eq!(cut.audio_props, TrackProps::default());
    assert!(cut.tracks.is_empty());
    let list = audio::tracks(cut);
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].0, 0);
    assert_eq!(list[0].1.kind, TrackKind::Dialogue);
    assert_eq!(list[0].1.offset, 0);
}

#[test]
fn extra_tracks_round_trip_through_audio_tracks() {
    let (mut engine, at) = engine_with_cut(24);
    let dialogue = clip("line.wav", wav(48_000, 1, 2400, |_, _| 0.1));
    let se = clip("door.flac", fixture("tone.flac"));
    let bgm = clip("theme.ogg", fixture("tone.ogg"));
    engine
        .apply("dialogue", vec![Command::SetCutAudio { at, audio: Some(dialogue) }])
        .unwrap();
    let mut t1 = track("door", TrackKind::Effects, se);
    t1.props.offset = 5;
    t1.props.gain_db = -3.5;
    let mut t2 = track("theme", TrackKind::Music, bgm);
    t2.props.offset = -2;
    t2.props.mute = true;
    t2.props.solo = true;
    engine
        .apply(
            "tracks",
            vec![
                Command::AddTrack { at, index: 1, track: t1 },
                Command::AddTrack { at, index: 2, track: t2 },
            ],
        )
        .unwrap();
    let cut = engine.project.cut(at.scene, at.cut).unwrap();
    let mut first = cut.audio_props.clone();
    first.gain_db = 2.25;
    first.name = "serifu A".into();
    engine
        .apply(
            "first",
            vec![Command::SetTrackProps { at, index: 0, before: cut.audio_props.clone(), after: first }],
        )
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tracks.animproj");
    engine.save(&path).unwrap();
    let loaded = Engine::load(&path).unwrap();
    assert_eq!(loaded.project, engine.project);
    let cut = loaded.project.cut(at.scene, at.cut).unwrap();
    assert_eq!(cut.tracks.len(), 2);
    assert_eq!(cut.tracks[0].clip.name, "door.flac");
    assert_eq!(*cut.tracks[1].clip.bytes, fixture("tone.ogg"), "bytes kept verbatim");
    assert_eq!(cut.audio_props.name, "serifu A");
    // The settings JSON tolerates a later build's kind and missing keys.
    let (p, file) = TrackProps::from_json(r#"{"name":"x","kind":"foley","file":"f.wav"}"#).unwrap();
    assert_eq!((p.kind, p.offset, p.gain_db, file.as_str()), (TrackKind::Other, 0, 0.0, "f.wav"));
    assert!(TrackProps::from_json("[1, 2]").is_none());
}

#[test]
fn the_envelope_of_a_sine_is_pinned_per_frame() {
    // 480 Hz at 48 kHz: 100 samples a period, 20 whole periods a frame
    // at 24 fps — RMS = 0.5 / √2 on every frame the clip covers.
    let c = clip("sine.wav", wav(48_000, 1, 48_000, |i, _| {
        (i as f32 * std::f32::consts::TAU / 100.0).sin() * 0.5
    }));
    let rms = 0.5 / 2f32.sqrt();
    let env = audio::envelope(&c, 0, 24, 30);
    assert_eq!(env.len(), 30);
    for (f, v) in env.iter().enumerate() {
        let want = if f < 24 { rms } else { 0.0 };
        assert!((v - want).abs() < 1e-3, "frame {f}: {v} (want {want})");
    }
    // An offset of 2 frames delays it; half a frame of sound reads half
    // the power.
    let env = audio::envelope(&c, 2, 24, 30);
    assert!(env[0] == 0.0 && env[1] == 0.0);
    assert!((env[2] - rms).abs() < 1e-3 && (env[25] - rms).abs() < 1e-3);
    assert_eq!(env[26], 0.0);
    let half = clip("half.wav", wav(48_000, 1, 1000, |i, _| {
        (i as f32 * std::f32::consts::TAU / 100.0).sin() * 0.5
    }));
    let env = audio::envelope(&half, 0, 24, 1);
    assert!((env[0] - rms / 2f32.sqrt()).abs() < 1e-3, "{}", env[0]);
    let n = audio::normalize(&[0.0, 0.5, 0.25]);
    assert_eq!(n, vec![0.0, 1.0, 0.5]);
}

#[test]
fn lip_flap_gives_the_pinned_mouths_with_hysteresis_and_the_minimum_hold() {
    use Mouth::*;
    let th = (0.2, 0.6);
    // Silence, loud, mid.
    let env = [0.0, 0.0, 0.0, 0.9, 0.9, 0.9, 0.3, 0.3, 0.3];
    assert_eq!(
        audio::lip_flap(&env, th, 2),
        vec![Closed, Closed, Closed, Open, Open, Open, Half, Half, Half]
    );
    // A one-frame burst still holds its mouth two frames.
    assert_eq!(
        audio::lip_flap(&[0.0, 0.9, 0.0, 0.0, 0.0], th, 2),
        vec![Closed, Open, Open, Closed, Closed]
    );
    // Hysteresis: hovering just under the open threshold stays open
    // until it falls below 0.8 of it (0.48).
    assert_eq!(
        audio::lip_flap(&[0.7, 0.55, 0.5, 0.45, 0.45], th, 2),
        vec![Open, Open, Open, Half, Half]
    );
    // Rising through half to open, then all the way shut.
    assert_eq!(
        audio::lip_flap(&[0.3, 0.3, 0.8, 0.8, 0.05, 0.05], th, 2),
        vec![Half, Half, Open, Open, Closed, Closed]
    );
    // Every run is at least the hold long, bar a short last run and a
    // leading closed run (it continues the closed mouth before the range).
    let noisy: Vec<f32> = (0..60).map(|i| ((i * 37 % 11) as f32) / 10.0).collect();
    let mouths = audio::lip_flap(&noisy, th, 3);
    assert_eq!(mouths[0], Closed);
    let mut runs = vec![1u32];
    for w in mouths.windows(2) {
        if w[0] == w[1] {
            *runs.last_mut().unwrap() += 1;
        } else {
            runs.push(1);
        }
    }
    assert!(runs.len() > 5, "{runs:?}");
    assert!(runs[1..runs.len() - 1].iter().all(|r| *r >= 3), "{runs:?}");
    let opening = audio::lip_flap(&[0.9, 0.0, 0.0, 0.0], th, 3);
    assert_eq!(opening, vec![Open, Open, Open, Closed], "a first open run holds too");
}

#[test]
fn generate_writes_the_mouths_as_one_undo_step() {
    let (mut engine, at) = engine_with_cut(12);
    let col = engine.add_column(at, "kuchi").unwrap();
    let ids: Vec<DrawingId> = (0..4).map(|_| engine.alloc_drawing_id()).collect();
    let mut cmds = Vec::new();
    for (i, id) in ids.iter().enumerate() {
        cmds.push(Command::AddDrawing {
            at,
            id: *id,
            index: i,
            name: ["closed", "half", "open", "body"][i].into(),
            strokes: vec![],
            layers: vec![],
        });
    }
    engine.apply("drawings", cmds).unwrap();
    // The column shows "body" from frame 1; the lip flap covers 3..8.
    engine
        .apply(
            "key",
            vec![Command::SetCell { at, column: col, frame: 0, key: Some(Exposure::Drawing(ids[3])) }],
        )
        .unwrap();
    let before = engine.project.clone();
    let steps = engine.history().len();
    use Mouth::*;
    let mouths = [Closed, Open, Open, Half, Half, Closed];
    let cmds = audio::lip_flap_commands(&engine.project, at, col, [ids[0], ids[1], ids[2]], 3, &mouths)
        .unwrap();
    assert_eq!(cmds.len(), 1, "one SetCells");
    engine.apply("lip flap", cmds).unwrap();
    assert_eq!(engine.history().len(), steps + 1, "one undo step");
    let cut = engine.project.cut(at.scene, at.cut).unwrap();
    let column = cut.xsheet.column(col).unwrap();
    let shown: Vec<Option<DrawingId>> = (0..12).map(|f| column.resolve(f)).collect();
    let (c, h, o, b) = (Some(ids[0]), Some(ids[1]), Some(ids[2]), Some(ids[3]));
    assert_eq!(shown, vec![b, b, b, c, o, o, h, h, c, b, b, b], "the frames after keep the body");
    engine.undo().unwrap();
    assert_eq!(engine.project, before);
}

#[test]
fn track_commands_have_exact_inverses() {
    let (mut engine, at) = engine_with_cut(24);
    let before = engine.project.clone();
    let a = clip("a.mp3", fixture("tone.mp3"));
    let b = clip("b.wav", wav(48_000, 2, 480, |_, _| 0.2));
    engine
        .apply(
            "add",
            vec![
                Command::AddTrack { at, index: 1, track: track("se", TrackKind::Effects, a) },
                Command::AddTrack { at, index: 1, track: track("bgm", TrackKind::Music, b) },
            ],
        )
        .unwrap();
    let cut = engine.project.cut(at.scene, at.cut).unwrap();
    assert_eq!(cut.tracks[0].props.name, "bgm", "inserted ahead of se");
    let p = cut.tracks[1].props.clone();
    let q = TrackProps {
        offset: 7,
        gain_db: -12.0,
        mute: true,
        kind: TrackKind::Other,
        ..p.clone()
    };
    engine
        .apply("props", vec![Command::SetTrackProps { at, index: 2, before: p.clone(), after: q }])
        .unwrap();
    engine
        .apply("remove", vec![Command::RemoveTrack { at, index: 1 }])
        .unwrap();
    let after = engine.project.clone();
    for _ in 0..3 {
        engine.undo().unwrap();
    }
    assert_eq!(engine.project, before);
    for _ in 0..3 {
        engine.redo().unwrap();
    }
    assert_eq!(engine.project, after);
    // Refusals: a stale before, a gain out of range, the first track's slot.
    let cut = engine.project.cut(at.scene, at.cut).unwrap();
    let now = cut.tracks[0].props.clone();
    let stale = Command::SetTrackProps { at, index: 1, before: p.clone(), after: p.clone() };
    assert!(engine.apply("stale", vec![stale]).is_err());
    let loud = TrackProps { gain_db: 99.0, ..now.clone() };
    assert!(engine.apply("loud", vec![Command::SetTrackProps { at, index: 1, before: now, after: loud }]).is_err());
    assert!(engine.apply("zero", vec![Command::RemoveTrack { at, index: 0 }]).is_err());
    assert_eq!(engine.project, after);
}

#[test]
fn insert_and_delete_frames_ripple_track_offsets() {
    let (mut engine, at) = engine_with_cut(24);
    let c = |n| clip("c.wav", wav(48_000, 1, 480, move |_, _| n));
    engine
        .apply("first", vec![Command::SetCutAudio { at, audio: Some(c(0.1)) }])
        .unwrap();
    let mut early = track("early", TrackKind::Effects, c(0.2));
    early.props.offset = 2;
    let mut late = track("late", TrackKind::Music, c(0.3));
    late.props.offset = 10;
    let mut before_cut = track("pre", TrackKind::Other, c(0.4));
    before_cut.props.offset = -3;
    engine
        .apply(
            "tracks",
            vec![
                Command::AddTrack { at, index: 1, track: early },
                Command::AddTrack { at, index: 2, track: late },
                Command::AddTrack { at, index: 3, track: before_cut },
            ],
        )
        .unwrap();
    let offsets = |engine: &Engine| {
        let cut = engine.project.cut(at.scene, at.cut).unwrap();
        audio::tracks(cut).iter().map(|(_, p, _)| p.offset).collect::<Vec<_>>()
    };
    assert_eq!(offsets(&engine), vec![0, 2, 10, -3]);
    engine
        .apply("insert", vec![Command::InsertFrames { at, frame: 2, count: 3 }])
        .unwrap();
    assert_eq!(offsets(&engine), vec![0, 5, 13, -3], "at or after frame 3 moves 3 later");
    let opened = engine.project.clone();
    // Delete frames 4..=11 (0-based 3..11): "early" (at 5) starts inside
    // and lands on the range start; "late" (13) moves 8 earlier.
    engine
        .apply("delete", vec![Command::DeleteFrames { at, frame: 3, count: 8 }])
        .unwrap();
    assert_eq!(offsets(&engine), vec![0, 3, 5, -3]);
    engine.undo().unwrap();
    assert_eq!(engine.project, opened, "the inverse restores every offset");
    engine.undo().unwrap();
    assert_eq!(offsets(&engine), vec![0, 2, 10, -3]);
    // A first track with no clip has nothing to keep in sync.
    engine
        .apply("no first", vec![Command::SetCutAudio { at, audio: None }])
        .unwrap();
    engine
        .apply("insert", vec![Command::InsertFrames { at, frame: 0, count: 2 }])
        .unwrap();
    let cut = engine.project.cut(at.scene, at.cut).unwrap();
    assert_eq!(cut.audio_props.offset, 0);
    assert_eq!(cut.tracks[0].props.offset, 4);
}

#[test]
fn the_mixdown_wav_carries_the_mix_of_the_rendered_range() {
    let (mut engine, at) = engine_with_cut(4);
    let c = clip("a.wav", wav(44_100, 1, 44_100 / 6, |i, _| if i < 44_100 / 24 { 0.25 } else { -0.125 }));
    engine
        .apply("a", vec![Command::SetCutAudio { at, audio: Some(c) }])
        .unwrap();
    let cut = engine.project.cut(at.scene, at.cut).unwrap();
    let whole = audio::mix(cut, 24, 4, audio::MIX_RATE);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("mix.wav");
    assert!(anim_core::deliver::write_mixdown(&path, cut, 24, 0, 3).unwrap());
    let (rate, channels, s) = audio::decode(&std::fs::read(&path).unwrap(), "").unwrap();
    assert_eq!((rate, channels), (48_000, 2));
    assert!(close(&s, &whole, 1e-4), "24-bit mixdown equals the mix");
    // A range starts its WAV at its first frame.
    let range = anim_core::deliver::mixdown(cut, 24, 1, 2).unwrap();
    assert_eq!(range.len(), 2 * 2000 * 2);
    assert!(close(&range, &whole[4000..12_000], 1e-6));
    // Nothing sounding: no file.
    let quiet = anim_core::model::Cut::new(cut.id, "quiet", 4);
    let none = dir.path().join("none.wav");
    assert!(!anim_core::deliver::write_mixdown(&none, &quiet, 24, 0, 3).unwrap());
    assert!(!none.exists());
}

#[test]
fn the_mix_resamples_only_its_span_and_matches_the_whole_clip_resampled() {
    // A 2-second 44.1 kHz stereo bed starting 30 frames before a 12-frame
    // cut, under a 22.05 kHz mono line 5 frames in: the mix equals the
    // whole clips resampled, sliced and summed, bit for bit.
    let bed = wav(44_100, 2, 88_200, |i, c| ((i as f32 * 0.013).sin() * 0.4) * if c == 0 { 1.0 } else { 0.5 });
    let line = wav(22_050, 1, 11_025, |i, _| (i as f32 * 0.07).cos() * 0.3);
    let (mut engine, at) = engine_with_cut(12);
    let (b, l) = (clip("bed.wav", bed), clip("line.wav", line));
    let mut props = TrackProps::new("bed", TrackKind::Music);
    props.offset = -30;
    props.gain_db = -6.0;
    engine
        .apply(
            "tracks",
            vec![
                Command::SetCutAudio { at, audio: Some(l.clone()) },
                Command::AddTrack { at, index: 1, track: AudioTrack { props: props.clone(), clip: b.clone() } },
                Command::SetTrackProps {
                    at,
                    index: 0,
                    before: TrackProps::default(),
                    after: TrackProps { offset: 5, ..TrackProps::default() },
                },
            ],
        )
        .unwrap();
    let cut = engine.project.cut(at.scene, at.cut).unwrap();
    let got = audio::mix(cut, 24, 12, 48_000);
    let len = 12 * 48_000 / 24;
    let mut want = vec![0.0f32; len * 2];
    let place = |want: &mut Vec<f32>, src: Vec<f32>, ch: usize, offset: i64, g: f32| {
        let start = (offset * 48_000).div_euclid(24);
        for i in 0..src.len() / ch {
            let o = start + i as i64;
            if o < 0 || o >= len as i64 {
                continue;
            }
            let (l, r) = if ch == 1 { (src[i], src[i]) } else { (src[i * ch], src[i * ch + 1]) };
            want[2 * o as usize] += l * g;
            want[2 * o as usize + 1] += r * g;
        }
    };
    place(&mut want, audio::resample_linear(&l.samples, 1, 22_050, 48_000), 1, 5, 1.0);
    place(&mut want, audio::resample_linear(&b.samples, 2, 44_100, 48_000), 2, -30, props.gain());
    for s in &mut want {
        *s = s.clamp(-1.0, 1.0);
    }
    assert_eq!(got.len(), want.len());
    assert!(got == want, "the span mix drifts from the whole-clip resampling");
}

#[test]
fn an_rf64_and_a_wav_hound_refuses_still_decode() {
    // RF64 (EBU 3306): the RIFF's own bytes behind an RF64 marker, a ds64
    // chunk and 0xFFFFFFFF sizes — decodes to the RIFF's samples.
    let riff = wav(16_000, 1, 800, |i, _| (i as f32 * 0.2).sin() * 0.5);
    let want = AudioClip::from_wav("a.wav", riff.clone()).unwrap();
    let data_len = riff.len() as u64 - 44;
    let mut rf64 = b"RF64".to_vec();
    rf64.extend_from_slice(&u32::MAX.to_le_bytes());
    rf64.extend_from_slice(b"WAVEds64");
    rf64.extend_from_slice(&28u32.to_le_bytes());
    rf64.extend_from_slice(&(riff.len() as u64 + 36 - 8).to_le_bytes());
    rf64.extend_from_slice(&data_len.to_le_bytes());
    rf64.extend_from_slice(&800u64.to_le_bytes());
    rf64.extend_from_slice(&0u32.to_le_bytes());
    rf64.extend_from_slice(&riff[12..40]); // "fmt " and its 16 bytes, "data"
    rf64.extend_from_slice(&u32::MAX.to_le_bytes());
    rf64.extend_from_slice(&riff[44..]);
    assert_eq!(audio::sniff(&rf64), Some(Format::Wav));
    let got = AudioClip::from_bytes("a.wav", rf64.clone()).unwrap();
    assert_eq!((got.sample_rate, got.channels), (16_000, 1));
    assert_eq!(got.samples, want.samples);
    assert_eq!(*got.bytes, rf64, "the clip keeps the file's own bytes");
    // A 64-bit float WAV: hound refuses it, symphonia's reader takes it.
    let mut f64wav = b"RIFF".to_vec();
    let body: Vec<u8> = (0..400).flat_map(|i| ((i as f64 * 0.1).sin() * 0.25).to_le_bytes()).collect();
    f64wav.extend_from_slice(&(36 + body.len() as u32).to_le_bytes());
    f64wav.extend_from_slice(b"WAVEfmt ");
    f64wav.extend_from_slice(&16u32.to_le_bytes());
    f64wav.extend_from_slice(&3u16.to_le_bytes());
    f64wav.extend_from_slice(&1u16.to_le_bytes());
    f64wav.extend_from_slice(&8_000u32.to_le_bytes());
    f64wav.extend_from_slice(&64_000u32.to_le_bytes());
    f64wav.extend_from_slice(&8u16.to_le_bytes());
    f64wav.extend_from_slice(&64u16.to_le_bytes());
    f64wav.extend_from_slice(b"data");
    f64wav.extend_from_slice(&(body.len() as u32).to_le_bytes());
    f64wav.extend_from_slice(&body);
    assert!(AudioClip::from_wav("f.wav", f64wav.clone()).is_err(), "hound reads no 64-bit float");
    let got = AudioClip::from_bytes("f.wav", f64wav).unwrap();
    assert_eq!((got.sample_rate, got.channels, got.samples.len()), (8_000, 1, 400));
    assert!((got.samples[10] - (1.0f64.sin() * 0.25) as f32).abs() < 1e-6);
    // Both refusing: hound's reason stands.
    let mut broken = riff.clone();
    broken[22] = 7; // seven channels
    let err = AudioClip::from_bytes("b.wav", broken).unwrap_err();
    assert!(err.starts_with("wav:"), "{err}");
}

#[test]
fn a_track_kind_is_guessed_from_the_file_stem() {
    for (stem, kind) in [
        ("c012_bgm", TrackKind::Music),
        ("BGM02", TrackKind::Music),
        ("theme music", TrackKind::Music),
        ("m01-a", TrackKind::Music),
        ("se_door", TrackKind::Effects),
        ("SFX-03", TrackKind::Effects),
        ("serifu_take2", TrackKind::Dialogue),
        ("voice", TrackKind::Dialogue),
        ("room tone", TrackKind::Effects),
        ("mom", TrackKind::Effects),
    ] {
        assert_eq!(TrackKind::guess(stem), kind, "{stem}");
    }
}
