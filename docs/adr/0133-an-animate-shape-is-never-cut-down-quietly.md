# ADR 0133: An Animate shape is never cut down quietly

* Status: accepted (2026-11-23)
* Relates to: ADR 0009 (XFL import), ADR 0035 (the XFL corpus and golden renders), ADR 0042
  (raw shapes merge when a file opens), ADR 0046 (stroke styles), ADR 0062 (documents macOS
  hands over), issues #498 and #347, backlog item P-006, storytold/craft-libs#3282

## Context

Issue #498: an Animate document opened from its XFL folder "with all of the layers", but "many
lines are entirely missing or filled in". The report came with a screen recording of AniCraft
and no file. In the recording whole outlines are absent (a head without its contour, an arm
whose fill is there and whose line is not) while short strokes and fills beside them are
drawn, and the status bar is not shown.

We may not ask for, or read, files made with Animate that their owner has not released (clean
room), so the cause was looked for with files written by hand:

| Suspect | How it was checked | Result |
|---|---|---|
| The merge of raw shapes when a file opens (ADR 0042) | Generated sketches of up to 400 wobbly strokes, 400 brush-like fills and 18 filled, outlined blobs split over several edges, in decimal and hexadecimal coordinates; rendered as imported and as opened | The same picture: at most 90 of 32 000 inked pixels differ, on antialiased joints. Past 20 000 segments the merge declines and the shapes stay as imported |
| The GPU stage on a heavy frame | An 850-edge sketch opened in the app, drawn by the GPU stage and by the CPU oracle | The same stage |
| The importer | Read for every place that drops part of a shape without a word | Five found, listed below |

The importer dropped, silently:

1. **A stroke of a kind it doesn't know.** `stroke()` returned nothing for a stroke element
   other than the six it names, so every line in that style vanished and the report said
   "Imported everything in the file". A newer Animate version's stroke kinds would do exactly
   what the recording shows.
2. **The rest of an edge after one unreadable command.** The shared edge reader
   (`craft_xfl::edges`) ends at the first command it doesn't know. One `edges` attribute holds
   many runs, each starting at its own move, so a single unknown command lost every later run
   of that edge: lines missing, and fills whose outline no longer closed.
3. **A fill whose outline doesn't close** was closed with a straight line across the gap,
   which can cover an area the fill never had ("filled in"), and nothing said so.
4. **A fill of a kind it doesn't know**, and
5. **edges that name a style their shape doesn't have.**

Which of these, if any, the reporter's file runs into is not known.

## Decision

1. **Nothing of an outline is dropped without a line in the report.** Each of the five has its
   own count, in the words an artist can act on, and `file.open` returns them in `skipped` as
   every other count (so the CLI and MCP see them too).
2. **Drawn the nearest way where there is one.**
   * A stroke of an unknown kind is drawn **solid**, with the weight, colour, caps and joints
     its element gives ("strokes of a kind AniCraft doesn't know (drawn solid)"), as ragged,
     stipple and hatched strokes already are. A plainer line is nearer the drawing than no
     line.
   * A fill of an unknown kind is **left out**: nothing says what colour it is, and a guess
     would paint over artwork.
   * A fill with a gap is still **closed straight across** (most gaps are short, and an empty
     shape is further from the drawing), now counted as "fills with a gap in their outline
     (closed straight across)".
3. **An unreadable command costs its own run only.** The importer hands the shared reader one
   run at a time (`edges::parse_runs`: the attribute cut at each `!`), so the runs after a
   damaged one are read as usual. A run that yields fewer segments than it has drawing
   commands is counted as "outline parts AniCraft couldn't read (left out)". This is the
   app's side of it; the same recovery belongs in the shared reader
   (storytold/craft-libs#3282), and `parse_runs` goes back to one call when it is there.
4. **A corpus package for it** (`crates/xfl/tests/corpus/unknowns/`, ADR 0035): a stroke and a
   fill of kinds that don't exist (their element names are made up for the file), a run cut
   short with a whole run after it, edges in styles their shape lacks and a fill missing a
   side, with the exact report, a golden frame and probes for what must and must not be drawn.
   It goes through the hostile sweep and the `.fla` layouts like every package.

### Issue #347, a `.fla` refused on macOS

The message in the report ("AniCraft cannot open files in the “Animation Document” format") is
the system's own, shown when an app declares a document type and does not take the open
request. v0.2.1, the latest release when it was reported, did not: the handler of ADR 0062 is
not in the commit that release was built from, and no release has carried it since. On
`main` a `.fla` handed over by Finder, the Dock or **Open With** goes through `file.open`
like a drop on the window, and the tests drive that path with the corpus packed as `.fla`.
Nothing more is changed here; it needs a Mac and a build from `main` to confirm.

## Consequences

* A file that used to open with lines missing and a clean report now either shows them (kinds
  AniCraft doesn't know, runs after a damaged one) or says what it left out.
* Reports get longer on files with unknown content. That is the point: the next report of
  missing lines comes with the line that names the cause.
* An unknown stroke kind drawn solid may be heavier or plainer than the original (a tapered or
  textured line becomes an even one).

## Not decided here

* What the unknown kinds are. A file that shows the problem, released by its owner under a
  licence the corpus can take, or its import report from this version, would settle it.
* Edges that hold only cubic curves, hairlines and a minimum line width on screen, strokes with
  a width profile: no public description or corpus evidence for them yet.
