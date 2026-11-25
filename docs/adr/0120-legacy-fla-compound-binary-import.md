# ADR 0120: Legacy Flash import — the compound binary container

Date: 2026-11-16 · Status: accepted — the container (Stage 1), shapes, the timelines
(scenes, layers, keyframes, instances, tweens, library symbols), texts, bitmaps, sounds, button
settings, scripts (kept, not run), shape hints and library folders are implemented; filters,
movie clip component settings and bitmap fills continue

## Context

Files saved by Flash 8 through CS4 — the bulk of the historical Flash authoring corpus —
are OLE2 **compound binary files** (Microsoft `[MS-CFB]`), unlike the zip packages Flash
CS5 and Animate write. The hardened zip reader (ADR 0054) refuses them with the sentence
"this .fla was saved by Flash CS4 or earlier…", which was honest, and
`docs/requirements.md` listed binary FLA as a P1 stretch goal. This ADR began with
reading the container and its record streams (Stage 1); decoding the records into the
document model came after it, and the Decision and Consequences below record both.

## Decision

1. **The container is read by `craft-cfb`**, the shared clean-room MS-CFB reader in
   craft-libs, written from the public specification. `crates/fla`'s `cfb` module keeps only
   the importer's policy on top of it: the hardened zip reader's caps (ADR 0054: 1 GiB per
   file, 256 MiB per stream), stream lookup by full path, and one sentence per refusal.
   Chain walks, cycle guards and directory bounds are `craft-cfb`'s, so every Craft app that
   reads compound files shares one hardened reader. The branch began with its own reader; it
   was replaced before merging, at review, by the shared one (AGENTS.md rule 9).

2. **Record streams read as data first.** Observation on real files establishes the
   stream layout: a 5-byte prologue per stream family (`Contents`: `3F 01 00 00 00`,
   `Media`: `03 05 60 01 58`, library items: `01 FF FF 01 00`), then an object graph
   framed the way MFC's `CArchive` writes objects (its documented behaviour, see below),
   whose class names are 16-bit little-endian length-prefixed ASCII: `CPicPage → CPicLayer → CPicFrame`
   (structure), `CPicShape`, `CPicSymbol`, `CPicText`, `CPicSprite`, `CPicButton`,
   `CPicMorphShape` (+ `CMorphSegment`/`CMorphCurve`/`CMorphHintItem`), `CPicBitmap`,
   `CMediaBits`, `CMediaSound`, `CQTAudioSettings`, `CDocumentPage`, `CColorDef`,
   `CObList`. At Stage 1 the payloads (geometry, fills, transforms, text) were still to be
   decoded; their layouts follow under Consequences.

3. **Reverse-engineer differentially.** EDAP Tools ships the same sample rigs in both
   eras ("for Flash 8–CS6" legacy OLE2, "for Animate" modern zip FLA). Decoding is built
   feature-by-feature against the pair: dump the record from the legacy file, find the
   same artwork's node in the modern twin's XML, decode until they agree. The corpus
   (9 pairs, CC BY-NC-SA) lives **outside** the repo for licensing; committed fixtures
   are synthetic, written by a spec-only writer like ADR 0054's. These published files are
   the format's only evidence: no Flash 8–CS4 files of our own exist. As the clean-room
   standard's fixture rule 2 allows for published files whose licence can't be committed,
   `crates/fla/tests/corpus.tsv` pins each one (URL, sha256, licence) for the tests to fetch.

4. **A microscope command ships now**: `anicraft-cli fla-dump` prints a legacy file's
   stream tree, prologues and record tags as JSON. It is the tool for every later decode
   task and for triaging user files; it also makes the refusal path observable
   (`anicraft-cli info` already names Flash CS4-or-earlier files specifically).

5. **Refusal keeps one sentence.** The container's errors are one sentence each, safe to
   show, never a panic: not a compound file, too large, damaged (with `craft-cfb`'s reason:
   a lying header, a looping chain or tree, a cut-short file), an over-large stream, over a
   limit, no such stream.

## Consequences

- `crates/fla` joins the workspace at L2 (std, `thiserror`, `craft-cfb`, `anicraft-doc`, `anicraft-geom`,
  `anicraft-text` for laying texts out as the XFL importer does, and the L0 `anicraft-flamedia`
  for the media payloads it shares with the XFL importer), with a row in the AGENTS.md map and
  the LAYERS table.
- Record decoding into `anicraft-doc` documents reuses the XFL importer's seam and its
  skipped-report pattern; unknown record kinds are counted, never fatal. The engine's open
  path routes OLE2 bytes to `anicraft_fla::import` before the zip importer sees them.
- **What the structure pass decodes** (differentially verified against the corpus's modern
  twins): the version gate; the `Contents` document index (one `CDocumentPage` per scene:
  page stream name + scene label; the library's stream-name → label pairs); and page
  streams' layer records (constant `00 80 00 00 00 80 00 00 0b` anchor + `ff fe ff`-marked
  UTF-16 name, stored bottom-first). Strings are one-byte-length UTF-16 behind
  `ff fe ff` (names) or `01 80 17` (library stream names) markers.
- **Shape records:** a `CPicShape` body (behind its class name or `0x80nn` class
  reference and a 15-byte object header) holds the full matrix (a–d 16.16, tx/ty i32
  twips), `05`, the edge count, the fill styles (solid RGBA; linear `10` and radial `12`
  gradients with matrix, focal point and stops), the stroke styles (RGBA, width in twips,
  ten style bytes, a fill), then edge records. Each record's flag byte holds four
  2-bit fields — move, control, anchor, style — whose point codes pick the storage
  (i16 in 1/256 twip, i32 in 1/256 twip, i16 in 1/2 twip), all relative to the pen; style
  changes carry stroke/left-fill/right-fill indices as u8 or u16. A record with a control
  point is a quadratic curve. Verified differentially: every shape body the decoder finds
  in the corpus (3197, 2201 of them curved) matches its twin's XFL edges and their fill and
  stroke style indices (the record's matrix and the styles' contents aren't compared). (An earlier reading took the edge unit for half-pixels; it is half-twips.) Fill regions are rebuilt per style as the XFL
  importer does. Bitmap fills are not yet located.
- **Stroke styles:** of a stroke's ten style bytes, [0..2] is a dashed line's dash length
  and [2..4] its gap, both u16 twips; without a dash, [2..4] is `(pattern << 4) | type`,
  the type in the order of Animate's stroke style list (0 solid, 2 dotted, 5 hatched seen;
  3 and 4 presumably ragged and stipple), a dotted line's pattern its dot spacing in tenths
  of a pixel. [6] is the cap, [7] the join, [9] the miter limit; [4] and [8] are zero in
  every corpus stroke, [5] (0 or 3) has no drawing effect found. Dashed and dotted lines map
  to the model's `Dash` as the XFL importer maps `DashedStroke` and `DottedStroke`; ragged,
  stipple and hatched lines draw solid and are counted with the XFL importer's wording; a
  type or byte 4/8 the corpus doesn't show is counted as a style not decoded yet. Verified
  on all 25 patterned strokes of the twins (11 dotted, 8 dashed, 4 hatched, kind, colour,
  width and pattern). The twins omit `dash1`/`dash2` when they are Animate's defaults, and
  the legacy bytes of those strokes read 6 and 6 px (`dash2="9"` is written where it
  differs). Only one dot spacing (3 px) and one hatch setting occur, so the dot spacing's
  unit and the low pattern bits (`2` on the dotted lines) rest on that one sample. Hairlines
  are stored as one-twip widths.
- **Timelines** (`crates/fla/src/archive.rs` holds the full grammar): every item stream is a
  `01` byte and one object graph. Objects are tagged as MFC's `CArchive` tags them — `FF FF`,
  schema, length and name to declare a class; `0x8000 | n` for an object of class `n`; a
  bare `n` for an object written before; `00 00` for none — with classes and objects sharing
  one count. Every `CPic*` object starts with `04`, a flag byte, its children up to `00 00`,
  two i32 (a point in twips, `0x80000000` for none), a u16 and a kind byte. A page's
  children are its layers (bottom-first); a layer's are its keyframes, then its name,
  current/hidden/locked bytes, colour (RGBA), outline, type (0 normal, 1 guide, 3 folder,
  4 mask) and its parent layer as an object reference (a folder is written in place as its
  first child's parent and later back-referenced at its own place in the stack); a
  keyframe's children are its objects, then its raw artwork (a shape body + cubic hints),
  then `18`, the u16 duration, the tween word (low byte none/classic/shape, high byte flags),
  the i16 ease, the sound block, the label, the frame actions, the label kind (1 comment),
  the shape tween's cache and hints (objects), and six custom ease curves (position,
  rotation, scale, colour, filters, all) when flagged. An instance (`CPicSymbol`,
  `CPicSprite` movie clip, `CPicButton`) holds the matrix, first frame, loop mode, colour
  effect (A, R, G, B multipliers in 1/256 and offsets, style, percent, tint), name, the u16
  **library item id from `Contents`** (not the stream number), persistent data, filter count
  and blend (SWF numbering); its base point is the transformation point already placed by
  the matrix. A `CPicShape` object is a drawing object (base u16 1), a loose shape (flag
  bit 0) or a group (members as children). `Contents` gives each library item's u32 id
  and symbol type after its label, and the stage (rectangle in twips, background, 16.16
  frame rate) in the document properties.
- **The keyframe's sound block** (after the ease): u16 sound (a `Contents` media id, 0 none),
  u16 n and n envelope points (u32 position in samples at 44.1 kHz, u16 left and right level
  `0..=32768`), u16 loops, u8 sync, u32 in and out points (`0x3FFFFFFF`: none), u16 zoom.
  The frame actions are a 16-byte header and the frame script (a UTF-16 string, line ends as
  written); the shape-hint list is a `CObList` of `CMorphHintItem`s (i32 start x, y, end x,
  y in twips, u32 key).
- **Buttons and movie clips** carry their own settings after the common instance fields: a
  button `0B`, the 16-byte actions header, its handler script (`on (release) { … }`), a byte
  (track as menu — a hypothesis, 0 throughout the corpus), its **instance name** and u32 0; a
  movie clip `08`, the header, its clip-event script, its name, then component settings
  (not decoded). The instance's own name string in the common fields is empty on every
  corpus instance. The instance class, not the library item's type, is what the twins write
  as an instance's `symbolType`; the symbol's type comes from `Contents`.
- **Texts** (`crates/fla/src/text.rs`): a `CPicText` record is its head (the common base,
  kind `0D`), matrix, bounds, field flags, then runs of an attribute block (size, face,
  RGBA, bold, italic, kerning, position, alignment, spacing, target, rendering) and UTF-16
  characters; it becomes the text block the XFL importer makes of the twin's
  `DOMStaticText` / `DOMDynamicText`, laid out with this computer's fonts.
- **Bitmap instances** (`CPicBitmap`, kind `02`): the matrix, the u16 media id and a byte
  (0); placed as the XFL importer places a `DOMBitmapInstance` (the image's rectangle,
  filled with it).
- **The library in `Contents`** (`crates/fla/src/library.rs`): every item has an identity
  block — u32 6, u32 1, the parent folder's item id, u32 1, its own item id (two u32, the
  twin's `itemID`). Folders are `u32 4`, a name, the identity block; a folder's path is its
  parents' names. Media items are `CMediaBits` / `CMediaSound` objects: version 6, the data
  stream's name, the leaf name, the **u16 media id** (what placements and frame sounds refer
  to — not the stream's number), the source path and import time, the identity block, the
  common item settings, then a sound's format byte (SWF's bits: `0F` 44.1 kHz 16-bit stereo,
  `0E` mono) and sample count. The `Media <n>` / `M <n> <stamp>` streams are byte for byte the
  twins' `bin/*.dat` files, so one L0 decoder (`anicraft-flamedia`) serves both importers:
  Animate's lossless bitmap (zlib in u16-length chunks, premultiplied A, R, G, B rows) and raw
  16-bit PCM (it comes across as a WAV). The publish settings in `Contents` give
  `Vector::ActionScriptVersion` ("2" on every corpus file), the language scripts are kept in.
- **Skipping what isn't decoded.** Filter lists, a movie clip's component settings, text and
  bitmap records in a layout not seen (or that don't end where their parent continues), and
  anything newer are skipped to where their parent continues — its own end
  (`00 00` and its base with its kind byte) or the next sibling, confirmed by parsing that
  sibling, maybe behind references to objects read before — numbering class declarations
  inside the skipped bytes once each as the scan passes them, so later class numbers stay
  right. An object of an already declared class that doesn't parse stops the skip: what it
  holds took object numbers a byte scan can't count, and a later reference would find the
  wrong object. A keyframe whose artwork doesn't decode keeps its timing (its fields are found
  by trial parse). Scan bytes, trial parses, nesting and object counts are capped per
  stream; a timeline that doesn't decode falls back to the structure-only reading. Mapping a
  document converts at most a million objects, and a group that contains itself is left out,
  so groups shared many times over can't stand for exponentially many.
- **Mapping** mirrors the XFL importer: folders hold no frames, zero-length frame records
  are dropped (the twins don't have them), raw shapes merge (ADR 0042), comments are
  reported, classic and shape tweens keep their eases (shape tweens are rebuilt by the
  engine; the stored morph cache is not used), movie clips and buttons loop. A moved
  transformation point becomes the instance's pivot (ADR 0111). Label kind 1 is a comment;
  0 and 2 are taken for a name and an anchor (Flash's menu order, a hypothesis: every corpus
  label is a comment), both a `Keyframe.label`, as XFL's names and anchors are. Shape hints
  go on the shape tween's start keyframe (ADR 0023), each with its start and end point,
  snapped onto the outlines within 2 px as the XFL importer pairs them (the twins keep them in
  `morphHintsList`, which the XFL path doesn't read, so here the legacy file opens with more
  than its twin). A frame sound keeps its sound, sync (0 event, 1 start, 2 stop, 3 stream —
  only stream is in the corpus, the rest a hypothesis by Flash's menu order), repeats and
  envelope (the format has no effect byte: an envelope plays as its points, as the XFL
  importer reads one without a preset name); in and out points are reported (played whole),
  as there. Frame scripts are kept verbatim as `FrameScript::Source` in the document's
  language, never run by the import; handler scripts on buttons and movie clips are reported
  ("scripts on instances"), as the XFL importer reports them. A button's name becomes the
  element's instance name when actions can use it. Library items go into their folders;
  empty folders come across too.
- **Verified differentially** against all 9 pairs: 337/337 timelines have the twins' layers,
  1628/1628 layers their keyframes (index and duration), 6993/6993 keyframes their tween
  kind, 4457/4457 instances their library item, matrix, loop mode, first frame and instance
  name (18 buttons, 3 named); 335/335 non-empty texts sit on the twins' keyframes with their
  kind, characters and position (372/372 text records pair with their twin's, and 371 match
  field for field: one differs by a twip in its left edge and width, a difference the test
  pins); 9/9 bitmap
  instances and bitmap items (name, folder, size), 5/5 frame sounds (sound, sync, repeats,
  envelope) and 4/4 sound items (folder, rate, channels, length); 39/39 library folders and
  328/328 symbols at their twins' library paths and types; the stage size, frame rate and
  colour match on every pair.
  Rendered frames of the movie template and the character rigs match their twins' apart
  from text antialiasing on a few pixels, and the shape tweens whose hints
  the twins' XFL path drops.
- **What stays open**, each counted in the report's `skipped` per kind: filters (an instance
  with filters loses its blend mode too; the corpus's six filtered instances all hold one
  blur in a 52-byte block before the blend — a type byte 1, then what look like distance,
  blur x, blur y and angle as f32 and quality and strength as u32 — too few samples to tell
  the fields apart), a movie clip's component settings, bitmap fills (kinds `40`–`43`, absent
  from the corpus), ragged, stipple and hatched strokes (drawn solid, as from XFL), sound in and out points (as in the XFL
  importer), a button's track-as-menu byte (no place in the model), sound formats other than
  raw 16-bit PCM and bitmaps other than the lossless layout or PNG / JPEG. Committed fixtures
  stay synthetic; the CC BY-NC-SA corpus runs via `ANICRAFT_FLA_CORPUS`, pinned by URL and
  sha256 in `crates/fla/tests/corpus.tsv` and fetched by `scripts/fetch-fla-corpus.sh`.

## Review at integration (2026-11-24)

The first-party review before merging (backlog P-039) checked three things and changed what
they asked for.

- **Provenance.** The account in #478 was checked against `standards/clean-room.md`. The
  knowledge comes from Microsoft's public `[MS-CFB]` and MFC documentation, Adobe's public
  help pages, and corpus evidence: sample files their author published on his own site in
  both formats, obtained from that source, never committed, and pinned by URL, sha256 and
  licence in a manifest the tests fetch from (fixture rule 2; a fresh fetch on 2026-11-24
  matched all 16 hashes, and the differential tests reproduce the numbers above). No app was
  operated and no parser or decompiler source was read; the container reader whose author had
  looked at a BSD library's source was replaced by `craft-cfb` before merging. One committed
  test reproduced a shape of the corpus (its geometry, fill, matrix and name): it now holds
  values of our own.
- **A second, independent file.** A Flash 8 document a community member made and attached to
  #478 (not committed: its maker hasn't stated a licence) was read locally. Every item stream
  parses to its end with the grammar above, with one skip, a movie clip's component settings.
  It confirms on a file that isn't the corpus's: the stage (550 × 400, 12 fps), layer names,
  lock and outline colours, the guide layer type (1), keyframe durations, classic tweens and
  their eases (+100, −100), a graphic instance's first frame following its keyframes, a shape
  tween with its morph cache, symbol types (graphic, button with its four frames, movie clip),
  static texts with face, size, colour, bold and italic, a linear and a radial gradient, and a
  30 s mono sound whose sample count is exactly its stream's length. Two things it doesn't
  settle: the byte read as a frame's sound sync is 3 on its sound frame, and also on some
  frames with no sound, so its meaning beyond "3 on the corpus's stream sounds" stays a
  hypothesis; and its sound repeats 32,767 times, which is likely Flash's Loop setting and
  comes across as the most repeats a sound holds.
- **Hostile files.** 40,000 mutated copies of that document (byte and bit flips, extreme
  16- and 32-bit values, copied blocks, in the header, the directory and every record stream)
  and 600 truncations: no panic, the slowest import 34 ms. Reading the code found what random
  mutation doesn't, small files that multiply work, and these are now bounded for the whole
  document:
  - streams are read against an allowance of twice the file's size (one stream named by
    thousands of symbols, scenes or media items is no longer read and decoded for each);
  - every byte a stream's decoders read, again after a rolled-back trial too, is charged to a
    work budget of 64 times the stream's length (it was a fixed 256 MiB of scanning per
    stream, and trial decodes of shapes and texts weren't charged);
  - a keyframe (with its script and hints) and a text's characters count against the million
    objects of a document, since references can place one record many times;
  - bitmaps share the XFL importer's 256-megapixel budget, refused by their header;
  - at most 20,000 symbols and 4,096 scenes are converted; the `Contents` scans step over
    strings by their head, so a candidate that isn't a record costs nothing;
  - the report says when the stage wasn't found (the defaults are used), a list was cut at
    its cap, a media id is listed twice, or an envelope was cut.
  The bounded file read that opens a `.fla` now applies the automation read root itself
  (ADR 0124), as every other read in `engine::vfs` does.

## References

- Microsoft `[MS-CFB]`: Compound File Binary Format specification (public).
- `craft-cfb` in storytold/craft-libs (`crates/cfb`): the shared reader of that specification.
- Microsoft's MFC documentation of `CArchive` object serialization (`CArchive::WriteObject`,
  `CRuntimeClass` schemas): the public description of the object framing.
- EDAP Tools' sample rigs and templates, https://flash-powertools.com/downloads/ (CC BY-NC-SA 3.0,
  not committed): every file the format was learned from, pinned by URL and sha256 in
  `crates/fla/tests/corpus.tsv`.
