//! The object graph's recovery paths and the mapping's guards on hostile input (ADR 0120):
//! skipping undecoded bytes in linear work without losing count of the objects in them,
//! references as siblings, groups used more than once or inside themselves, and the
//! `Contents` scans. Every byte here is written from the grammar in `crate::archive` and
//! `crate::doc`, never taken from a corpus file.

use crate::tests::{one_frame_stream, properties, rectangle_body, scene, synthetic_contents, write, Ar, FrameSpec, InstSpec};
use crate::text_tests::{record, units, Head, Style};
use anicraft_doc::{Element, ElementKind};
use std::sync::Arc;

fn utf16(s: &str) -> Vec<u8> {
    s.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

fn frame_spec(duration: u16) -> FrameSpec {
    let mut spec = FrameSpec::default();
    spec.duration = duration;
    spec
}

/// A document of the synthetic `Contents`, the scene page and the symbol its instances place.
fn open(page: Vec<u8>) -> (anicraft_doc::Document, crate::Report) {
    let payloads = vec![("Contents", synthetic_contents()), ("P 1 1", page), ("Symbol 9", one_frame_stream(&rectangle_body()))];
    crate::doc::import(&write(&payloads)).expect("opens")
}

/// A page of one layer, "art", whose one keyframe holds what `objects` writes.
fn page_with(objects: impl FnOnce(&mut Ar)) -> Vec<u8> {
    let mut w = Ar::new();
    w.page(|w| {
        w.layer("art", 0, None, |w| w.frame(&[], &frame_spec(4), objects));
    });
    w.out
}

/// The number the last object written took (one with no children of its own).
fn last(w: &Ar) -> u16 {
    w.count - 1
}

fn skipped(report: &crate::Report, what: &str) -> usize {
    report.skipped.get(what).copied().unwrap_or(0)
}

fn kinds(elements: &[Arc<Element>]) -> Vec<&'static str> {
    elements.iter().map(|e| match &e.kind {
        ElementKind::Symbol { .. } => "symbol",
        ElementKind::Text(_) => "text",
        ElementKind::Shape(_) => "shape",
        ElementKind::Group { .. } => "group",
        _ => "other",
    }).collect()
}

// Skipping undecoded bytes.

#[test]
fn skipping_past_many_class_declarations_takes_linear_work_and_numbers_each_once() {
    // An object of a class not decoded, its 6 MB holding 15 000 declarations of a class that
    // doesn't parse there: each is tried as the next sibling and fails. The skip numbers them
    // once each as it passes, not again from the start for every later trial (about a minute
    // of work here), so the instance after them and a back-reference to it still line up.
    const FAKES: u16 = 15_000;
    let page = page_with(|w| {
        w.tag("CPicFuture");
        w.out.extend([0x04, 0x00]);
        for _ in 0..FAKES {
            w.out.extend([0xFF, 0xFF, 0x01, 0x00, 0x08, 0x00]);
            w.out.extend(b"CPicPage");
            w.out.extend([0x04, 0x00, 0xFE, 0xFF]);
            w.out.extend([0u8; 400]);
        }
        // Each declared a class and an object of it.
        w.count += 2 * FAKES;
        let mut inst = InstSpec::default();
        inst.first_frame = 2;
        w.instance(&inst);
        let n = last(w);
        w.u16(n);
    });
    let (doc, report) = open(page);
    let k = &scene(&doc).layers[0].keyframes[0];
    assert_eq!(kinds(&k.elements), ["symbol", "symbol"], "the instance and the reference to it");
    for e in &k.elements {
        assert!(matches!(e.kind, ElementKind::Symbol { first_frame: 2, .. }), "{:?}", e.kind);
    }
    assert_eq!(skipped(&report, "objects of a kind not decoded (left out)"), 1, "{:?}", report.skipped);
}

#[test]
fn an_object_of_a_known_class_that_does_not_parse_is_never_skipped_over() {
    // An object not decoded holds an instance (its class declared before) that doesn't parse:
    // that instance took a number, and what it holds may have taken more, which a skip can't
    // count. Skipping past it would leave every later number one off, and the reference to `b`
    // at the end would find `c`; instead the timeline is refused and the scene comes in by its
    // layer names.
    let page = page_with(|w| {
        let mut a = InstSpec::default();
        a.first_frame = 1;
        w.instance(&a);
        w.tag("CPicFuture");
        w.out.extend([0x04, 0x00]);
        w.tag("CPicSymbol");
        w.out.extend([0x04, 0x00, 0xFE, 0xFF]);
        let (mut b, mut c) = (InstSpec::default(), InstSpec::default());
        b.first_frame = 2;
        c.first_frame = 3;
        w.instance(&b);
        let b_number = last(w);
        w.instance(&c);
        w.u16(b_number);
    });
    let (doc, report) = open(page);
    assert_eq!(skipped(&report, "scenes whose timeline didn't decode (layer names only)"), 1, "{:?}", report.skipped);
    let tl = scene(&doc);
    assert_eq!(tl.layers.iter().map(|l| l.name.as_str()).collect::<Vec<_>>(), ["art"]);
    assert!(tl.layers.iter().all(|l| l.keyframes.iter().all(|k| k.elements.is_empty())), "nothing placed by a wrong number");
}

// References where a parent continues.

#[test]
fn a_reference_after_a_text_a_bitmap_or_a_button_is_their_next_sibling() {
    let page = page_with(|w| {
        let mut inst = InstSpec::default();
        inst.first_frame = 1;
        w.instance(&inst);
        let i = last(w);
        // A dynamic text, then the instance again by reference.
        w.tag("CPicText");
        let head = Head { field: 0x0003, matrix: [65536, 0, 0, 65536, 400, 600], bounds: [0, 2000, 0, 400], ..Head::default() };
        w.out.extend(record(&head, &[(units("Score"), Style::default())]));
        w.u16(i);
        // A bitmap instance (its media isn't in this file), then the reference.
        w.object("CPicBitmap", 0, None, 0, 0x02, |_| {});
        for v in [65536, 0, 0, 65536, 0, 0] {
            w.i32(v);
        }
        w.u16(7);
        w.out.push(0);
        w.u16(i);
        // A button with its settings, then the reference (the last child: the frame's end next).
        let mut button = InstSpec::default();
        button.class = "CPicButton";
        w.instance(&button);
        w.out.push(0x0B);
        w.out.extend_from_slice(&[4, 0, 0, 0, 1, 0, 0, 0, 0x10, 0x20, 0, 0, 0, 0, 0, 0]);
        w.string("");
        w.out.push(0);
        w.string("");
        w.i32(0);
        w.u16(i);
    });
    let (doc, report) = open(page);
    let k = &scene(&doc).layers[0].keyframes[0];
    assert_eq!(kinds(&k.elements), ["symbol", "text", "symbol", "symbol", "symbol", "symbol"], "{:?}", report.skipped);
    for what in ["texts whose records didn't decode", "bitmap instances whose records didn't decode", "button settings in a layout not decoded (left out)"] {
        assert_eq!(skipped(&report, what), 0, "{what}: {:?}", report.skipped);
    }
    assert_eq!(skipped(&report, "instances of missing bitmaps"), 1, "{:?}", report.skipped);
}

#[test]
fn a_recovered_keyframe_followed_by_a_reference_keeps_its_fields() {
    // The second keyframe's artwork uses a fill kind not decoded (its fields are found by trial
    // parse), and the layer then lists the first keyframe again by reference.
    let mut bad = rectangle_body();
    bad[24 + 1 + 4 + 2 + 4] = 0x40; // the fill's kind: a bitmap fill
    let mut w = Ar::new();
    w.page(|w| {
        w.layer("art", 0, None, |w| {
            let mut first = 0u16;
            w.frame(&[], &frame_spec(2), |w| first = w.count - 1);
            w.frame(&bad, &frame_spec(3), |_| {});
            w.u16(first);
        });
    });
    let (doc, report) = open(w.out);
    let spans: Vec<(u32, u32)> = scene(&doc).layers[0].keyframes.iter().map(|k| (k.start, k.duration)).collect();
    assert_eq!(spans, [(0, 2), (2, 3), (5, 2)], "{:?}", report.skipped);
    assert_eq!(skipped(&report, "keyframes that didn't decode"), 0, "{:?}", report.skipped);
    assert_eq!(skipped(&report, "shapes with bitmap fills (decode pending)"), 1, "{:?}", report.skipped);
}

#[test]
fn references_to_classes_are_not_siblings() {
    // A text followed by the number of `CPicText`'s class slot (a class, not an object): not
    // where the frame continues, so the text is skipped to the frame's end.
    let page = page_with(|w| {
        w.tag("CPicText");
        let class_slot = w.count - 2;
        let head = Head { field: 0x0003, ..Head::default() };
        w.out.extend(record(&head, &[(units("x"), Style::default())]));
        w.u16(class_slot);
    });
    let (_, report) = open(page);
    assert_eq!(skipped(&report, "texts whose records didn't decode"), 1, "{:?}", report.skipped);
}

// Groups used more than once.

/// A group: a `CPicShape` with no artwork of its own whose members `members` writes (given
/// the group's own number).
fn group(w: &mut Ar, members: impl FnOnce(&mut Ar, u16)) -> u16 {
    let n = w.object("CPicShape", 0, None, 0, 0x03, |w| {
        let me = w.count - 1;
        members(w, me);
    });
    w.empty_shape();
    w.i32(0); // no cubic hints
    n
}

/// A drawing object holding the rectangle.
fn drawing(w: &mut Ar) {
    w.object("CPicShape", 0, None, 1, 0x03, |_| {});
    w.out.extend(rectangle_body());
    w.i32(0);
}

/// Groups nested `depth` deep, each holding the one inside it twice (written, then by
/// reference); the innermost holds what `inner` writes.
fn shared(w: &mut Ar, depth: u32, inner: fn(&mut Ar)) -> u16 {
    if depth == 0 {
        return group(w, |w, _| inner(w));
    }
    group(w, |w, _| {
        let n = shared(w, depth - 1, inner);
        w.u16(n);
    })
}

fn shapes_in(elements: &[Arc<Element>]) -> usize {
    elements.iter().map(|e| match &e.kind {
        ElementKind::Shape(_) => 1,
        ElementKind::Group { .. } => shapes_in(&e.kind.group_members()),
        _ => 0,
    }).sum()
}

#[test]
fn a_group_used_twice_comes_in_twice() {
    // Two levels of a group listed twice: four rectangles, nothing left out.
    let (doc, report) = open(page_with(|w| {
        shared(w, 2, drawing);
    }));
    let k = &scene(&doc).layers[0].keyframes[0];
    assert_eq!(kinds(&k.elements), ["group"]);
    assert_eq!(shapes_in(&k.elements), 4);
    assert!(report.skipped.is_empty(), "{:?}", report.skipped);
}

#[test]
fn a_group_that_contains_itself_comes_in_once_and_says_so() {
    let (doc, report) = open(page_with(|w| {
        group(w, |w, me| {
            drawing(w);
            w.u16(me);
            w.u16(me);
        });
    }));
    let k = &scene(&doc).layers[0].keyframes[0];
    assert_eq!(kinds(&k.elements), ["group"]);
    assert_eq!(shapes_in(&k.elements), 1);
    assert_eq!(skipped(&report, "groups that contain themselves (left out)"), 2, "{:?}", report.skipped);
}

#[test]
fn groups_shared_exponentially_import_in_bounded_work() {
    // 40 levels, each holding the next twice: 41 objects that stand for 2^40 uses. The
    // mapping stops at its budget and says so (it would otherwise never finish).
    let (doc, report) = open(page_with(|w| {
        shared(w, 40, |_| {});
    }));
    assert_eq!(scene(&doc).layers.len(), 1);
    assert!(skipped(&report, "objects past a million in one document (left out)") > 0, "{:?}", report.skipped);
}

/// A drawing whose one record holds three fills, so it emits three shapes.
fn three_fills(w: &mut Ar) {
    w.object("CPicShape", 0, None, 1, 0x03, |_| {});
    w.out.extend(crate::tests::three_squares_body());
    w.i32(0);
}

#[test]
fn every_shape_a_record_emits_is_charged_against_the_object_allowance() {
    // Four uses of a three-fill drawing, in groups two levels deep: 11 objects visited and 12
    // shapes emitted, 24 with the library's rectangle, and the two keyframes they sit on.
    let file = || {
        let payloads = vec![("Contents", synthetic_contents()), ("P 1 1", page_with(|w| {
            shared(w, 2, three_fills);
        })), ("Symbol 9", one_frame_stream(&rectangle_body()))];
        write(&payloads)
    };
    let (doc, report) = crate::doc::import_within(&file(), 26, usize::MAX).expect("opens");
    assert_eq!(shapes_in(&scene(&doc).layers[0].keyframes[0].elements), 12);
    assert!(report.skipped.is_empty(), "{:?}", report.skipped);
    // With room for 15, the visits alone (12 with the library's, and the two keyframes) would
    // have fit and all 12 shapes come in; the shapes count too, so they stop at the allowance
    // and say so.
    let (doc, report) = crate::doc::import_within(&file(), 15, usize::MAX).expect("opens");
    let shapes = shapes_in(&scene(&doc).layers[0].keyframes[0].elements);
    assert_eq!(shapes, 6);
    assert!(skipped(&report, "objects past a million in one document (left out)") > 0, "{:?}", report.skipped);
}

#[test]
fn every_rebuilt_shape_record_is_charged_its_edges_before_the_rebuild() {
    // The same drawing rebuilt at each of its four uses: its edges are charged each time, so a
    // record placed many times can't rebuild its geometry without end.
    let file = write(&[("Contents", synthetic_contents()), ("P 1 1", page_with(|w| {
        shared(w, 2, three_fills);
    })), ("Symbol 9", one_frame_stream(&rectangle_body()))]);
    let full = |edges: usize| {
        let (doc, report) = crate::doc::import_within(&file, usize::MAX, edges).expect("opens");
        (shapes_in(&scene(&doc).layers[0].keyframes[0].elements), report)
    };
    let (all, report) = full(usize::MAX);
    assert_eq!(all, 12);
    assert!(report.skipped.is_empty(), "{:?}", report.skipped);
    // The budget the fixture needs, from its records: the drawing's edges at each of its four
    // uses, and the library rectangle's once.
    let edges = |body: Vec<u8>| crate::shape::find_all(&one_frame_stream(&body)).first().map(|r| r.edges.len()).expect("the record decodes");
    let enough = 4 * edges(crate::tests::three_squares_body()) + edges(rectangle_body());
    let (at_budget, report) = full(enough);
    assert_eq!(at_budget, all, "exactly the edges rebuilt fit");
    assert!(report.skipped.is_empty(), "{:?}", report.skipped);
    let (fewer, report) = full(enough - 1);
    assert!(fewer < all, "one edge short leaves a use out");
    assert!(skipped(&report, "shapes past the import's geometry budget (left out)") > 0, "{:?}", report.skipped);
}

// The `Contents` scans.

#[test]
fn a_library_stream_name_without_a_label_does_not_stall_the_scan() {
    // `01 80 17`, a one-unit name, then nothing that reads as a label within the record: the
    // scan moves on, and finds the entry after it.
    let mut c = vec![0x01, 0x80, 0x17, 0x01];
    c.extend(utf16("S"));
    c.extend([0u8; 300]);
    c.extend([0x01, 0x80, 0x17, 0x08]);
    c.extend(utf16("Symbol 9"));
    c.extend([0xFF, 0xFE, 0xFF, 0x04]);
    c.extend(utf16("rays"));
    c.extend(9u32.to_le_bytes());
    c.push(2);
    c.extend([0u8; 8]);
    let items = crate::doc::library_items(&c);
    let got: Vec<(&str, &str, u32, u8)> = items.iter().map(|i| (i.stream.as_str(), i.label.as_str(), i.id, i.kind)).collect();
    assert_eq!(got, [("Symbol 9", "rays", 9, 2)]);
}

/// `Contents` with one `CDocumentPage` record per label (stream `P <n> 1`), each label right
/// behind its record's stream name and the last at the stream's very end.
fn contents_with_scenes(labels: &[&str]) -> Vec<u8> {
    let mut c = vec![0x3F, 0x01, 0x00, 0x00, 0x00];
    for (n, label) in labels.iter().enumerate() {
        c.extend([0xFF, 0xFF, 0x01, 0x00]);
        c.extend(13u16.to_le_bytes());
        c.extend(b"CDocumentPage");
        c.extend([0x17, 0x00]);
        c.extend(utf16(&format!("P {} 1", n + 1)));
        c.extend([0xFF, 0xFE, 0xFF, label.encode_utf16().count() as u8]);
        c.extend(utf16(label));
    }
    c
}

fn scene_names(contents: Vec<u8>) -> Vec<String> {
    let (doc, _) = crate::doc::import(&write(&[("Contents", contents)])).expect("opens");
    doc.scenes.iter().map(|s| s.name.clone()).collect()
}

#[test]
fn a_scene_label_outside_ascii_advances_by_its_stored_units() {
    // 東京 is 2 UTF-16 units (4 bytes in the stream) but 6 bytes of UTF-8: the next record
    // right behind it is still found, and a label at the very end doesn't read past it.
    assert_eq!(scene_names(contents_with_scenes(&["東京", "Scene 2"])), ["東京", "Scene 2"]);
    assert_eq!(scene_names(contents_with_scenes(&["東京"])), ["東京"]);
    assert_eq!(scene_names(contents_with_scenes(&["Scene 1", "Scene 2"])), ["Scene 1", "Scene 2"]);
}

#[test]
fn a_stage_rectangle_of_extreme_edges_is_no_stage_and_never_overflows() {
    let s = crate::doc::stage(&properties([0, 11_000, 0, 8_000], 24 << 16)).expect("a stage");
    assert_eq!((s.width, s.height, s.fps), (550, 400, 24.0));
    for rect in [[i32::MIN, i32::MAX, i32::MIN, i32::MAX], [i32::MAX, i32::MIN, i32::MAX, i32::MIN], [0, i32::MAX, 0, 8_000], [i32::MIN, 0, 0, 8_000]] {
        assert!(crate::doc::stage(&properties(rect, 24 << 16)).is_none(), "{rect:?}");
    }
}

#[test]
fn a_page_child_that_is_not_a_layer_it_decodes_is_reported() {
    let file = |extra: bool| {
        let mut w = Ar::new();
        w.page(|w| {
            w.layer("art", 0, None, |w| w.frame(&[], &frame_spec(4), |_| {}));
            if extra {
                // A kind of layer this importer doesn't know: skipped over, and said so.
                w.object("CPicLayerOfTheFuture", 0, None, 0, 0x0B, |_| {});
            }
        });
        write(&[("Contents", synthetic_contents()), ("P 1 1", w.out), ("Symbol 9", one_frame_stream(&rectangle_body()))])
    };
    let (_, report) = crate::doc::import(&file(false)).expect("opens");
    assert_eq!(skipped(&report, "layers that didn't decode (left out)"), 0);
    let (doc, report) = crate::doc::import(&file(true)).expect("opens");
    assert_eq!(scene(&doc).layers.len(), 1, "the layer that decodes still comes in");
    assert_eq!(skipped(&report, "layers that didn't decode (left out)"), 1, "{:?}", report.skipped);
}

#[test]
fn keyframes_past_the_last_frame_are_left_out_not_stacked_on_it() {
    // Two frames of 65,535 reach past frame 100,000, so the third starts beyond it.
    let file = write(&[("Contents", synthetic_contents()), ("P 1 1", {
        let mut w = Ar::new();
        w.page(|w| {
            w.layer("art", 0, None, |w| {
                for _ in 0..4 {
                    w.frame(&rectangle_body(), &frame_spec(u16::MAX), |_| {});
                }
            });
        });
        w.out
    }), ("Symbol 9", one_frame_stream(&rectangle_body()))]);
    let (doc, report) = crate::doc::import(&file).expect("opens");
    let starts: Vec<u32> = scene(&doc).layers[0].keyframes.iter().map(|k| k.start).collect();
    let mut unique = starts.clone();
    unique.dedup();
    assert_eq!(unique, starts, "no two keyframes share a start: {starts:?}");
    assert!(starts.len() < 4, "{starts:?}");
    assert!(skipped(&report, "keyframes past frame 100,000 (left out)") > 0, "{:?}", report.skipped);
}

// The document-wide allowances: what a small hostile file can't multiply.

/// The synthetic `Contents` listing the "Symbol 9" stream as `n` more library symbols.
fn contents_naming_one_stream(n: u32) -> Vec<u8> {
    let mut c = synthetic_contents();
    for k in 0..n {
        c.extend([0x01, 0x80, 0x17, 0x08]);
        c.extend(utf16("Symbol 9"));
        c.extend([0xFF, 0xFE, 0xFF, 0x04]);
        c.extend(utf16("copy"));
        c.extend((100 + k).to_le_bytes());
        c.push(0x00);
        c.extend([0u8; 8]);
    }
    c
}

#[test]
fn one_stream_named_by_many_symbols_is_read_within_the_files_allowance() {
    // A symbol stream of a few kilobytes that `Contents` lists thousands of times: reading and
    // decoding it for each would cost far more than the file holds.
    let stream = {
        let mut w = Ar::new();
        w.page(|w| {
            for _ in 0..40 {
                w.layer("artwork", 0, None, |w| w.frame(&rectangle_body(), &frame_spec(1), |_| {}));
            }
        });
        w.out
    };
    let file = write(&[("Contents", contents_naming_one_stream(4000)), ("P 1 1", page_with(|_| {})), ("Symbol 9", stream.clone())]);
    assert!(4000 * stream.len() > 2 * file.len(), "the fixture asks for more than its allowance");
    let (doc, report) = crate::doc::import(&file).expect("opens");
    let left_empty = skipped(&report, "library symbols past the import's reading budget (left empty)");
    assert!(left_empty > 0, "{:?}", report.skipped);
    let decoded = doc.library.iter().filter(|s| s.timeline.layers.len() == 40).count();
    assert!(decoded > 0 && decoded * stream.len() <= 2 * file.len(), "{decoded} symbols decoded");
    assert_eq!(decoded + left_empty, doc.library.len(), "every symbol is there, decoded or named as left empty");
}

#[test]
fn keyframes_and_text_characters_are_charged_against_the_object_allowance() {
    // Five empty keyframes: each is an object of the allowance, so three fit.
    let frames = {
        let mut w = Ar::new();
        w.page(|w| {
            w.layer("art", 0, None, |w| {
                for _ in 0..5 {
                    w.frame(&[], &frame_spec(1), |_| {});
                }
            });
        });
        w.out
    };
    // The library symbol's own keyframe takes one of the allowance first.
    let (doc, report) = crate::doc::import_within(&write(&[("Contents", synthetic_contents()), ("P 1 1", frames), ("Symbol 9", one_frame_stream(&[]))]), 4, usize::MAX).expect("opens");
    assert_eq!(scene(&doc).layers[0].keyframes.len(), 3);
    assert!(skipped(&report, "objects past a million in one document (left out)") > 0, "{:?}", report.skipped);
    // A text is charged its characters too: one of forty doesn't fit an allowance of twenty.
    let text = |chars: &str| {
        let chars = chars.to_string();
        page_with(move |w| {
            w.tag("CPicText");
            let head = Head { matrix: [65536, 0, 0, 65536, 0, 0], bounds: [0, 2000, 0, 400], ..Head::default() };
            w.out.extend(record(&head, &[(units(&chars), Style::default())]));
        })
    };
    let texts = |page: Vec<u8>| {
        let (doc, report) = crate::doc::import_within(&write(&[("Contents", synthetic_contents()), ("P 1 1", page), ("Symbol 9", one_frame_stream(&[]))]), 20, usize::MAX).expect("opens");
        (scene(&doc).layers[0].keyframes[0].elements.len(), skipped(&report, "objects past a million in one document (left out)"))
    };
    assert_eq!(texts(text("short")), (1, 0));
    let (kept, left_out) = texts(text(&"long ".repeat(8)));
    assert!(kept == 0 && left_out > 0);
}

#[test]
fn a_stream_is_charged_every_byte_it_reads_whichever_decoder_reads_it() {
    // An item stream, one that is mostly a shape record, and one that is mostly a text: each
    // decodes within a few times its length, and none within half of it, so the walker, the
    // shape decoder and the text decoder all charge the stream's work budget.
    let layers = {
        let mut w = Ar::new();
        w.page(|w| {
            for _ in 0..20 {
                w.layer("a layer with a name", 0, None, |w| w.frame(&[], &frame_spec(1), |_| {}));
            }
        });
        w.out
    };
    let shape = one_frame_stream(&crate::tests::three_squares_body());
    let text = page_with(|w| {
        w.tag("CPicText");
        let head = Head { matrix: [65536, 0, 0, 65536, 0, 0], bounds: [0, 2000, 0, 400], ..Head::default() };
        w.out.extend(record(&head, &[(units(&"words ".repeat(100)), Style::default())]));
    });
    for stream in [layers, shape, text] {
        assert!(crate::archive::parse_within(&stream, 4 * stream.len()).is_ok());
        assert!(crate::archive::parse_within(&stream, stream.len() / 2).is_err());
    }
}

#[test]
fn a_document_without_stage_properties_says_the_defaults_are_used() {
    let mut contents = synthetic_contents();
    let name = utf16("legacyLineSpacing");
    let at = contents.windows(name.len()).position(|w| w == name.as_slice()).expect("the property");
    contents[at] ^= 0x20; // no longer that property's name
    let (doc, report) = crate::doc::import(&write(&[("Contents", contents), ("P 1 1", page_with(|_| {})), ("Symbol 9", one_frame_stream(&[]))])).expect("opens");
    assert_eq!(doc.stage, anicraft_doc::Stage::default());
    assert_eq!(skipped(&report, "the stage's size, colour and frame rate (not found: the defaults are used)"), 1, "{:?}", report.skipped);
}
