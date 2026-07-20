//! The audio scratch track (C4): WAV decode law, SetCutAudio undo
//! exactness, and SQLite round-trip (bytes persisted verbatim).

use anim_core::Engine;
use anim_core::command::{Command, CutRef};
use anim_core::model::AudioClip;

/// Synthesize a small 16-bit PCM WAV in memory: `n` frames of a ramp at
/// `sample_rate`/`channels`.
fn wav_bytes(sample_rate: u32, channels: u16, n: u32) -> Vec<u8> {
    let spec = hound::WavSpec {
        channels,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut out = std::io::Cursor::new(Vec::new());
    {
        let mut w = hound::WavWriter::new(&mut out, spec).unwrap();
        for i in 0..(n * channels as u32) {
            w.write_sample(((i % 1000) as i32 - 500) as i16).unwrap();
        }
        w.finalize().unwrap();
    }
    out.into_inner()
}

fn engine_with_cut() -> (Engine, CutRef) {
    let mut engine = Engine::new("audio test");
    let scene = engine.add_scene("SC01");
    let cut = engine.add_cut(scene, "cut A", 24).unwrap();
    (engine, CutRef { scene, cut })
}

#[test]
fn from_wav_decodes_and_keeps_bytes_verbatim() {
    let bytes = wav_bytes(48_000, 2, 4800); // 0.1s stereo
    let clip = AudioClip::from_wav("scratch.wav", bytes.clone()).unwrap();
    assert_eq!(clip.sample_rate, 48_000);
    assert_eq!(clip.channels, 2);
    assert_eq!(clip.samples.len(), 4800 * 2);
    assert_eq!(*clip.bytes, bytes, "original bytes are the persisted truth");
    assert!((clip.seconds() - 0.1).abs() < 1e-3);
    // 16-bit normalization: full scale is 1/32768.
    assert!(clip.samples.iter().all(|s| s.abs() <= 1.0));
}

#[test]
fn from_wav_rejects_garbage_and_too_many_channels() {
    assert!(AudioClip::from_wav("x", b"not a wav at all".to_vec()).is_err());
    let bytes = wav_bytes(48_000, 3, 100);
    assert!(
        AudioClip::from_wav("x", bytes).is_err(),
        "3-channel WAV must be rejected (mono/stereo only)"
    );
}

/// Float WAVs are a trust boundary: NaN/inf and out-of-range samples must
/// sanitize at decode (NaN would even break Project equality reflexivity,
/// since sample vectors were once compared element-wise).
#[test]
fn from_wav_sanitizes_hostile_float_samples() {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 44_100,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut out = std::io::Cursor::new(Vec::new());
    {
        let mut w = hound::WavWriter::new(&mut out, spec).unwrap();
        for v in [0.5f32, f32::NAN, f32::INFINITY, -9.0, f32::NEG_INFINITY, -0.25] {
            w.write_sample(v).unwrap();
        }
        w.finalize().unwrap();
    }
    let clip = AudioClip::from_wav("hostile.wav", out.into_inner()).unwrap();
    assert_eq!(*clip.samples, vec![0.5, 0.0, 0.0, -1.0, 0.0, -0.25]);
    let project_check = clip.clone();
    assert_eq!(clip, project_check, "equality must be reflexive after decode");
}

#[test]
fn set_cut_audio_undo_redo_is_exact() {
    let (mut engine, at) = engine_with_cut();
    let before = engine.project.clone();
    let clip_a = AudioClip::from_wav("a.wav", wav_bytes(44_100, 1, 441)).unwrap();
    let clip_b = AudioClip::from_wav("b.wav", wav_bytes(48_000, 2, 480)).unwrap();

    engine
        .apply(
            "audio a",
            vec![Command::SetCutAudio { at, audio: Some(clip_a) }],
        )
        .unwrap();
    engine
        .apply(
            "audio b (replace)",
            vec![Command::SetCutAudio { at, audio: Some(clip_b) }],
        )
        .unwrap();
    let after = engine.project.clone();

    engine.undo().unwrap(); // back to clip a
    engine.undo().unwrap(); // back to none
    assert_eq!(engine.project, before);
    engine.redo().unwrap();
    engine.redo().unwrap();
    assert_eq!(engine.project, after);
}

#[test]
fn audio_survives_sqlite_roundtrip() {
    let (mut engine, at) = engine_with_cut();
    let clip = AudioClip::from_wav("scratch.wav", wav_bytes(48_000, 2, 9600)).unwrap();
    engine
        .apply("audio", vec![Command::SetCutAudio { at, audio: Some(clip) }])
        .unwrap();

    let dir = std::env::temp_dir().join("anim_core_tests");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("audio_roundtrip.animproj");
    engine.save(&path).unwrap();
    let loaded = Engine::load(&path).unwrap();
    std::fs::remove_file(&path).ok();
    assert_eq!(loaded.project, engine.project);

    // And removal round-trips too (no cut_audio row).
    engine
        .apply("remove", vec![Command::SetCutAudio { at, audio: None }])
        .unwrap();
    let path = dir.join("audio_roundtrip2.animproj");
    engine.save(&path).unwrap();
    let loaded = Engine::load(&path).unwrap();
    std::fs::remove_file(&path).ok();
    assert_eq!(loaded.project, engine.project);
}
