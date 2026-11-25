//! Differential checks of Animate's own media payloads (`bin/*.dat`, `anicraft_flamedia`)
//! against the local ground-truth corpus, run only when it is available:
//! `ANICRAFT_FLA_CORPUS=/path/to/corpus cargo test -p anicraft-xfl --test own_media_corpus`.
//!
//! The corpus files are EDAP Tools sample rigs (CC BY-NC-SA), so they stay out of the
//! repository; these tests observe what imports, they don't commit fixtures. Every modern twin
//! (zip `.fla`, or an XFL folder) keeps the same 88 × 31 Creative Commons badge only as
//! `bin/*.dat`, and the movie template keeps four sounds only as raw PCM there.

use std::path::{Path, PathBuf};

fn corpus() -> Option<PathBuf> {
    std::env::var_os("ANICRAFT_FLA_CORPUS").map(PathBuf::from)
}

/// Lists a folder of the corpus; one that can't be listed fails the test, so a corpus read
/// short can't pass by comparing less.
fn list(dir: &Path) -> Vec<PathBuf> {
    let entries = std::fs::read_dir(dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
    entries.map(|e| e.unwrap_or_else(|e| panic!("{}: {e}", dir.display())).path()).collect()
}

/// The corpus's modern twins: `*-animate.fla`, and the `.fla`/`.xfl` files in `*-animate/`.
/// A `*-animate.fla` whose `*-animate/` folder holds `.fla` files is the download those were
/// unpacked from (a zip of documents, not a document), so it isn't one; Mademoiselle's is a
/// zipped XFL folder, a document, and counts beside its unpacked copy.
fn twins(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for path in list(dir) {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string();
        if !name.contains("-animate") {
            continue;
        }
        if path.is_dir() {
            out.extend(list(&path).into_iter().filter(|inner| inner.extension().is_some_and(|e| e == "fla" || e == "xfl")));
        } else if path.extension().is_some_and(|e| e == "fla") {
            let unpacked = path.with_extension("");
            let bundle = unpacked.is_dir() && list(&unpacked).iter().any(|p| p.extension().is_some_and(|e| e == "fla"));
            if !bundle {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

#[test]
fn every_twins_badge_decodes_from_animates_own_bitmap_data() {
    let Some(dir) = corpus() else { return };
    let mut badges = Vec::new();
    let mut opened = Vec::new();
    for path in twins(&dir) {
        let (doc, report) = anicraft_xfl::import_path(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        opened.push(path.display().to_string());
        let summary = report.summary();
        assert!(!summary.contains("Animate's own format"), "{}: {summary}", path.display());
        let sizes: Vec<_> = doc.bitmaps.iter().map(|b| (b.name.as_str(), b.width, b.height)).collect();
        assert_eq!(sizes, [("logo CC88x31.png", 88, 31)], "{}", path.display());
        let px = doc.bitmaps[0].pixels().expect("pixels");
        assert_eq!(px.rgba.len(), 88 * 31 * 4);
        // The badge's ink is the well-known rich black #231F20, read in A,R,G,B order.
        assert!(px.rgba.chunks(4).any(|p| p == [0x23, 0x1f, 0x20, 0xff]), "{}", path.display());
        // Mostly opaque, with clear and faint (anti-aliased) pixels at its rounded corners, and
        // the middle opaque.
        let alpha = |a: u8| px.rgba.chunks(4).filter(|p| p[3] == a).count();
        assert!(alpha(255) > 88 * 31 * 3 / 4 && alpha(0) > 0, "{}: {} opaque, {} clear", path.display(), alpha(255), alpha(0));
        assert!(px.rgba.chunks(4).all(|p| p[3] == 255 || p[3] < 16), "{}: only edge pixels are see-through", path.display());
        let mid = (15 * 88 + 44) * 4;
        assert_eq!(px.rgba[mid + 3], 255, "{}: the middle is opaque", path.display());
        badges.push(px.rgba.clone());
    }
    // Every twin of the manifest's 9 pairs, by name: 5 Animate `.fla` files, Mademoiselle's
    // zipped XFL folder and its unpacked copy, and the RoboKit and Storyboard downloads' 3
    // `.fla` files.
    let expected = [
        "giskard-animate.fla",
        "kinefox-animate.fla",
        "mademoiselle-animate.fla",
        "mademoiselle-animate/EDAPT_SMR_MademoiselleD_v4a_CC.xfl",
        "magnebot-animate.fla",
        "movietemplate-animate.fla",
        "rabbit-animate.fla",
        "robokit-animate/EDAPT_SMR_FreeStyle_RoboKit_v1b_CC.fla",
        "storyboard-animate/EDAPT_Storyboard_Template_CC.fla",
        "storyboard-animate/EDAPT_Storyboard_Template_Sample_Animatic_CC.fla",
    ];
    let mut got: Vec<String> = opened.iter().map(|p| Path::new(p).strip_prefix(&dir).map_or_else(|_| p.clone(), |r| r.to_string_lossy().replace('\\', "/"))).collect();
    got.sort();
    assert_eq!(got, expected, "every twin the manifest pins opens");
    assert!(badges.windows(2).all(|w| w[0] == w[1]), "every twin's badge decodes to the same pixels");
}

#[test]
fn the_movie_templates_raw_pcm_sounds_come_across() {
    let Some(dir) = corpus() else { return };
    let (doc, report) = anicraft_xfl::import_path(&dir.join("movietemplate-animate.fla")).expect("imports");
    let summary = report.summary();
    assert!(!summary.contains("sounds in formats AniCraft can't decode"), "{summary}");
    let mut sounds: Vec<_> = doc.sounds.iter().map(|s| (s.name.clone(), s.rate, s.channels, s.frames)).collect();
    sounds.sort();
    assert_eq!(
        sounds,
        [
            ("Kevin MacLeod - Off to Osaka (mono)".to_string(), 44_100, 1, 901_680),
            ("blink1".to_string(), 44_100, 2, 12_376),
            ("blink2".to_string(), 44_100, 1, 7_072),
            ("maracas.wav".to_string(), 44_100, 2, 21_216),
        ]
    );
    for s in &doc.sounds {
        let d = s.decoded().expect("decodes");
        assert_eq!(d.pcm.frames() as u64, s.frames);
        // Sound, not silence; and not the noise a wrong byte order would make: neighbouring
        // samples stay close (MEDIA.md §3 measured 111–588 of 32768 on these four).
        let ch = usize::from(s.channels);
        let samples = &d.pcm.samples;
        let peak = samples.iter().fold(0f32, |m, v| m.max(v.abs()));
        assert!(peak > 0.01, "{}: silent", s.name);
        let steps: Vec<f32> = samples.iter().zip(samples.iter().skip(ch)).map(|(a, b)| (a - b).abs()).collect();
        let mean = steps.iter().sum::<f32>() / steps.len().max(1) as f32;
        assert!(mean * 32_768.0 < 2_000.0, "{}: mean step {}", s.name, mean * 32_768.0);
    }
    // The frames that play them still find them.
    let on_frames = doc.timelines().flat_map(|t| t.layers.iter()).flat_map(|l| l.keyframes.iter()).filter(|k| k.sound.is_some()).count();
    assert!(on_frames >= 5, "{on_frames} frame sounds");
}
