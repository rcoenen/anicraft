//! XFL shape edges → AniCraft paths.
//!
//! A `DOMShape` stores its outline as `<Edge>` elements. Each carries an `edges` string of
//! drawing commands in twips (1/20 px): `!x y` moves, `|x y` or `/x y` draws a line, `[cx cy x y`
//! or `]cx cy x y` draws a quadratic curve; `Sn` marks selection state and is skipped. Numbers
//! are decimal, or `#` hex fixed point (signed 24.8). An edge names the fill style on its
//! left (`fillStyle0`) and right (`fillStyle1`) and an optional stroke style.
//!
//! An `edges` string holds one run of segments per `!`. A command the reader doesn't know ends
//! its own run only: the runs after it are read as usual and the damaged one is counted, so the
//! import can say that part of an outline is missing (ADR 0133).
//!
//! Fill regions are rebuilt per fill style: its right-side edges as drawn, its left-side edges
//! reversed, then chained end to start into closed loops. Strokes are the stroked edges in
//! drawing order. Written from the public description of the format; no Animate code involved.

use anicraft_geom::{Path, Vec2};
use std::collections::HashMap;

/// One segment in pixels: a line (`ctrl` = None) or a quadratic curve.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Seg {
    pub from: Vec2,
    pub ctrl: Option<Vec2>,
    pub to: Vec2,
}

impl Seg {
    fn reversed(self) -> Seg {
        Seg { from: self.to, ctrl: self.ctrl, to: self.from }
    }
}

/// An `<Edge>`: its fill styles (left, right), stroke style and segments.
#[derive(Clone, Debug, Default)]
pub struct Edge {
    pub fill_left: u32,
    pub fill_right: u32,
    pub stroke: u32,
    pub segs: Vec<Seg>,
}

/// What an `edges` attribute decodes to.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Parsed {
    pub segs: Vec<Seg>,
    /// Runs that stop before their end, at a command the reader doesn't know.
    pub unread: usize,
}

/// Decodes an `edges` attribute through craft-libs, run by run, and maps its neutral geometry to
/// AniCraft. The shared reader stops at the first command it doesn't know; a run starts at a
/// move and depends on nothing before it, so each is handed over on its own and only a damaged
/// run loses its end.
pub fn parse_runs(s: &str) -> Parsed {
    let mut out = Parsed::default();
    let point = |p: craft_xfl::edges::Point| Vec2::new(p.x, p.y);
    let mut rest = s;
    while !rest.is_empty() {
        // Up to the next move (`!` is ASCII, so the cut is on a character boundary).
        let end = rest.bytes().skip(1).position(|b| b == b'!').map_or(rest.len(), |i| i + 1);
        let (run, tail) = rest.split_at(end);
        rest = tail;
        let drawn = run.bytes().filter(|b| matches!(b, b'|' | b'/' | b'[' | b']')).count();
        let room = craft_xfl::edges::MAX_EDGE_SEGMENTS.saturating_sub(out.segs.len());
        if room == 0 {
            // Full: what the runs still to come would draw is left out, and counted undecoded.
            out.unread += usize::from(drawn > 0);
            continue;
        }
        let read = craft_xfl::edges::parse_edge_segments(run);
        if read.len() < drawn || read.len() > room {
            out.unread += 1;
        }
        out.segs.extend(read.into_iter().take(room).map(|segment| Seg { from: point(segment.from), ctrl: segment.control.map(point), to: point(segment.to) }));
    }
    out
}

/// The segments of an `edges` attribute (see [`parse_runs`]).
pub fn parse(s: &str) -> Vec<Seg> {
    parse_runs(s).segs
}

/// Hash key for an end point (to 1/100 px, which absorbs the format's rounding).
fn key(p: Vec2) -> (i64, i64) {
    ((p.x * 100.0).round() as i64, (p.y * 100.0).round() as i64)
}

fn push_seg(path: &mut Path, s: &Seg) {
    match s.ctrl {
        Some(q) => path.quad_to(q, s.to),
        None => path.line_to(s.to),
    };
}

/// The region of fill style `style`, as closed loops.
pub fn fill_path(edges: &[Edge], style: u32) -> Path {
    fill_region(edges, style).0
}

/// [`fill_path`], and how many of its loops don't come back to their start: part of the outline
/// is missing there, and the loop is closed straight across the gap.
pub fn fill_region(edges: &[Edge], style: u32) -> (Path, usize) {
    let mut segs: Vec<Seg> = Vec::new();
    for e in edges {
        if e.fill_right == style {
            segs.extend(e.segs.iter().copied());
        }
        if e.fill_left == style {
            segs.extend(e.segs.iter().rev().map(|s| s.reversed()));
        }
    }
    let mut by_start: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
    for (i, s) in segs.iter().enumerate() {
        // Zero-length segments carry no area and would confuse the chaining.
        if key(s.from) != key(s.to) {
            by_start.entry(key(s.from)).or_default().push(i);
        }
    }
    let mut used = vec![false; segs.len()];
    let mut path = Path::new();
    let mut gaps = 0;
    for first in 0..segs.len() {
        if used.get(first).copied().unwrap_or(true) || segs.get(first).is_some_and(|s| key(s.from) == key(s.to)) {
            continue;
        }
        let Some(&start) = segs.get(first) else { continue };
        path.move_to(start.from);
        let mut at = first;
        let mut closed = false;
        while let Some(s) = segs.get(at).copied() {
            if let Some(u) = used.get_mut(at) {
                *u = true;
            }
            push_seg(&mut path, &s);
            if key(s.to) == key(start.from) {
                closed = true;
                break;
            }
            let next = by_start.get(&key(s.to)).and_then(|v| v.iter().copied().find(|&j| !used.get(j).copied().unwrap_or(true)));
            match next {
                Some(j) => at = j,
                None => break,
            }
        }
        if !closed {
            gaps += 1;
        }
        path.close();
    }
    (path, gaps)
}

/// The stroked edges of stroke style `style`, in drawing order.
pub fn stroke_path(edges: &[Edge], style: u32) -> Path {
    let mut path = Path::new();
    let mut last: Option<Vec2> = None;
    for e in edges.iter().filter(|e| e.stroke == style) {
        for s in &e.segs {
            if last.is_none_or(|l| key(l) != key(s.from)) {
                path.move_to(s.from);
            }
            push_seg(&mut path, s);
            last = Some(s.to);
        }
    }
    path
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_commands_and_numbers() {
        let s = parse("!0 0|200 0[400 0 400 200S2/0 200!#A.8 -20");
        assert_eq!(s.len(), 3);
        assert_eq!(s[0].to, Vec2::new(10.0, 0.0), "twips to pixels");
        assert_eq!(s[1].ctrl, Some(Vec2::new(20.0, 0.0)));
        assert_eq!(s[2], Seg { from: Vec2::new(20.0, 10.0), ctrl: None, to: Vec2::new(0.0, 10.0) });
        // Hex fixed point: #A.8 = 10.5 twips; a negative two's-complement value.
        let hex = parse("!#A.8 -20|#FFFFFF.80 0");
        assert_eq!(hex[0].from, Vec2::new(0.525, -1.0));
        assert_eq!(hex[0].to, Vec2::new(-0.025, 0.0));
        assert!(parse("!1 2|x").is_empty(), "stops at garbage, never panics");
    }

    /// A command the reader doesn't know costs its own run its end, and no other run anything.
    #[test]
    fn a_damaged_run_loses_only_its_own_end() {
        let whole = parse_runs("!0 0|200 0!400 0|600 0[800 0 800 200");
        assert_eq!((whole.segs.len(), whole.unread), (3, 0));
        let cut = parse_runs("!0 0|200 0?7|300 0!400 0|600 0[800 0 800 200");
        assert_eq!(cut.unread, 1);
        assert_eq!(cut.segs, whole.segs, "the first run up to the cut, and the run after it whole");
        // Before the first move too, and a run that is nothing but the unknown command.
        let lead = parse_runs("|200 0?!400 0|600 0!?");
        assert_eq!((lead.segs.len(), lead.unread), (2, 0), "no drawing command was lost");
        assert_eq!(parse_runs("!0 0|x!0 0|200 0").unread, 1);
    }

    #[test]
    fn a_square_split_into_edges_with_fills_on_either_side() {
        // Top and right drawn clockwise with the fill on the right; bottom and left drawn the
        // other way with the fill on the left. Both describe the same square.
        let e = |l, r, d: &str| Edge { fill_left: l, fill_right: r, stroke: 1, segs: parse(d) };
        let edges = vec![e(0, 1, "!0 0|200 0|200 200"), e(1, 0, "!0 0|0 200|200 200")];
        let p = fill_path(&edges, 1);
        let lines = p.flatten(&anicraft_geom::Affine::IDENTITY, 0.1);
        assert_eq!(lines.len(), 1, "one closed loop");
        assert!(lines[0].closed);
        assert!(p.contains(Vec2::new(5.0, 5.0), &anicraft_geom::Affine::IDENTITY, anicraft_geom::FillRule::EvenOdd));
        assert!(!p.contains(Vec2::new(15.0, 5.0), &anicraft_geom::Affine::IDENTITY, anicraft_geom::FillRule::EvenOdd));
        let st = stroke_path(&edges, 1);
        assert_eq!(st.anchors().len(), 6, "two runs of two segments each, plus their starts");
        assert!(fill_path(&edges, 7).is_empty());
    }
}
