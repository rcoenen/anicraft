//! Bounds on hostile input: the container's header, FAT, directory and stream chains (as the
//! importer sees them through `craft-cfb`), the tag scanner's budget, and the shape rebuild's
//! indexes (which must not change its output).
//! Every file here is built from the format's grammar ([MS-CFB]), never from corpus bytes.

use super::cfb::{Compound, Error};
use super::records;
use super::shape::{Edge, Seg, ShapeRecord};
use super::tests::{entry_record, write};
use anicraft_doc::{Paint, Rgba, Stroke};
use anicraft_geom::{Affine, Path, Vec2};
use std::collections::HashMap;

const ENDOFCHAIN: u32 = 0xFFFF_FFFE;
const FREESECT: u32 = 0xFFFF_FFFF;

fn u32_at(f: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(f[at..at + 4].try_into().unwrap())
}

fn set_u32(f: &mut [u8], at: usize, v: u32) {
    f[at..at + 4].copy_from_slice(&v.to_le_bytes());
}

/// Where FAT entry `id` sits (the test writer's FAT is one sector, listed in the header's
/// first DIFAT slot).
fn fat_word(f: &[u8], id: u32) -> usize {
    (u32_at(f, 76) as usize + 1) * 512 + id as usize * 4
}

/// Where mini-FAT entry `id` sits.
fn minifat_word(f: &[u8], id: u32) -> usize {
    (u32_at(f, 60) as usize + 1) * 512 + id as usize * 4
}

fn damaged(r: Result<Compound<'_>, Error>) -> String {
    match r {
        Err(Error::Damaged(why)) => why,
        other => panic!("expected a damaged container, got {other:?}"),
    }
}

#[test]
fn a_huge_sector_shift_is_refused_before_it_is_used() {
    for (major, shift) in [(3u16, 200u16), (4, 64), (3, 0xFFFF), (4, 63)] {
        let mut f = write(&[("Contents", b"hi".to_vec())]);
        f[26..28].copy_from_slice(&major.to_le_bytes());
        f[30..32].copy_from_slice(&shift.to_le_bytes());
        assert!(matches!(Compound::new(&f), Err(Error::Damaged(_))), "version {major}, shift {shift}");
    }
}

#[test]
fn a_fat_sector_listed_twice_is_refused() {
    // In the header: the FAT's one sector, listed again in the second DIFAT slot.
    let mut f = write(&[("Contents", b"hi".to_vec())]);
    let fat = u32_at(&f, 76);
    set_u32(&mut f, 44, 2);
    set_u32(&mut f, 80, fat);
    assert!(damaged(Compound::new(&f)).contains("twice"));

    // In an extended DIFAT sector: the same sector 127 times, the count claiming them all
    // (a 1 GiB file of these would reserve a FAT far larger than the file).
    let mut f = write(&[("Contents", b"hi".to_vec())]);
    let fat = u32_at(&f, 76);
    f.resize(f.len() + 130 * 512, 0); // room enough for the count to be possible
    let difat = (f.len() / 512 - 1) as u32;
    let mut sector = vec![0u8; 512];
    for i in 0..127 {
        set_u32(&mut sector, i * 4, fat);
    }
    set_u32(&mut sector, 508, ENDOFCHAIN);
    f.extend_from_slice(&sector);
    set_u32(&mut f, 44, 128);
    set_u32(&mut f, 68, difat);
    set_u32(&mut f, 72, 1);
    assert!(damaged(Compound::new(&f)).contains("twice"));
}

#[test]
fn fat_sectors_past_the_file_or_more_than_it_could_hold_are_refused() {
    let mut f = write(&[("Contents", b"hi".to_vec())]);
    let past = (f.len() / 512 - 1) as u32; // the sector after the last
    set_u32(&mut f, 76, past);
    assert!(matches!(Compound::new(&f), Err(Error::Damaged(_))));

    // A count no file this size could hold is refused before anything is read for it.
    let mut f = write(&[("Contents", b"hi".to_vec())]);
    set_u32(&mut f, 44, u32::MAX);
    assert!(matches!(Compound::new(&f), Err(Error::Damaged(_))));
    let mut f = write(&[("Contents", b"hi".to_vec())]);
    let all = (f.len() / 512) as u32; // every sector, the header's included
    set_u32(&mut f, 44, all);
    // A count the file could hold but its DIFAT doesn't list costs nothing (`craft-cfb` reads
    // only the sectors listed) and leaves the streams readable.
    assert_eq!(Compound::new(&f).unwrap().stream("Contents").unwrap(), b"hi".to_vec());
}

#[test]
fn a_stream_chain_longer_than_its_size_reads_only_its_size() {
    // Through the FAT: a 5000-byte stream is sectors 0 to 9; its chain goes on into 10.
    let mut f = write(&[("Contents", vec![1; 5000])]);
    assert_eq!(Compound::new(&f).unwrap().stream("Contents").unwrap(), vec![1; 5000]);
    let at = fat_word(&f, 9);
    assert_eq!(u32_at(&f, at), ENDOFCHAIN);
    set_u32(&mut f, at, 10);
    assert_eq!(Compound::new(&f).unwrap().stream("Contents").unwrap(), vec![1; 5000]);
    // Looping back past its size is never followed: no second lap, the declared bytes.
    set_u32(&mut f, at, 0);
    assert_eq!(Compound::new(&f).unwrap().stream("Contents").unwrap(), vec![1; 5000]);

    // Through the mini-FAT: 100 bytes are mini sectors 0 and 1; the chain goes on into 2.
    let mut f = write(&[("Contents", vec![2; 100])]);
    assert_eq!(Compound::new(&f).unwrap().stream("Contents").unwrap(), vec![2; 100]);
    let at = minifat_word(&f, 1);
    assert_eq!(u32_at(&f, at), ENDOFCHAIN);
    set_u32(&mut f, at, 2);
    assert_eq!(Compound::new(&f).unwrap().stream("Contents").unwrap(), vec![2; 100]);
}

#[test]
fn a_stream_chain_that_ends_before_its_size_is_truncated() {
    let mut f = write(&[("Contents", vec![1; 5000])]);
    let at = fat_word(&f, 5);
    set_u32(&mut f, at, ENDOFCHAIN);
    assert!(matches!(Compound::new(&f).unwrap().stream("Contents"), Err(Error::Damaged(_))));

    let mut f = write(&[("Contents", vec![2; 200])]);
    let at = minifat_word(&f, 1);
    set_u32(&mut f, at, FREESECT);
    assert!(matches!(Compound::new(&f).unwrap().stream("Contents"), Err(Error::Damaged(_))));
}

#[test]
fn the_root_mini_stream_chain_is_read_to_its_size() {
    let mut f = write(&[("Contents", vec![3; 100])]);
    let root = entry_record(&f, "Root Entry");
    let first = u32_at(&f, root + 116);
    let at = fat_word(&f, first);
    assert_eq!(u32_at(&f, at), ENDOFCHAIN, "the writer's mini stream is one sector");
    set_u32(&mut f, at, first);
    assert_eq!(Compound::new(&f).unwrap().stream("Contents").unwrap(), vec![3; 100], "a looping tail is never followed");
    set_u32(&mut f, at, ENDOFCHAIN);
    // A root claiming more than the file holds reserves no more than the file (`craft-cfb`
    // reads the mini stream to the file's end at most), and its streams still read.
    f[root + 120..root + 128].copy_from_slice(&0x8000_0000u64.to_le_bytes());
    assert_eq!(Compound::new(&f).unwrap().stream("Contents").unwrap(), vec![3; 100]);
}

#[test]
fn empty_and_cutoff_sized_streams_still_read() {
    let payloads = vec![
        ("Contents", b"hi".to_vec()),
        ("Empty", Vec::new()),
        ("Under", vec![4; 4095]), // the mini stream's largest
        ("At", vec![5; 4096]),    // the cutoff: the FAT
        ("Whole", vec![6; 8 * 512]),
    ];
    let mut f = write(&payloads);
    let file = Compound::new(&f).unwrap();
    for (name, want) in &payloads {
        assert_eq!(&file.stream(name).unwrap(), want, "{name}");
    }
    // An empty stream reads nothing wherever its chain claims to start (writers put 0 there).
    let empty = entry_record(&f, "Empty");
    for start in [0, 5, ENDOFCHAIN, FREESECT] {
        set_u32(&mut f, empty + 116, start);
        assert_eq!(Compound::new(&f).unwrap().stream("Empty").unwrap(), Vec::<u8>::new(), "start {start:#x}");
    }
}

#[test]
fn a_stream_larger_than_the_file_is_refused_before_it_is_read() {
    let mut f = write(&[("Contents", vec![1; 5000])]);
    let record = entry_record(&f, "Contents");
    f[record + 120..record + 128].copy_from_slice(&(200u64 << 20).to_le_bytes());
    assert_eq!(Compound::new(&f).unwrap().stream("Contents"), Err(Error::StreamTooLarge));
}

/// A container whose `Contents` stream sits `depth` levels below the root (inside
/// `depth - 1` nested storages), written with `craft-cfb`'s own writer.
fn nested(depth: usize) -> Vec<u8> {
    let mut w = craft_cfb::Writer::new(craft_cfb::Version::V3);
    let mut parent = craft_cfb::Writer::ROOT;
    for i in 1..depth {
        parent = w.storage(parent, &format!("Storage {i}"), [0; 16]).unwrap();
    }
    w.stream(parent, "Contents", b"hi".to_vec()).unwrap();
    w.finish()
}

#[test]
fn storages_nested_past_the_depth_bound_are_refused_before_their_paths_are_built() {
    // At the bound the deepest stream still opens and reads, by its full path.
    let f = nested(64);
    let file = Compound::new(&f).expect("64 levels open");
    let deepest = file.entries().iter().find(|e| e.path.ends_with("/Contents")).expect("the deep stream is listed");
    assert_eq!(file.stream(&deepest.path.clone()).unwrap(), b"hi".to_vec());
    // One level deeper, and far deeper, is refused (without building a path per ancestor).
    for depth in [65, 2000] {
        assert!(matches!(Compound::new(&nested(depth)), Err(Error::OverLimit(_))), "depth {depth}");
    }
}

#[test]
fn full_paths_past_their_byte_budget_are_refused() {
    let f = write(&[("Contents", b"hi".to_vec()), ("S 1", b"a".to_vec()), ("S 2", b"b".to_vec())]);
    let total: usize = Compound::new(&f).unwrap().entries().iter().map(|e| e.path.len()).sum();
    assert!(Compound::open_within(&f, total).is_ok(), "a budget the paths fit in opens");
    assert!(matches!(Compound::open_within(&f, total - 1), Err(Error::OverLimit(_))), "one byte under is refused");
}

/// Length-prefixed record names, as a stream lays them out.
fn tags(names: &[&str]) -> Vec<u8> {
    let mut d = records::ITEM_PROLOGUE.to_vec();
    for n in names {
        d.extend_from_slice(&(n.len() as u16).to_le_bytes());
        d.extend_from_slice(n.as_bytes());
        d.push(0);
    }
    d
}

#[test]
fn the_tag_scan_stops_at_its_budget_and_says_so() {
    let d = tags(&["CPicPage", "CPicLayer", "CPicFrame"]);
    let all = records::scan_tags(&d);
    assert!(all.complete);
    assert_eq!(all.tags.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(), ["CPicPage", "CPicLayer", "CPicFrame"]);
    let exact = records::scan_tags_max(&d, 3);
    assert!(exact.complete, "exactly the budget is the whole scan");
    assert_eq!(exact.tags, all.tags);
    let cut = records::scan_tags_max(&d, 2);
    assert!(!cut.complete, "a scan that stopped early says so");
    assert_eq!(cut.tags, all.tags[..2]);

    // A stream of nothing but tags keeps at most the budget, and the dump says so.
    let flood = tags(&vec!["CPicShape"; records::MAX_TAGS + 10]);
    let scan = records::scan_tags(&flood);
    assert_eq!((scan.tags.len(), scan.complete), (records::MAX_TAGS, false));
    let dumped = super::dump(&write(&[("S 1 1", flood)])).unwrap();
    let s = dumped.iter().find(|s| s.path == "S 1 1").unwrap();
    assert_eq!((s.tags.len(), s.tags_complete), (records::MAX_TAGS, false));
    let dumped = super::dump(&write(&[("S 1 1", d)])).unwrap();
    assert!(dumped.iter().all(|s| s.tags_complete));
}

#[test]
fn the_dump_shares_one_tag_budget_across_streams() {
    // Two streams of three-quarters of the budget each: the first is whole, the second keeps
    // what is left and says it stopped early.
    let three_quarters = records::MAX_TAGS / 4 * 3;
    let flood = tags(&vec!["CPicShape"; three_quarters]);
    let dumped = super::dump(&write(&[("S 1 1", flood.clone()), ("S 2 2", flood)])).unwrap();
    let get = |path: &str| dumped.iter().find(|s| s.path == path).map(|s| (s.tags.len(), s.tags_complete)).unwrap();
    assert_eq!(get("S 1 1"), (three_quarters, true));
    assert_eq!(get("S 2 2"), (records::MAX_TAGS - three_quarters, false));
}

// The shape rebuild before its indexes, kept here as the oracle the indexed one must match
// exactly: one style's fill segments gathered by scanning every edge, and each chain step's
// next segment found by scanning its vertex's list from the start.

fn key(p: Vec2) -> (i64, i64) {
    ((p.x * 100.0).round() as i64, (p.y * 100.0).round() as i64)
}

fn push(path: &mut Path, s: &Seg) {
    match s.ctrl {
        Some(c) => path.quad_to(c, s.to),
        None => path.line_to(s.to),
    };
}

fn oracle_fill_path(rec: &ShapeRecord, style: u16) -> Path {
    let mut segs: Vec<Seg> = Vec::new();
    for e in &rec.edges {
        if e.fill_right == style {
            segs.push(e.seg);
        }
        if e.fill_left == style {
            segs.push(Seg { from: e.seg.to, ctrl: e.seg.ctrl, to: e.seg.from });
        }
    }
    let mut by_start: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
    for (i, s) in segs.iter().enumerate() {
        if key(s.from) != key(s.to) {
            by_start.entry(key(s.from)).or_default().push(i);
        }
    }
    let mut used = vec![false; segs.len()];
    let mut path = Path::new();
    for (first, start) in segs.iter().enumerate() {
        if used[first] || key(start.from) == key(start.to) {
            continue;
        }
        path.move_to(start.from);
        let mut at = first;
        while let Some(s) = segs.get(at) {
            used[at] = true;
            push(&mut path, s);
            if key(s.to) == key(start.from) {
                break;
            }
            match by_start.get(&key(s.to)).and_then(|v| v.iter().copied().find(|&j| !used[j])) {
                Some(j) => at = j,
                None => break,
            }
        }
        path.close();
    }
    path
}

fn oracle_stroke_path(rec: &ShapeRecord, style: u16) -> Path {
    let mut path = Path::new();
    let mut last: Option<Vec2> = None;
    for e in rec.edges.iter().filter(|e| e.stroke == style) {
        if last.is_none_or(|l| key(l) != key(e.seg.from)) {
            path.move_to(e.seg.from);
        }
        push(&mut path, &e.seg);
        last = Some(e.seg.to);
    }
    path
}

/// The fill, stroke and path of each shape the oracle makes, in its order.
fn oracle_shapes(rec: &ShapeRecord) -> Vec<String> {
    let mut out = Vec::new();
    for (i, paint) in rec.fills.iter().enumerate() {
        let Some(paint) = paint else { continue };
        let path = oracle_fill_path(rec, i as u16 + 1);
        if !path.is_empty() {
            out.push(format!("{:?} {:?} {:?}", Some(paint), None::<Stroke>, path));
        }
    }
    for (i, stroke) in rec.strokes.iter().enumerate() {
        let path = oracle_stroke_path(rec, i as u16 + 1);
        if !path.is_empty() {
            out.push(format!("{:?} {:?} {:?}", None::<Paint>, Some(stroke), path));
        }
    }
    out
}

fn shapes_of(rec: &ShapeRecord) -> Vec<String> {
    rec.shapes().iter().map(|s| format!("{:?} {:?} {:?}", s.fill, s.stroke, s.path.as_ref())).collect()
}

/// A shape whose edges crowd a small lattice: many share each vertex, both sides carry
/// fills (some on both, some naming a style the record doesn't list), strokes interleave,
/// and some edges are curves or have no length.
fn crowded_shape(seed: u64, edges: usize) -> ShapeRecord {
    let mut state = seed;
    let mut next = move |n: u64| {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (state >> 33) % n
    };
    let point = |i: u64| Vec2::new((i % 4) as f32 * 10.0, (i / 4) as f32 * 10.0);
    let mut list = Vec::new();
    let mut pen = 0u64;
    for _ in 0..edges {
        let to = if next(10) == 0 { pen } else { next(16) };
        let ctrl = (next(4) == 0).then(|| Vec2::new(next(40) as f32, next(40) as f32));
        let (stroke, fill_left, fill_right) = (next(4) as u16, next(5) as u16, next(5) as u16);
        list.push(Edge { stroke, fill_left, fill_right, seg: Seg { from: point(pen), ctrl, to: point(to) } });
        pen = if next(3) == 0 { next(16) } else { to };
    }
    let solid = |v: f32| Some(Paint::Solid(Rgba::new(v, 0.0, 0.0, 1.0)));
    ShapeRecord {
        matrix: Affine::IDENTITY,
        fills: vec![solid(0.25), None, solid(0.75)], // edges name style 4 too: listed by none
        strokes: vec![Stroke::new(Rgba::new(0.0, 0.0, 1.0, 1.0), 1.0), Stroke::new(Rgba::new(0.0, 1.0, 0.0, 1.0), 2.0)],
        stroke_kinds: vec![crate::shape::StrokeKind::Solid; 2],
        edges: list,
        approximations: Vec::new(),
        end: 0,
    }
}

#[test]
fn the_indexed_shape_rebuild_matches_the_scanning_one() {
    for seed in 0..40 {
        let rec = crowded_shape(seed, 10 + seed as usize * 7);
        for style in 0..=5 {
            assert_eq!(rec.fill_path(style), oracle_fill_path(&rec, style), "seed {seed}, fill {style}");
            assert_eq!(rec.stroke_path(style), oracle_stroke_path(&rec, style), "seed {seed}, stroke {style}");
        }
        assert_eq!(shapes_of(&rec), oracle_shapes(&rec), "seed {seed}");
    }
}

#[test]
fn many_edges_through_one_vertex_rebuild_the_same() {
    // A fan of triangles around the origin, alternating two fills: every one passes
    // through the centre twice.
    let n = 1500;
    let at = |i: usize| {
        let a = i as f32 / n as f32 * std::f32::consts::TAU;
        Vec2::new(a.cos() * 100.0, a.sin() * 100.0)
    };
    let mut edges = Vec::new();
    for i in 0..n {
        let (a, b) = (at(i), at(i + 1));
        let fill = 1 + (i % 2) as u16 * 2;
        for (from, to) in [(Vec2::ZERO, a), (a, b), (b, Vec2::ZERO)] {
            edges.push(Edge { stroke: 1, fill_left: 0, fill_right: fill, seg: Seg { from, ctrl: None, to } });
        }
    }
    let mut rec = crowded_shape(0, 0);
    rec.edges = edges;
    let got = shapes_of(&rec);
    assert_eq!(got.len(), 3, "two fills and a stroke");
    assert_eq!(got, oracle_shapes(&rec));
}

#[test]
fn each_dumped_stream_is_its_own_entry_even_when_a_damaged_file_repeats_a_path() {
    // Two entries with one path: each dumps its own size and its own bytes.
    let mut f = write(&[("Contents", vec![1; 8]), ("Contentz", vec![2; 100])]);
    let second = entry_record(&f, "Contentz");
    f[second + 14] = b's'; // "Contentz" → "Contents" (UTF-16, the 8th unit)
    let dumped = crate::dump(&f).expect("dumps");
    let sizes: Vec<u64> = dumped.iter().map(|s| s.size).collect();
    assert!(sizes.contains(&8) && sizes.contains(&100), "{sizes:?}");
    for s in &dumped {
        let want = if s.size == 8 { "01 01 01 01 01 01 01 01" } else { "02 02 02 02 02 02 02 02" };
        assert_eq!(s.head, want, "the {}-byte entry shows its own bytes", s.size);
    }
}

#[test]
fn a_dump_of_one_stream_reads_only_that_stream() {
    let mut f = write(&[("Contents", b"hello".to_vec()), ("Other", vec![7; 5000])]);
    let other = entry_record(&f, "Other");
    f[other + 120..other + 128].copy_from_slice(&0x7FFF_FFFFu64.to_le_bytes()); // past the file
    assert!(crate::dump(&f).is_err(), "the damaged stream fails a whole dump");
    let one = crate::dump_only(&f, Some("Contents")).expect("but not one of another stream");
    assert_eq!(one.iter().map(|s| s.path.as_str()).collect::<Vec<_>>(), ["Contents"]);
    assert!(matches!(crate::dump_only(&f, Some("Nowhere")), Err(Error::NotFound(_))), "a stream it doesn't have is said");
}
