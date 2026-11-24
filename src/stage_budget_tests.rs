//! The stage keeps memory and frame time flat (P-004, ADR 0132; #314, #477), driven frame by
//! frame through the real app: a zoom gesture draws nothing until it rests, and editing or
//! zooming for a long time holds no more pictures than the view on screen needs.
//! `stage_memory_and_frame_times` prints the numbers.

use crate::app::App;
use egui::{Event, Pos2, Rect, vec2};
use serde_json::json;

fn frame(ctx: &egui::Context, app: &mut App, time: f64, events: Vec<Event>) -> f64 {
    let input = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1600.0, 940.0))), time: Some(time), events, ..Default::default() };
    let t = std::time::Instant::now();
    ctx.run_ui(input, |ui| app.frame(ui)).textures_delta.clear();
    t.elapsed().as_secs_f64() * 1000.0
}

/// The app showing the sample, the pointer over the stage, the view drawn.
fn sample_app(ctx: &egui::Context) -> (App, f64) {
    let session = anicraft_engine::Session::new(anicraft_engine::doc::sample::demo());
    let mut app = App::with_context(ctx, session, Some(std::path::PathBuf::from("unused.png")));
    let mut t = 0.0;
    for _ in 0..3 {
        t += 0.016;
        frame(ctx, &mut app, t, vec![]);
    }
    let at = app.stage_cache.canvas.center();
    frame(ctx, &mut app, t, vec![Event::PointerMoved(at)]);
    assert!(drawn(ctx, &mut app, &mut t), "the first view was drawn");
    (app, t)
}

/// Frames until every tile of the view is on screen and the render thread has nothing left to
/// draw (the look-ahead ring around the view too); whether that happened.
fn drawn(ctx: &egui::Context, app: &mut App, t: &mut f64) -> bool {
    for _ in 0..30_000 {
        *t += 0.016;
        frame(ctx, app, *t, vec![]);
        let (ready, needed) = app.stage_cache.ready;
        if needed > 0 && ready == needed && app.stage_cache.service.as_ref().is_some_and(|s| !s.busy()) {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    false
}

/// `(pictures, bytes, pictures asked for and not found)` of the render service's cache.
fn cache(app: &App) -> (usize, usize, u64) {
    let (n, bytes, _, misses) = app.stage_cache.service.as_ref().map(|s| s.cache_stats()).unwrap_or_default();
    (n, bytes, misses)
}

/// #477: every frame of a Ctrl+wheel or pinch zoom is a zoom level of its own. Each one's
/// tiles used to be asked for, drawn (in the browser, inside the frame) and kept, though the
/// next frame no longer wanted them. Now nothing is asked for until the zoom rests; then the
/// view is drawn once and the stand-in tiles of the old zoom go.
#[test]
fn a_zoom_gesture_draws_nothing_until_it_rests() {
    let ctx = egui::Context::default();
    let (mut app, mut t) = sample_app(&ctx);
    let (zoom, before) = (app.view.zoom, cache(&app));
    for _ in 0..40 {
        t += 0.016;
        frame(&ctx, &mut app, t, vec![Event::Zoom(1.05)]);
        assert!(app.stage_cache.tile_count() > 0, "the pictures of the old zoom stand in");
    }
    assert!(app.view.zoom > zoom * 4.0, "the view zoomed in: {} to {}", zoom, app.view.zoom);
    assert_eq!(cache(&app), before, "nothing was asked for or drawn while the zoom was changing");
    // At rest: drawn, and only this view's tiles are held.
    assert!(drawn(&ctx, &mut app, &mut t));
    let needed = app.stage_cache.ready.1;
    assert!(cache(&app).2 > before.2, "the view at the new zoom was asked for");
    assert_eq!(app.stage_cache.tile_count(), needed, "the old zoom's textures are gone");
}

/// #314: a long drag is hundreds of document states. The textures and cached pictures after
/// many moves are what they were after a few: the view on screen and the state before it.
#[test]
fn editing_for_long_holds_no_more_pictures_than_editing_briefly() {
    let ctx = egui::Context::default();
    let (mut app, mut t) = sample_app(&ctx);
    app.run("edit.select_all", json!({}));
    let mut held = Vec::new();
    for step in 0..40 {
        assert!(app.run("element.translate", json!({"dx": 3, "dy": if step % 2 == 0 { 2 } else { -2 }})).is_some());
        assert!(drawn(&ctx, &mut app, &mut t), "move {step} was drawn");
        let needed = app.stage_cache.ready.1;
        assert_eq!(app.stage_cache.tile_count(), needed, "move {step}: one view of textures");
        held.push(cache(&app).1);
    }
    // The view and the ring around it, for this state and the one before: reached after two
    // moves, and never more however many follow.
    let (early, late) = (held.iter().take(5).max(), held.iter().skip(5).max());
    assert!(late <= early, "the cache held {early:?} bytes within 5 moves and {late:?} after 40");
}

/// This process's memory in use (MB), where the system says (Windows, Linux).
fn resident_mb() -> Option<f64> {
    let pid = std::process::id();
    if cfg!(windows) {
        let out = std::process::Command::new("tasklist").args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"]).output().ok()?;
        let text = String::from_utf8_lossy(&out.stdout).to_string();
        let kb: String = text.trim().rsplit('"').nth(1)?.chars().filter(char::is_ascii_digit).collect();
        Some(kb.parse::<f64>().ok()? / 1024.0)
    } else {
        let text = std::fs::read_to_string("/proc/self/statm").ok()?;
        Some(text.split_whitespace().nth(1)?.parse::<f64>().ok()? * 4096.0 / 1048576.0)
    }
}

/// Memory and frame times over a long session, on the CPU stage:
/// `cargo test --release --bin anicraft stage_memory_and_frame_times -- --ignored --nocapture`.
#[test]
#[ignore = "a measurement, not a check"]
fn stage_memory_and_frame_times() {
    let ctx = egui::Context::default();
    let (mut app, mut t) = sample_app(&ctx);
    let report = |what: &str, app: &App, ms: f64, worst: f64| {
        let (n, bytes, _) = cache(app);
        println!("{what:<34} {ms:>6.2} ms a frame (worst {worst:>6.2})  cache {n:>4} pictures {:>7.1} MB  textures {:>3}  process {:>7.1} MB", bytes as f64 / 1048576.0, app.stage_cache.tile_count(), resident_mb().unwrap_or(0.0));
    };
    report("at rest", &app, 0.0, 0.0);
    // A drag of 300 pointer moves.
    app.run("edit.select_all", json!({}));
    let (mut sum, mut worst) = (0.0f64, 0.0f64);
    for step in 0..300 {
        app.run("element.translate", json!({"dx": if step % 2 == 0 { 2 } else { -2 }, "dy": 0}));
        t += 0.016;
        let ms = frame(&ctx, &mut app, t, vec![]);
        sum += ms;
        worst = worst.max(ms);
    }
    report("a drag of 300 moves", &app, sum / 300.0, worst);
    // Playing the movie five times over.
    app.run("edit.deselect", json!({}));
    let frames = app.s.frame_count().max(1);
    let (mut sum, mut worst, mut n) = (0.0f64, 0.0f64, 0u32);
    for lap in 0..5 {
        for f in 0..frames {
            app.s.set_frame(f);
            t += 0.016;
            let ms = frame(&ctx, &mut app, t, vec![]);
            if lap == 0 {
                drawn(&ctx, &mut app, &mut t);
            } else {
                sum += ms;
                worst = worst.max(ms);
                n += 1;
            }
        }
        report(&format!("played {} frames, lap {}", frames, lap + 1), &app, sum / f64::from(n.max(1)), worst);
    }
    // Zooming in to the limit, a notch a frame, resting every 12 frames.
    while app.view.zoom < 63.0 {
        let (mut sum, mut worst) = (0.0f64, 0.0f64);
        for _ in 0..12 {
            t += 0.016;
            let ms = frame(&ctx, &mut app, t, vec![Event::Zoom(1.06)]);
            sum += ms;
            worst = worst.max(ms);
        }
        drawn(&ctx, &mut app, &mut t);
        report(&format!("zoomed to {:.0} %", app.view.zoom * 100.0), &app, sum / 12.0, worst);
    }
}
