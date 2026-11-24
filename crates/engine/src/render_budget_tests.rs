//! Memory and frame time stay flat (P-004, ADR 0132; #314, #477): what the picture cache holds
//! doesn't grow with how long a document is edited or played, what the screen needs is drawn
//! before look-ahead, and with no render thread a view fills in a bounded step at a time.
//! These check relations, not timings; `stage_timings` prints the timings.

use crate::Session;
use crate::render_service::{FrameJob, KEEP_SETS, Raster, Rasterizer, RenderService, lookahead_batch, make_job, render_batch, render_job};
use anicraft_doc::{DisplayList, Document, sample};
use anicraft_geom::Affine;
use anicraft_render::{RenderStats, View};
use serde_json::json;
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::Duration;

/// The stage's tile edge.
const TILE: u32 = 512;
/// Most tiles a 1600 × 900 view can touch, wherever the tile grid falls: 5 across, 3 down.
const VIEW_TILES: usize = 15;

/// The tiles a `vw × vh` px view centred on the stage's centre needs at device scale `z`, cut
/// as the stage cuts them.
fn view_tiles(doc: &Document, state: u64, frame: u32, z: f32, (vw, vh): (f32, f32), tile: u32) -> Vec<FrameJob> {
    let (sw, sh) = ((doc.stage.width as f32 * z).ceil() as i64, (doc.stage.height as f32 * z).ceil() as i64);
    let (cx, cy) = (doc.stage.width as f32 * z / 2.0, doc.stage.height as f32 * z / 2.0);
    let t = i64::from(tile);
    let lo = |c: f32, half: f32| ((c - half).floor() as i64).max(0) / t;
    let hi = |c: f32, half: f32, len: i64| (((c + half).ceil() as i64).min(len) - 1) / t;
    let mut out = Vec::new();
    for ty in lo(cy, vh / 2.0)..=hi(cy, vh / 2.0, sh) {
        for tx in lo(cx, vw / 2.0)..=hi(cx, vw / 2.0, sw) {
            let view = View { transform: Affine::translate(-((tx * t) as f32), -((ty * t) as f32)).mul(&Affine::scale(z, z)), width: t.min(sw - tx * t) as u32, height: t.min(sh - ty * t) as u32, background: Some(doc.stage.background) };
            out.push(make_job(state, doc, frame, view, None));
        }
    }
    out
}

/// A rasterizer that hands back blank pictures of the right size at once: what the cache keeps
/// doesn't depend on who drew it.
fn blank() -> Rasterizer {
    Arc::new(|_: &DisplayList, v: &View| Ok((Raster::Premul8(vec![0; v.width as usize * v.height as usize * 4]), RenderStats::default())))
}

/// The stage's frame: say which set is shown, then draw what is missing.
fn show(svc: &RenderService, jobs: Vec<FrameJob>) -> usize {
    if let Some(j) = jobs.first() {
        svc.show(j.set);
    }
    let raster = svc.raster_id();
    // Keyed for the rasterizer in use, as `RenderService::job` keys them.
    let jobs: Vec<FrameJob> = jobs.into_iter().map(|j| FrameJob { raster, ..j }).collect();
    svc.render_now_batch(jobs).len()
}

/// #314: a drag makes a new document state on every pointer move, and each state's pictures
/// used to stay until the whole budget was full (768 MB; "900 MB for a 24-frame move"). Now
/// the cache holds the state on screen and the one before it, however long the drag.
#[test]
fn a_long_drag_holds_no_more_pictures_than_a_short_one() {
    let doc = sample::demo();
    let svc = RenderService::new(1 << 30, None);
    svc.set_rasterizer(Some(blank()));
    let view = (1600.0, 900.0);
    let mut held = Vec::new();
    for step in 0..120u64 {
        let tiles = show(&svc, view_tiles(&doc, 1000 + step, 0, 1.0, view, TILE));
        let (images, bytes, ..) = svc.cache_stats();
        assert!(images <= KEEP_SETS * tiles, "step {step}: {images} pictures for {tiles} tiles on screen");
        held.push(bytes);
    }
    assert_eq!(held.get(10), held.last(), "the cache after 120 moves holds what it held after 10");
    // Undo: the state before is still there, so nothing is drawn again.
    let back = view_tiles(&doc, 1000 + 118, 0, 1.0, view, TILE);
    svc.show(back[0].set);
    let raster = svc.raster_id();
    let (_, _, _, misses) = svc.cache_stats();
    assert!(svc.request_batch(back.into_iter().map(|j| FrameJob { raster, ..j }).collect()).iter().all(Option::is_some), "the state before the last is kept");
    assert_eq!(svc.cache_stats().3, misses);
    // A zoom gesture is the same: every frame a new scale, two of them kept.
    let mut held = Vec::new();
    for step in 0..80 {
        let z = 1.0 * 1.05f32.powi(step);
        let tiles = show(&svc, view_tiles(&doc, 5000, 0, z, view, TILE));
        let (images, bytes, ..) = svc.cache_stats();
        assert!(tiles <= VIEW_TILES && images <= KEEP_SETS * VIEW_TILES, "zoom step {step}: {images} pictures, {tiles} tiles on screen");
        held.push(bytes);
    }
    let most = held.iter().copied().max().unwrap_or(0);
    assert!(most <= KEEP_SETS * VIEW_TILES * (TILE as usize * TILE as usize * 4 + 1024), "two views' worth of pictures at most: {most} bytes");
    // A picture that finishes after its set has gone (the worker was still drawing it) is not
    // kept either.
    let late = view_tiles(&doc, 1000, 0, 1.0, view, TILE);
    let keys: Vec<u64> = late.iter().map(|j| j.key).collect();
    svc.prefetch(late.into_iter().map(|j| FrameJob { raster, ..j }).collect());
    for _ in 0..60_000 {
        if !svc.busy() {
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(!svc.busy());
    assert!(keys.iter().all(|k| svc.wait_for(*k, 0).is_none()), "pictures of a set long gone came back");
}

/// Playing a movie fills the cache with its frames once; playing it again and again adds
/// nothing, whether the budget holds the whole movie or only part of it.
#[test]
fn playing_longer_does_not_grow_the_cache() {
    let doc = sample::demo();
    let frames = 24u32;
    let one_frame = 6 * (TILE as usize * TILE as usize * 4);
    for budget in [1usize << 30, 5 * one_frame] {
        let svc = RenderService::new(budget, None);
        svc.set_rasterizer(Some(blank()));
        let mut after = Vec::new();
        for _lap in 0..4 {
            for f in 0..frames {
                show(&svc, view_tiles(&doc, 7, f, 1.0, (1600.0, 900.0), TILE));
            }
            after.push(svc.cache_stats().1);
        }
        assert_eq!(after.first(), after.last(), "budget {budget}: four laps hold what one lap held");
        assert!(after.iter().all(|b| *b <= budget + one_frame), "budget {budget}: {after:?}");
        if budget > frames as usize * one_frame {
            let (_, _, hits, misses) = svc.cache_stats();
            assert!(hits >= 3 * misses, "the laps after the first are cache hits: {hits} hits, {misses} misses");
        }
    }
}

/// A visible tile used to be drawn in the same pass as up to 31 look-ahead tiles, and shown
/// when the last of them was done. Now what the screen asks for is drawn next and alone.
#[test]
fn what_the_screen_needs_is_drawn_before_look_ahead() {
    let doc = sample::demo();
    let svc = RenderService::new(64 << 20, None);
    // Records each picture as its drawing starts, and waits there until the test says go.
    let order: Arc<Mutex<Vec<u32>>> = Arc::default();
    let go: Arc<(Mutex<bool>, Condvar)> = Arc::default();
    let (o, g) = (order.clone(), go.clone());
    let raster: Rasterizer = Arc::new(move |_: &DisplayList, v: &View| {
        o.lock().unwrap_or_else(PoisonError::into_inner).push(v.width);
        let mut open = g.0.lock().unwrap_or_else(PoisonError::into_inner);
        while !*open {
            open = g.1.wait(open).unwrap_or_else(PoisonError::into_inner);
        }
        Ok((Raster::Premul8(vec![0; v.width as usize * v.height as usize * 4]), RenderStats::default()))
    });
    svc.set_rasterizer(Some(raster));
    let view = |w: u32| View { transform: Affine::scale(0.1, 0.1), width: w, height: 4, background: None };
    // Look-ahead (5 px wide) for many frames; the worker starts on it and is held there.
    let pass = lookahead_batch();
    svc.prefetch((0..(4 * pass as u32 + 8)).map(|f| svc.job(1, &doc, f, view(5), None)).collect());
    let started = |n: usize| {
        for _ in 0..60_000 {
            if order.lock().unwrap_or_else(PoisonError::into_inner).len() >= n {
                return true;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        false
    };
    assert!(started(1), "the worker began the look-ahead");
    // Now the screen asks for three pictures (7 px wide).
    let urgent: Vec<FrameJob> = (100..103).map(|f| svc.job(1, &doc, f, view(7), None)).collect();
    let keys: Vec<u64> = urgent.iter().map(|j| j.key).collect();
    assert!(svc.request_batch(urgent).iter().all(Option::is_none));
    *go.0.lock().unwrap_or_else(PoisonError::into_inner) = true;
    go.1.notify_all();
    for k in keys {
        assert!(svc.wait_for(k, 60_000).is_some());
    }
    let order = order.lock().unwrap_or_else(PoisonError::into_inner).clone();
    let last = order.iter().rposition(|w| *w == 7).unwrap_or(usize::MAX);
    assert!(last < pass + 3, "the screen's pictures came after {last} others; one look-ahead pass is {pass} ({order:?})");
}

/// The browser has no render thread (ADR 0036): a view that needs many tiles used to draw them
/// all inside one frame. Within a budget it draws what fits (at least one) and the rest over
/// the next frames, nearest first, and the pictures are the oracle's.
#[test]
fn without_a_thread_a_view_fills_in_a_step_at_a_time() {
    let doc = sample::demo();
    let svc = RenderService::without_thread(64 << 20);
    assert!(!svc.has_thread());
    let jobs = view_tiles(&doc, 3, 9, 0.25, (320.0, 180.0), 64);
    assert!(jobs.len() >= 12, "{} tiles", jobs.len());
    let mut frames = 0;
    loop {
        frames += 1;
        let got = svc.request_batch_within(jobs.clone(), Duration::ZERO);
        let ready = got.iter().filter(|g| g.is_some()).count();
        assert_eq!(ready, frames, "one more picture each frame, and the earlier ones from the cache");
        assert!(got.iter().take(ready).all(Option::is_some), "in the order asked for");
        if ready == jobs.len() {
            for (j, img) in jobs.iter().zip(&got) {
                assert_eq!(img.as_ref().map(|i| &i.rgba_premul), Some(&render_job(j).rgba_premul));
            }
            break;
        }
    }
    // With all the time it needs, or asked the plain way (an edit, a frame of playback: one
    // whole picture), everything comes at once.
    let other = view_tiles(&doc, 4, 9, 0.25, (320.0, 180.0), 64);
    assert!(svc.request_batch_within(other, Duration::from_secs(3600)).iter().all(Option::is_some));
    assert!(svc.request_batch(view_tiles(&doc, 5, 9, 0.25, (320.0, 180.0), 64)).iter().all(Option::is_some));
}

/// A drawing made with the Brush tool: `n` wavy strokes across the stage, merged into the
/// drawing as the tool does off Object Drawing.
fn brush_drawing(n: u32) -> Session {
    let mut s = Session::new(Document::default());
    for k in 0..n {
        let pts: Vec<[f32; 2]> = (0..=120)
            .map(|i| {
                let t = i as f32 / 120.0;
                [60.0 + t * 1100.0, 80.0 + (k % 8) as f32 * 70.0 + (k / 8) as f32 * 9.0 + (t * 14.0 + k as f32).sin() * 40.0]
            })
            .collect();
        s.execute("draw.brush", &json!({"points": pts, "size": 12, "object": false})).expect("a brush stroke");
    }
    s
}

/// #477, through the stage's own path: zoomed all the way into a brush drawing, the tiles of a
/// view together walk fewer edges than the view of the whole drawing at 100 %.
#[test]
fn a_view_at_the_deepest_zoom_walks_fewer_edges_than_the_whole_drawing() {
    let s = brush_drawing(12);
    let edges = |z: f32| render_batch(&view_tiles(&s.doc, 1, 0, z, (800.0, 450.0), 256)).iter().map(|i| i.stats.edges).sum::<usize>();
    let (normal, deep) = (edges(1.0), edges(64.0));
    assert!(deep > 0 && deep < normal, "{deep} edges at 6400 %, {normal} at 100 %");
}

/// What a 1600 × 900 stage view costs at each zoom, on the CPU oracle, in the stage's tiles:
/// `cargo test --release -p anicraft-engine --lib stage_timings -- --ignored --nocapture`.
#[test]
#[ignore = "a measurement, not a check"]
fn stage_timings() {
    for (name, s) in [("8 brush strokes", brush_drawing(8)), ("64 brush strokes, merged", brush_drawing(64)), ("512 brush strokes, merged", brush_drawing(512))] {
        let doc = &s.doc;
        println!("-- {name}: {} items", s.display_list().items.len());
        println!("zoom  tiles  ms/view   edges  prepare ms  one thread ms");
        for z in [1.0f32, 4.0, 16.0, 64.0] {
            let jobs = view_tiles(doc, 1, 0, z, (1600.0, 900.0), TILE);
            let _ = render_batch(&jobs);
            let t = std::time::Instant::now();
            let mut edges = 0;
            for _ in 0..5 {
                edges = render_batch(&jobs).iter().map(|i| i.stats.edges).sum::<usize>();
            }
            let ms = t.elapsed().as_secs_f64() * 1000.0 / 5.0;
            let dl = jobs[0].display_list();
            let t = std::time::Instant::now();
            for j in &jobs {
                let _ = anicraft_render::prepare_scene(&dl, &j.view);
            }
            let prep = t.elapsed().as_secs_f64() * 1000.0;
            // As the browser draws: one thread, a tile after another.
            let pool = rayon::ThreadPoolBuilder::new().num_threads(1).build().expect("a pool");
            let t = std::time::Instant::now();
            pool.install(|| {
                for j in &jobs {
                    let _ = crate::render_service::render_job_with(j, &dl);
                }
            });
            let single = t.elapsed().as_secs_f64() * 1000.0;
            println!("{z:>4}  {:>5}  {ms:>7.2}  {edges:>6}  {prep:>10.2}  {single:>13.2}", jobs.len());
        }
    }
}
