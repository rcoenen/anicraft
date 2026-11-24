# ADR 0132: Memory and frame time stay flat: picture sets, a zoom at rest, and edges nobody sees

* Status: accepted (2026-11-23)
* Relates to: backlog item P-004 (`docs/roadmap.md`), issues
  [#314](https://github.com/storytold/anicraft/issues/314) and
  [#477](https://github.com/storytold/anicraft/issues/477), ADR 0002 (the render thread),
  ADR 0006 (latency or throughput), ADR 0007 (stage tiles), ADR 0029 (pictures kept on the
  GPU), ADR 0036 (the web build), ADR 0112 (several documents), `docs/performance.md`

## Context

Two reports: "a single symbol that moves across the screen with 24 frames takes up 900 MB"
in the web version (#314), and "the app stutters and lags when I try to zoom in very closely"
after drawing with the Brush (#477). An earlier fix cut the web build's picture cache from
768 MB to 256 MB, which lowered the ceiling without changing why it was reached.

We measured before changing anything (`docs/performance.md` has the tables):

* **On many threads, drawing a stage view costs about the same at every zoom.** A 1600 × 900
  view takes 6 to 9 ms on the CPU oracle from 100 % to 6400 % on a 32-thread machine, also for
  a drawing of 64 merged brush strokes: there the cost is the pixels.
* **But the work per tile grew with the zoom and with the whole drawing.** A tile prepared
  every edge of every item that touched it: 42,700 edges for the view at 100 %, 281,300 at
  6400 %, all but a few off the tile. On one thread, as the browser draws, the view took
  9.0 ms at 100 % and 33.5 ms at 6400 %.
* **The cache filled with pictures nobody could ask for again.** A drag makes a new document
  state on every pointer move; each state's tiles were kept until the least recently used
  fell out of a 768 MB budget (384 MB more on the GPU, 192 tile textures in the shell). A
  zoom gesture did the same with zoom levels: 136 MB after 84 frames of Ctrl+wheel. That is
  #314: the memory followed how long you had been editing, not what was on screen.
* **Every frame of a zoom gesture asked for a whole view** at a zoom level the next frame no
  longer wanted. With a render thread the answers arrived too late to be shown; in the
  browser, which has no thread (ADR 0036), they were drawn inside the frame.
* **A visible tile waited for look-ahead.** The render thread took up to 32 jobs a pass, the
  urgent ones topped up with look-ahead, and put the pictures in the cache when the whole
  pass was done.

## Decision

### 1. Picture sets: the cache keeps what is on screen

Every picture belongs to a **picture set**: one document state at one scale
(`render_service::set_key`, in every `FrameJob` and `FrameImage`). The stage tells the render
service which set it shows (`RenderService::show`, each frame, cheap when nothing changed).
The cache keeps that set and the one shown before it (`KEEP_SETS = 2`) and drops the pictures
of every other set at once. A picture that finishes for a set that has gone is not kept, and
the render thread doesn't start one.

Two, not one, so that undo then redo, or 100 % and back, finds its pictures. Not more: a
third state back is one redraw away (a few milliseconds), and each kept set is a view's worth
of memory. The frames of a movie played at one zoom are one set, so playback caching is as it
was, under the same byte budgets. A cache nobody has told what is shown (tests, tools) keeps
everything, as before.

The shell does the same with its tile textures: when the view is whole, the textures of
other sets, which were only standing in for it, go.

**Considered:** lowering the budgets (it moves the ceiling, as the earlier fix did);
keying tiles by their content so a drag only redraws the tiles the dragged thing touches (the
right next step for the browser, where a drag still redraws the view on the interface thread;
a much larger change, listed under P-004 as not yet).

### 2. A zoom that is changing draws nothing

While this frame's device scale differs from the last frame's, the stage asks for no tiles
and no look-ahead and paints the pictures it has, scaled (ADR 0007 already draws stale tiles
under missing ones). The first frame at rest asks once. No timer: egui smooths a wheel notch
over several frames, and each of those has a different zoom.

### 3. What the screen needs is drawn first and alone

The render thread takes urgent jobs in passes of their own. Look-ahead goes in passes of one
job per thread (`lookahead_batch`), so an urgent request arriving meanwhile waits about one
tile's time, not 32 tiles'.

### 4. Without a render thread, moving around the stage has a time budget

`RenderService::request_batch_within(jobs, budget)` draws, with no render thread, until the
budget is spent (at least one picture) and leaves the rest for the next call. The stage uses
it (8 ms) when the document state and frame are the ones on screen, that is for a pan or a
zoom come to rest: the tiles then agree with each other whatever order they arrive in. An
edit or another frame of the movie is still drawn whole in one call, because half of one
state beside half of another is worse than a late frame.

### 5. Edges that add nothing are left out, and the pictures are the same to the last bit

The oracle's rasterizer accumulates signed area per row. Reading its code:

* an edge wholly above or below the rows being filled is skipped;
* an edge wholly at or right of the row's end deposits into cells past the last pixel, which
  no pixel reads;
* an edge to the **left** does count: it carries the winding into the row.

So `render::prepare` now

* drops from each fill polygon every point whose two neighbours and itself are all above, all
  below, or all right of the surface (`drop_unseen`; one place at a time, since a chord from
  "above" to "right" can cross the surface). The two edges at such a point add nothing and
  neither does the one that replaces them, and the edges that do add are walked in the same
  order;
* flattens a curve whose control points are all in one of those places to its chord
  (`Path::flatten_with`), with a margin below and right for the rounding of the curve's
  points. Only for a fill alone (a stroke's dashes and width follow the length along the
  whole line, and a hairline is not a filled polygon), and only when the path's segment ends
  already reach all four sides of the surface, so that the fill's pixel bounds, which place
  the coverage buffer, are the whole surface either way.

The left side is deliberately not cut: a chord there has the same area mathematically, but
the sum of many small heights and one large one can differ in the last bit of a float, and
the oracle's output must not change (`docs/performance.md`). `cut_tests` draws a set of
scenes cut and whole at five zooms and fourteen tile positions each and compares the float
pixmaps for equality. The GPU stage starts from the same prepared fills (ADR 0019), so it
bins fewer edges too; its parity tests are unchanged.

What is left is linear in the path per tile (transforming each segment and testing its hull):
preparing a view at 6400 % of a 64-stroke drawing went from 3.3 ms to 1.3 ms, and drawing it
on one thread from 33.5 ms to 13.0 ms. A spatial index over a path's segments would make it
follow the visible part alone; not yet.

## Consequences

* Editing, zooming or playing for a long time holds what a short session holds: a drag of 300
  moves ends with two views of pictures instead of the whole budget; the web build's memory
  stops growing after the first moves of a drag (measured in Node, `tests/wasm.rs`).
* A tile deep inside a large drawing walks fewer edges than the whole drawing has at 100 %.
* Undo further back than one step redraws the view (it was a cache hit while the budget
  lasted).
* A zoom gesture shows the old pictures scaled until it rests; it used to show the same on
  the desktop (the answers came too late) and to stutter in the browser.
* `FrameJob` and `FrameImage` have a `set` field; `RenderService` gains `show`, `has_thread`,
  `without_thread` and `request_batch_within`.

## Tests (relations, not timings)

* `render::cut_tests`: cut and whole pictures are equal bit for bit, masks too; a tile deep in
  a drawing walks fewer edges than the stage at 100 %, where whole it walked more with every
  zoom.
* `engine::render_budget_tests`: a drag of 120 states holds what it held after 10, and so
  does a zoom of 80 levels; four laps of a movie hold what one lap held, with a budget above
  and below the movie's size; urgent pictures come within one look-ahead pass; without a
  thread a view fills in one picture a call and matches the oracle; the deepest zoom walks
  fewer edges than 100 % through the stage's own tiles.
* `stage_budget_tests` (the shell, frame by frame): a zoom gesture asks for nothing until it
  rests and then holds one view of textures; 40 edits hold what 5 held.
* `tests/wasm.rs` (in Node): the WebAssembly memory after 132 moves of a drag is what it was
  after 12.
