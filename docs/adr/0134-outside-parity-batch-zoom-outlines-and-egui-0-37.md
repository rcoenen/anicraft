# ADR 0134: Decisions taken with the outside parity batch: zoom in device pixels, aligning by outlines, a marquee that touches, egui 0.37

* Status: accepted (2026-11-24), built
* Relates to: ADR 0125 (the roadmap as a guideline; backlog items P-001, P-017, P-029),
  ADR 0041 and ADR 0044 (the marquee), ADR 0112 (document tabs), ADR 0092 (GPU recovery),
  ADR 0040 (localisation), outside PRs #594 (rcoenen), #590 (ViktorBystrov72), #580, #574,
  #575 and #578 (AyberkA0)

## Context

Nineteen outside pull requests were reviewed and merged in one batch. Most are fixes that need
no record beyond the changelog. Four change a rule that other work will build on, so they are
written down here.

## Decisions

### 1. 100 % zoom is one stage pixel on one device pixel

`StageView::zoom` is still egui points per stage pixel, because everything that draws on the
stage works in points. What the artist reads and types (the zoom field, its presets, the badge,
Ctrl+1, the 5 % to 6400 % limits) is `zoom × pixels_per_point`. So 100 % shows a 1920 × 1080
stage on 1920 × 1080 screen pixels at any display scale and any Interface Scale (#565); before,
both scales multiplied into the picture. A view the artist chose keeps its percentage and its
centre when the scale changes (a window moved to another display); a fitted view fits again.
Fit in Window may go below the 5 % limit to show a very large stage whole, and Zoom Out from
there never jumps up to 5 %.

*Rejected:* 100 % as one stage pixel per point (what we had). It is what some design tools do,
but it made "100 %" depend on a preference about the size of buttons.

Handles and other marks stay sized in points, so they remain usable on dense displays.

### 2. Align, distribute, Match Size and Space Evenly measure outlines

They line up where a shape's outline is, without the width of its stroke (#569), as the
reference app does for shapes: a thicker stroke leaves aligned artwork where it is.
`Session::element_outline_bounds_on` is that box; `element_bounds_on` stays what an element
covers (selection boxes, hit areas, Free Transform). Raw pieces are still grouped into one
drawing by what they cover.

The same rule is applied to every kind of element, symbol instances included. Whether the
reference app measures an instance with the strokes inside it was not observed; if it does,
instances can be given their covered box in `selected_units` without touching the rest.

Inside a symbol (at any depth, also with a group or drawing object opened inside it), To Stage
aligns to the symbol's registration point (#568): a rectangle of no size at its origin.

### 3. The marquee takes the objects it touches

An instance, group, drawing object, text or bitmap whose box the marquee crosses is selected
whole (#522); raw shapes are still cut at its edges (ADR 0041). The test is the element's box,
not its outline. The reference app has a preference that turns this off; we have no such
preference yet, and it is always on.

### 4. egui and eframe 0.37

Closing the window aborted on macOS (`NSRangeException`, exit code 134). The cause is in
AccessKit's macOS adapter, which gave winit's view a subclass and put the old class back on
close while AppKit was observing the view; it is fixed upstream (AccessKit/accesskit#835,
accesskit_macos 0.27.2), which only egui 0.37 can use. We checked the diagnosis against our own
code: the tab close flow (ADR 0112) only decides *when* the window may close, and GPU recovery
(ADR 0092) rebuilds the surface, never the view's class; the author's measurement of a bare
eframe window (aborts with AccessKit on 0.36, none without it, none on 0.37) agrees.

So the fix is the version move, taken whole in this batch:

* `WidgetType` is `Role`, `Id::new` is `Id::unique`, `Ui::id` is `Ui::scope_id`; screenshots
  come back through `Context::request_screenshot`.
* egui 0.37 names its own resize handles and passes tooltips to screen readers. The names egui
  gives its own parts (those handles, and the colour mixer's sliders and value button) are
  listed as egui's in the pseudo-locale tests (`EGUI_OWN`); they are English until egui lets
  us name them or we draw a mixer of our own.
* Single-line text fields are as tall as the buttons beside them.
* A popup's response is no longer the node its controls hang under, so a name set on it
  reached nobody. The colour pickers name their dialog on the popup's own `Ui`
  (`colour::name_popup`), with the popup's place, which containers no longer carry.
* New crates in `Cargo.lock` are MIT or MIT/Apache-2.0; `cargo deny check licenses bans
  sources` passes.

*Not verified here:* a run on a Mac. The author measured 3 aborts in 3 `--screenshot` runs on
`main` and none in 6 on the branch; Cmd+Q and the close button go through the same drop and
are on the Mac checklist (P-015).

## Consequences

* Open branches that use the old egui names need the renames above.
* A document's view remembers the display scale it was measured at (`StageView::pixels_per_point`),
  per tab.
* Tests that look a control up by name must expect History rows of the same name: a step is
  named after its command.
