# ADR 0131: Several layers selected at once, a ruler that selects no frames, and new masks locked

* Status: accepted (2026-11-23)
* Relates to: backlog items P-009 and P-010 in `docs/roadmap.md`; issues #195, #429, #512, #523,
  #513, #159, #246, #255; outside pull requests #222, #532 and #535;
  [ADR 0010](0010-mask-layers.md), [ADR 0044](0044-selection-across-layers.md),
  [ADR 0061](0061-timeline-polish.md), [ADR 0087](0087-dockable-panels-second-slice.md)

## Context

P-009 and P-010 are bug lists. Most of their fixes are small and need no record. Three of them
decide how the timeline behaves from now on, and each could reasonably have gone another way.

1. **#512.** The layer list had one selected layer: the active one. Artists coming from
   Animate hold Ctrl or Shift to select several, drag them into a folder together, and expect
   the folder button to gather them.
2. **#195.** With frames selected on one layer, a click on the playhead left that selection
   alone while the playhead stayed inside it, so F5 went on adding frames to one layer. The
   reference app adds them to every layer after a click on the ruler. Outside PR #222 fixed it
   by selecting the playhead's frame on every layer.
3. **#429.** A new mask shows its edge on the stage in the layer colour, because its layer is
   unlocked. The reporter read the edge as a rendering fault.

## Decisions

### The selected layers are session state, valid only while they hold the active layer

`Session::layer_sel` lists the layers picked together; `Session::picked_layers()` is how it is
read. The list counts only while it contains the active layer and its layers are in the edited
timeline; otherwise the active layer alone is selected.

Many commands set the active layer directly (a new layer, a deleted one, entering a symbol,
another scene, undo). Clearing the list in each of them would be a rule every future command
has to remember. With the validity rule none of them needs to know the list exists, and a
stale list can never make a command act on layers the artist no longer sees as selected.

The selection is not in the document and not in the undo history, like the active layer and
the artwork selection.

`layer.select` takes `add: "toggle"` (Ctrl-click) and `add: "range"` (Shift-click, counted
from the active layer, which stays the anchor, as in a list box). The last layer can't be
toggled out: a timeline always has an active layer. The shell adds Shift+Up and Shift+Down on
the focused timeline, so the selection can be made without a pointer.

### Which commands act on the selection

`layer.move` and `layer.delete` act on `layers: [ids]`, else `layer`, else the selected layers.
Both stay one undo step and are all or none. Moved layers land together in the order they had;
a layer selected along with the folder or mask that holds it moves once, inside it; the camera
layer, selected with others, stays on top.

`layer.new_folder` puts several selected layers into the new folder, which lands where the top
one was. **This departs from the reference app**, where New Folder always makes an empty
folder. The reporter of #512 asked for it, it is what the button does in most layer lists
artists also use, and an empty folder above a multi-selection is rarely what was meant.
`wrap: false` gives the empty folder; with one layer selected nothing changes.

The row toggles (eye, padlock, outline, onion) and Move Layer Up and Down still act on one
layer. Layer Properties for several layers is P-027.

### A click on the ruler leaves no frames selected

The ruler is not a frame of any one layer, so a click or a drag on it drops the frame
selection, also when the playhead stays inside it. The frame keys then follow the rule `main`
already had for "no frames selected" (#242, #438): F5, Shift+F5 and Shift+F6 act on every layer
that reaches the playhead, and F6 and F7 on the active layer.

PR #222's approach (select the playhead's frame on every layer) is merged for its test and
credit and then changed, for two reasons:

* After any scrub, F7 would have put a blank keyframe on every layer and Shift+F6 cleared a
  keyframe on every layer. Scrubbing is the most common thing done on a timeline; a key
  pressed after it must not empty the stage.
* It also sent Ctrl+C and Ctrl+X to the frames instead of the selected artwork after a scrub.

### A new mask is locked

`layer.toggle_mask` locks the mask layer it makes, as the reference app does and as the SVG
importer already did. The rule of [ADR 0010](0010-mask-layers.md) stays: an unlocked mask
shows its edge in the layer colour, so it can be seen, picked and reshaped; a locked one shows
only its result. The masked layer is not locked: in AniCraft the mask applies whether or not
the layers are locked, so locking the artwork would only be in the way.

Hiding the edge unless the mask layer is active was considered and rejected: an unlocked
mask's shapes can be picked on the stage, and something that can be picked must be visible.

`layer.set_type` and Layer Properties do not lock: there the type is one field among several
the artist is setting by hand.

## Not reproduced

* **#159** (a side dock that grows under the pointer at interface scales of 85, 95, 110, 135,
  160, 170 and 185 %, reported on 0.2.1). On `main` the stage's rectangle did not move in 54
  combinations of screen density, window size and interface scale, nor with content wider
  than the dock. A dock's size comes from egui's panel state, which grows only when what is
  inside is wider than the panel; [ADR 0087](0087-dockable-panels-second-slice.md) holds every
  panel's content to the dock's width. A test now pins that at the reported scales. The issue
  stays open for the reporters to retest on a current build.
* **#255** (the window flickers after it regains focus, Intel Arc). It needs the machine.
  Preferences › Window graphics was added for it earlier; nothing changes here.

## Consequences

* Commands that take `layer?` and should act on several layers have one helper,
  `cmds_timeline::layer_targets`.
* Scripts that ran `layer.toggle_mask` and then edited the mask layer must unlock it first
  (`layer.toggle_lock {on: false}`); the command's result says `locked: true`.
* `anicraft-cli info` and the MCP state report `layer_selection`.
