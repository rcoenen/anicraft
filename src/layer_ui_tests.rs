//! The timeline's layer column, driven by pointer events: a layer dragged by its name to another
//! place (#407), the eye, padlock and square above the layers (#422), the line between the
//! layers and the frames (#384); and the Properties panel's W and H (#294).

use crate::app::App;
use crate::timeline::{DropRow, LayerDrop, layer_drop};
use anicraft_engine::doc::LayerId;
use egui::{Event, Modifiers, PointerButton, Pos2, Rect, pos2, vec2};

fn opened() -> (App, egui::Context) {
    let ctx = egui::Context::default();
    let session = anicraft_engine::Session::new(anicraft_engine::doc::sample::demo());
    let mut app = App::with_context(&ctx, session, Some(std::path::PathBuf::from("unused.png")));
    settle(&ctx, &mut app);
    (app, ctx)
}

fn frame(ctx: &egui::Context, app: &mut App, events: Vec<Event>) {
    let mut all = vec![Event::ModifiersChanged(Modifiers::NONE)];
    all.extend(events);
    let input = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1600.0, 940.0))), events: all, ..Default::default() };
    ctx.run_ui(input, |ui| app.frame(ui)).textures_delta.clear();
}

fn settle(ctx: &egui::Context, app: &mut App) {
    for _ in 0..3 {
        frame(ctx, app, Vec::new());
    }
}

fn button(pos: Pos2, pressed: bool) -> Event {
    Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }
}

/// Presses at `from` and moves to `to` in steps, the button still down.
fn press_and_move(ctx: &egui::Context, app: &mut App, from: Pos2, to: Pos2) {
    frame(ctx, app, vec![Event::PointerMoved(from)]);
    frame(ctx, app, vec![button(from, true)]);
    for i in 1..=10 {
        frame(ctx, app, vec![Event::PointerMoved(from + (to - from) * (i as f32 / 10.0))]);
    }
}

fn drag(ctx: &egui::Context, app: &mut App, from: Pos2, to: Pos2) {
    press_and_move(ctx, app, from, to);
    frame(ctx, app, vec![button(to, false)]);
    settle(ctx, app);
}

fn click(ctx: &egui::Context, app: &mut App, at: Pos2) {
    frame(ctx, app, vec![Event::PointerMoved(at)]);
    frame(ctx, app, vec![button(at, true)]);
    frame(ctx, app, vec![button(at, false)]);
    settle(ctx, app);
}

/// The layer names top down.
fn names(app: &App) -> Vec<String> {
    app.s.timeline().layers.iter().map(|l| l.name.clone()).collect()
}

/// The row of the layer called `name`, and where the frames start.
fn row(app: &App, name: &str) -> (Rect, f32) {
    let geo = app.drop.timeline.as_ref().expect("the timeline is drawn");
    let id = app.s.timeline().layers.iter().find(|l| l.name == name).map(|l| l.id).expect("the layer");
    let rect = geo.rows.iter().find(|(_, l)| *l == id).map(|(r, _)| *r).expect("its row shows");
    (rect, geo.cells_left)
}

/// A point on the layer's name, `t` of the way down its row.
fn on_name(app: &App, name: &str, t: f32) -> Pos2 {
    let (r, _) = row(app, name);
    pos2(r.left() + 70.0, r.top() + r.height() * t)
}

#[test]
fn a_drop_lands_above_below_or_inside() {
    let l = LayerId;
    let plain = |n: u64, top: f32| DropRow { top, bottom: top + 28.0, id: l(n), folder: false, first_child: None, dragged: false };
    // A, an open folder F holding B, then C; D is being dragged (not among the rows hovered).
    let rows = [plain(1, 0.0), DropRow { folder: true, first_child: Some(l(3)), ..plain(2, 28.0) }, plain(3, 56.0), plain(4, 84.0)];
    assert_eq!(layer_drop(&rows, -5.0), Some(LayerDrop::Before(l(1))), "above the list: the top");
    assert_eq!(layer_drop(&rows, 5.0), Some(LayerDrop::Before(l(1))));
    assert_eq!(layer_drop(&rows, 20.0), Some(LayerDrop::After(l(1))));
    assert_eq!(layer_drop(&rows, 30.0), Some(LayerDrop::Before(l(2))), "a folder's top quarter: above it");
    assert_eq!(layer_drop(&rows, 42.0), Some(LayerDrop::Into(l(2))), "its middle: inside");
    assert_eq!(layer_drop(&rows, 54.0), Some(LayerDrop::Into(l(2))), "under an open folder: inside, first");
    assert_eq!(layer_drop(&rows, 80.0), Some(LayerDrop::After(l(3))), "under the folder's last layer: still inside");
    assert_eq!(layer_drop(&rows, 86.0), Some(LayerDrop::Before(l(4))), "above the layer after the folder: out of it");
    assert_eq!(layer_drop(&rows, 500.0), Some(LayerDrop::After(l(4))), "below the list: the bottom");
    // A closed or empty folder: its bottom quarter is below it; a mask's lower half is above
    // its first masked layer.
    let closed = [DropRow { folder: true, ..plain(2, 0.0) }];
    assert_eq!(layer_drop(&closed, 26.0), Some(LayerDrop::After(l(2))));
    let mask = [DropRow { first_child: Some(l(3)), ..plain(2, 0.0) }, plain(3, 28.0)];
    assert_eq!(layer_drop(&mask, 20.0), Some(LayerDrop::Before(l(3))));
    // Over the dragged layer's own rows, or with no rows: nowhere.
    let own = [DropRow { dragged: true, ..plain(1, 0.0) }];
    assert_eq!(layer_drop(&own, 10.0), None);
    assert_eq!(layer_drop(&[], 10.0), None);
    // The parameters `layer.move` takes.
    assert_eq!(LayerDrop::Into(l(2)).params(l(9)), serde_json::json!({"layer": 9, "into": 2}));
    assert_eq!(LayerDrop::After(l(2)).params(l(9)), serde_json::json!({"layer": 9, "after": 2}));
}

#[test]
fn a_layer_dragged_by_its_name_moves_in_one_undo_step() {
    let (mut app, ctx) = opened();
    assert_eq!(names(&app), ["Ball", "Star", "Sun", "Hills", "Sky"]);
    let undo_before = app.s.history_len();
    // Ball onto the lower half of Hills: just below it.
    let (from, to) = (on_name(&app, "Ball", 0.5), on_name(&app, "Hills", 0.8));
    drag(&ctx, &mut app, from, to);
    assert_eq!(names(&app), ["Star", "Sun", "Hills", "Ball", "Sky"]);
    assert_eq!(app.s.history_len(), undo_before + 1, "one step");
    assert!(app.tl_layer_drag.is_none());
    assert_eq!(app.s.active_layer().map(|l| l.name.clone()).as_deref(), Some("Ball"), "the dragged layer is the active one");
    // Sky onto the upper half of Star: the top.
    let (from, to) = (on_name(&app, "Sky", 0.5), on_name(&app, "Star", 0.2));
    drag(&ctx, &mut app, from, to);
    assert_eq!(names(&app), ["Sky", "Star", "Sun", "Hills", "Ball"]);
    app.run("edit.undo", serde_json::json!({}));
    assert_eq!(names(&app), ["Star", "Sun", "Hills", "Ball", "Sky"]);

    // Esc during the drag, or a release over the frames, leaves the stack alone.
    let before = names(&app);
    let (from, to) = (on_name(&app, "Star", 0.5), on_name(&app, "Ball", 0.8));
    press_and_move(&ctx, &mut app, from, to);
    assert!(app.tl_layer_drag.is_some(), "dragging");
    frame(&ctx, &mut app, vec![Event::Key { key: egui::Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }]);
    assert!(app.tl_layer_drag.is_none());
    frame(&ctx, &mut app, vec![button(to, false)]);
    settle(&ctx, &mut app);
    assert_eq!(names(&app), before);
    let (r, cells) = row(&app, "Ball");
    let from = on_name(&app, "Star", 0.5);
    drag(&ctx, &mut app, from, pos2(cells + 200.0, r.center().y));
    assert_eq!(names(&app), before);

    // A click on the name still only picks the layer.
    let at = on_name(&app, "Sun", 0.5);
    click(&ctx, &mut app, at);
    assert_eq!(names(&app), before);
    assert_eq!(app.s.active_layer().map(|l| l.name.clone()).as_deref(), Some("Sun"));
}

#[test]
fn a_layer_dropped_on_a_folder_goes_inside() {
    let (mut app, ctx) = opened();
    app.run("layer.select", serde_json::json!({"index": 0}));
    app.run("layer.new_folder", serde_json::json!({"name": "Cast"}));
    settle(&ctx, &mut app);
    assert_eq!(names(&app).first().map(String::as_str), Some("Cast"));
    let (from, to) = (on_name(&app, "Sun", 0.5), on_name(&app, "Cast", 0.5));
    drag(&ctx, &mut app, from, to);
    let tl = app.s.timeline();
    let (folder, sun) = (tl.layers.iter().find(|l| l.name == "Cast").map(|l| l.id), tl.layers.iter().find(|l| l.name == "Sun").and_then(|l| l.parent));
    assert_eq!(sun, folder, "Sun is in the folder");
    assert_eq!(names(&app).get(1).map(String::as_str), Some("Sun"));
}

#[test]
fn the_buttons_above_the_layers_act_on_every_layer() {
    let (mut app, ctx) = opened();
    let (first, cells) = row(&app, "Ball");
    // The header is the ruler's band above the first row; the buttons head their columns.
    let head = |k: f32| pos2(cells - 16.0 - k * 26.0, first.top() - 12.0);
    let all = |app: &App, f: fn(&anicraft_engine::doc::Layer) -> bool| app.s.timeline().layers.iter().all(|l| f(l));
    click(&ctx, &mut app, head(2.0));
    assert!(all(&app, |l| !l.visible), "the eye hides every layer");
    click(&ctx, &mut app, head(2.0));
    assert!(all(&app, |l| l.visible), "and shows them again");
    click(&ctx, &mut app, head(1.0));
    assert!(all(&app, |l| l.locked), "the padlock locks every layer (Sky was locked already)");
    click(&ctx, &mut app, head(1.0));
    assert!(all(&app, |l| !l.locked));
    click(&ctx, &mut app, head(0.0));
    assert!(all(&app, |l| l.outline), "the square shows every layer as outlines");
    click(&ctx, &mut app, head(0.0));
    assert!(all(&app, |l| !l.outline));
}

#[test]
fn the_line_between_layers_and_frames_drags_and_resets() {
    let (mut app, ctx) = opened();
    let (r, cells) = row(&app, "Star");
    let w = app.prefs.timeline_layer_w;
    assert_eq!(w, crate::prefs::LAYER_W_DEFAULT);
    // Narrower: the frames start further left, and the width is kept in the preferences.
    drag(&ctx, &mut app, pos2(cells, r.center().y), pos2(cells - 60.0, r.center().y));
    assert!((app.prefs.timeline_layer_w - (w - 60.0)).abs() < 1.5, "{}", app.prefs.timeline_layer_w);
    let (_, now) = row(&app, "Star");
    assert!((now - (cells - 60.0)).abs() < 1.5, "the frames moved with it: {now}");
    assert!(app.tl_layer_w_drag.is_none());
    // Never narrower than the toggles need, nor wider than the limit.
    drag(&ctx, &mut app, pos2(now, r.center().y), pos2(now - 900.0, r.center().y));
    assert_eq!(app.prefs.timeline_layer_w, *crate::prefs::LAYER_W_RANGE.start());
    // A double-click puts it back.
    let (_, now) = row(&app, "Star");
    let at = pos2(now, r.center().y);
    frame(&ctx, &mut app, vec![Event::PointerMoved(at)]);
    for _ in 0..2 {
        frame(&ctx, &mut app, vec![button(at, true)]);
        frame(&ctx, &mut app, vec![button(at, false)]);
    }
    settle(&ctx, &mut app);
    assert_eq!(app.prefs.timeline_layer_w, crate::prefs::LAYER_W_DEFAULT);
    // A hand-edited or damaged preference comes back in range.
    let p = crate::prefs::Prefs::from_json(r#"{"timeline_layer_w": 99999}"#);
    assert_eq!(p.timeline_layer_w, *crate::prefs::LAYER_W_RANGE.end());
    assert_eq!(crate::prefs::Prefs::from_json("{}").timeline_layer_w, crate::prefs::LAYER_W_DEFAULT, "older preference files");
}

#[test]
fn width_and_height_stretch_the_selection_from_its_top_left_corner() {
    use crate::panels::resize_affine;
    // Twice as wide, half as high: the corner (10, 20) stays.
    let m = resize_affine([10.0, 20.0, 110.0, 60.0], 200.0, 20.0).expect("a change");
    assert_eq!(m, [2.0, 0.0, 0.0, 0.5, -10.0, 10.0]);
    assert_eq!((m[0] * 10.0 + m[4], m[3] * 20.0 + m[5]), (10.0, 20.0));
    assert_eq!((m[0] * 110.0 + m[4], m[3] * 60.0 + m[5]), (210.0, 40.0));
    // The same size, a size that isn't a positive number, or an empty box: nothing to do.
    assert_eq!(resize_affine([10.0, 20.0, 110.0, 60.0], 100.0, 40.0), None);
    for (w, h) in [(0.0, 40.0), (-5.0, 40.0), (f32::NAN, 40.0), (100.0, f32::INFINITY)] {
        assert_eq!(resize_affine([10.0, 20.0, 110.0, 60.0], w, h), None);
    }
    assert_eq!(resize_affine([f32::INFINITY, 0.0, f32::NEG_INFINITY, 0.0], 10.0, 10.0), None);
    // A level line has no height to stretch: only its width changes.
    assert_eq!(resize_affine([0.0, 5.0, 50.0, 5.0], 100.0, 30.0), Some([2.0, 0.0, 0.0, 1.0, 0.0, 0.0]));

    // Through the command the panel sends: a 100 × 40 rectangle becomes 200 × 20 in place.
    let (mut app, _ctx) = opened();
    app.run("layer.select", serde_json::json!({"index": 0}));
    app.run("draw.rect", serde_json::json!({"x": 10, "y": 20, "w": 100, "h": 40, "fill": "#336699", "stroke": null, "object": true}));
    let b = app.s.selection_bounds();
    let m = resize_affine([b.min.x, b.min.y, b.max.x, b.max.y], 200.0, 20.0).expect("a change");
    app.run("modify.transform", serde_json::json!({"a": m[0], "b": m[1], "c": m[2], "d": m[3], "e": m[4], "f": m[5]}));
    let b = app.s.selection_bounds();
    for (got, want) in [(b.min.x, 10.0), (b.min.y, 20.0), (b.max.x, 210.0), (b.max.y, 40.0)] {
        assert!((got - want).abs() < 0.01, "{got} vs {want}");
    }
}

#[test]
fn brush_settings_come_back_with_the_next_start() {
    let (mut app, _ctx) = opened();
    // Untouched tools write nothing, so they keep following the defaults.
    assert!(!crate::brushtool::sync(&mut app, false));
    assert_eq!((app.prefs.brush_tool.as_ref(), app.prefs.paint_brush_tool.as_ref()), (None, None));
    // A changed size and smoothing are kept once the slider is let go, each tool its own.
    app.brush.size = 37.0;
    app.brush.smoothing = 80.0;
    app.paint_brush.size = 3.0;
    assert!(!crate::brushtool::sync(&mut app, true), "not while the pointer is down");
    assert!(crate::brushtool::sync(&mut app, false));
    assert!(!crate::brushtool::sync(&mut app, false), "written once");
    // What the preferences file holds brings them back in a fresh window.
    let json = serde_json::to_string(&app.prefs).expect("preferences serialise");
    let (mut next, _ctx) = opened();
    assert_ne!(next.brush.size, 37.0);
    next.prefs = crate::prefs::Prefs::from_json(&json);
    crate::brushtool::restore(&mut next);
    assert_eq!((next.brush.size, next.brush.smoothing, next.paint_brush.size), (37.0, 80.0, 3.0));
    assert!(!crate::brushtool::sync(&mut next, false), "restored settings are the kept ones");
    // Older files have none; junk is ignored and values out of range are clamped.
    let p = crate::prefs::Prefs::from_json(r#"{"brush_tool": 7, "paint_brush_tool": {"size": 999999}}"#);
    assert!(p.brush_tool.is_none());
    assert!(p.paint_brush_tool.is_some_and(|b| b.size <= 2000.0));
    assert!(crate::prefs::Prefs::from_json("{}").brush_tool.is_none());
}

/// A click with modifier keys held.
fn click_with(ctx: &egui::Context, app: &mut App, at: Pos2, modifiers: Modifiers) {
    let step = |app: &mut App, events: Vec<Event>| {
        let mut all = vec![Event::ModifiersChanged(modifiers)];
        all.extend(events);
        let input = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1600.0, 940.0))), events: all, ..Default::default() };
        ctx.run_ui(input, |ui| app.frame(ui)).textures_delta.clear();
    };
    step(app, vec![Event::PointerMoved(at)]);
    step(app, vec![Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: true, modifiers }]);
    step(app, vec![Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: false, modifiers }]);
    settle(ctx, app);
}

/// The layers selected in the layer list, by name, top down.
fn picked(app: &App) -> Vec<String> {
    app.s.picked_layers().iter().filter_map(|id| app.s.timeline().layer(*id)).map(|l| l.name.clone()).collect()
}

/// #512: Ctrl-click and Shift-click on layer names select several layers, a drag that starts on
/// one of them moves them all in one undo step, and a plain click is one layer again.
#[test]
fn several_layers_are_picked_with_ctrl_and_shift_and_dragged_together() {
    let (mut app, ctx) = opened();
    assert_eq!(names(&app), ["Ball", "Star", "Sun", "Hills", "Sky"]);
    let at = on_name(&app, "Star", 0.5);
    click(&ctx, &mut app, at);
    let at = on_name(&app, "Hills", 0.5);
    click_with(&ctx, &mut app, at, Modifiers::COMMAND);
    assert_eq!(picked(&app), ["Star", "Hills"]);
    assert_eq!(app.s.active_layer().map(|l| l.name.as_str()), Some("Star"), "the layer clicked first stays the active one");
    // Dragged by Hills, the one that is not active: both land under Sky, Star first.
    let undo_before = app.s.history_len();
    let (from, to) = (on_name(&app, "Hills", 0.5), on_name(&app, "Sky", 0.8));
    drag(&ctx, &mut app, from, to);
    assert_eq!(names(&app), ["Ball", "Sun", "Sky", "Star", "Hills"]);
    assert_eq!(app.s.history_len(), undo_before + 1, "one step");
    assert_eq!(picked(&app), ["Star", "Hills"], "still selected where they landed");
    // Ctrl-click again takes a layer out; Shift-click takes every layer from the active one.
    let at = on_name(&app, "Hills", 0.5);
    click_with(&ctx, &mut app, at, Modifiers::COMMAND);
    assert_eq!(picked(&app), ["Star"]);
    let at = on_name(&app, "Ball", 0.5);
    click_with(&ctx, &mut app, at, Modifiers::SHIFT);
    assert_eq!(picked(&app), ["Ball", "Sun", "Sky", "Star"]);
    // A drag that starts on a layer outside the selection moves that layer alone.
    let (from, to) = (on_name(&app, "Hills", 0.5), on_name(&app, "Ball", 0.2));
    drag(&ctx, &mut app, from, to);
    assert_eq!(names(&app), ["Hills", "Ball", "Sun", "Sky", "Star"]);
    assert_eq!(picked(&app), ["Hills"]);
    let at = on_name(&app, "Sun", 0.5);
    click(&ctx, &mut app, at);
    assert_eq!(picked(&app), ["Sun"], "a plain click is one layer again");
}

/// The sample with the accessibility tree on.
fn opened_with_tree() -> (App, egui::Context) {
    let (mut app, ctx) = opened();
    ctx.enable_accesskit();
    settle(&ctx, &mut app);
    (app, ctx)
}

/// One more frame (with `events`, in a window `height` tall): every named node, or label with
/// its text, and where it is, in points.
fn nodes_after(ctx: &egui::Context, app: &mut App, height: f32, events: Vec<Event>) -> Vec<(String, Rect)> {
    let input = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1600.0, height))), events, ..Default::default() };
    let mut out = ctx.run_ui(input, |ui| app.frame(ui));
    out.textures_delta.clear();
    let ppp = ctx.pixels_per_point();
    let nodes = out.platform_output.accesskit_update.map(|u| u.nodes).unwrap_or_default();
    nodes.into_iter().filter_map(|(_, n)| Some((n.label().or(n.value())?.to_string(), n.bounds().map(|b| Rect::from_min_max(pos2(b.x0 as f32, b.y0 as f32) / ppp, pos2(b.x1 as f32, b.y1 as f32) / ppp))?))).collect()
}

fn named_nodes(ctx: &egui::Context, app: &mut App) -> Vec<(String, Rect)> {
    nodes_after(ctx, app, 940.0, Vec::new())
}

/// #523: the "1–9 of 17 layers" count in the layer list's header never lies over the buttons
/// there: the parenting toggle beside the heading and the eye, padlock and square that head
/// their columns. In a column too narrow for it the heading carries it instead.
#[test]
fn the_layer_count_clears_the_header_buttons() {
    let (mut app, ctx) = opened_with_tree();
    for _ in 0..12 {
        app.run("layer.new", serde_json::json!({}));
    }
    for width in [crate::prefs::LAYER_W_DEFAULT, *crate::prefs::LAYER_W_RANGE.start(), *crate::prefs::LAYER_W_RANGE.end()] {
        app.prefs.timeline_layer_w = width;
        settle(&ctx, &mut app);
        let nodes = named_nodes(&ctx, &mut app);
        let find = |name: &str| nodes.iter().find(|(l, _)| l == name).map(|(_, r)| *r).unwrap_or_else(|| panic!("no {name}"));
        let (count, at) = nodes.iter().find(|(l, _)| l.ends_with("of 17 layers")).cloned().unwrap_or_else(|| panic!("more layers than rows, and no count at width {width}"));
        for button in ["Parenting view", "Hide all layers", "Lock all layers", "Show all layers as outlines"] {
            assert!(!at.intersects(find(button)), "{count:?} at {at:?} is over {button} at {:?} (layer column {width} wide)", find(button));
        }
    }
}

/// #513: the Movie Explorer's list uses the height its pane has. With the dock to itself and
/// more rows than fit, the last row shown is at the bottom of the pane, not half-way down.
#[test]
fn the_movie_explorer_list_reaches_the_bottom_of_its_pane() {
    use crate::dock::Panel;
    let (mut app, ctx) = opened_with_tree();
    for _ in 0..60 {
        app.run("layer.new", serde_json::json!({}));
    }
    crate::movie_explorer::open(&mut app);
    for p in [Panel::Properties, Panel::Align, Panel::Library, Panel::History, Panel::Swatches, Panel::Colour] {
        app.dock.close(p);
    }
    app.dock_changed();
    settle(&ctx, &mut app);
    let pane = app.dock_view.drawn.iter().find(|d| d.first == Panel::MovieExplorer).map(|d| d.rect).expect("the Movie Explorer is docked");
    assert!(pane.height() > 600.0, "the pane has the dock to itself: {pane:?}");
    let rows: Vec<Rect> = named_nodes(&ctx, &mut app).into_iter().filter(|(l, r)| l.starts_with("Layer: ") && pane.contains(r.center())).map(|(_, r)| r).collect();
    let row_h = rows.first().map_or(0.0, |r| r.height());
    let last = rows.iter().map(|r| r.bottom()).fold(0.0, f32::max);
    assert!(rows.len() < 60, "more rows than fit, so the list scrolls: {} shown", rows.len());
    assert!(pane.bottom() - last < 2.0 * row_h, "the list ends at {last}, the pane at {} ({} rows of {row_h})", pane.bottom(), rows.len());
}

/// #246: a click on the fill square or the stroke square under the tools opens its colour
/// picker (the stroke's square lies over a corner of the fill's, and each takes its own clicks).
#[test]
fn the_fill_and_stroke_squares_open_their_colour_pickers() {
    for (square, corner, picker) in [("Fill colour", 1.0, "Fill colour picker"), ("Stroke colour", 0.0, "Colour picker")] {
        let (mut app, ctx) = opened_with_tree();
        // A window tall enough to show the squares under the last tool.
        let tall = 1400.0;
        let mut nodes = Vec::new();
        for _ in 0..3 {
            nodes = nodes_after(&ctx, &mut app, tall, Vec::new());
        }
        assert!(!nodes.iter().any(|(l, _)| l == picker), "no picker is open yet");
        let r = nodes.iter().find(|(l, _)| l == square).map(|(_, r)| *r).unwrap_or_else(|| panic!("no {square} square"));
        assert!(r.bottom() < tall, "the squares are on screen: {r:?}");
        // The corner of the square that the other one does not cover.
        let at = pos2(r.left() + 4.0 + (r.width() - 8.0) * corner, r.top() + 4.0 + (r.height() - 8.0) * corner);
        nodes_after(&ctx, &mut app, tall, vec![Event::PointerMoved(at)]);
        nodes_after(&ctx, &mut app, tall, vec![button(at, true)]);
        nodes_after(&ctx, &mut app, tall, vec![button(at, false)]);
        for _ in 0..3 {
            nodes = nodes_after(&ctx, &mut app, tall, Vec::new());
        }
        assert!(nodes.iter().any(|(l, _)| l == picker), "a click on {square} opens its picker");
    }
}
