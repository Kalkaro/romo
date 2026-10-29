use crate::{
    geometry::{Corner, Pos2, Rect, Vec2, pos2, vec2},
    model::{Board, Kind},
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Guide {
    pub start: Pos2,
    pub end: Pos2,
}

/// Pen strokes whose bounds come this close, in board units, form one drawing.
const DRAWING_GAP: f32 = 12.0;
/// Edges and centers this close on screen, in pixels, snap together.
const THRESHOLD: f32 = 6.0;
/// Only objects this close on screen, in pixels, to what is being moved or
/// resized are snap targets, so distant objects that happen to line up don't
/// pull it around.
const RANGE: f32 = 320.0;

/// Rectangles that objects may align to, skipping `exclude`d objects. Strokes
/// that touch or overlap merge into one drawing; lone strokes and connectors
/// are not targets, so objects align to whole drawings rather than single marks.
pub fn targets(board: &Board, exclude: impl Fn(u64) -> bool) -> Vec<Rect> {
    let mut targets = Vec::new();
    let mut strokes = Vec::new();
    for item in board.items.iter().filter(|item| !exclude(item.id)) {
        match item.kind {
            Kind::Pen => strokes.push(item.rect()),
            Kind::Arrow => {}
            _ => targets.push(item.rect()),
        }
    }
    targets.extend(drawings(strokes));
    targets
}

/// The bounds of each group of two or more strokes that touch or overlap.
fn drawings(mut strokes: Vec<Rect>) -> Vec<Rect> {
    fn root(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    strokes.sort_by(|a, b| a.min.x.total_cmp(&b.min.x));
    let mut parent: Vec<usize> = (0..strokes.len()).collect();
    for (i, stroke) in strokes.iter().enumerate() {
        let reach = stroke.expand(DRAWING_GAP);
        // Sorted by left edge, so every later stroke starts even further right.
        for (j, other) in strokes.iter().enumerate().skip(i + 1) {
            if other.min.x > reach.max.x {
                break;
            }
            if reach.intersects(*other) {
                let (a, b) = (root(&mut parent, i), root(&mut parent, j));
                parent[a] = b;
            }
        }
    }
    let mut groups = vec![(Rect::NOTHING, 0); strokes.len()];
    for (i, stroke) in strokes.iter().enumerate() {
        let group = &mut groups[root(&mut parent, i)];
        group.0 = group.0.union(*stroke);
        group.1 += 1;
    }
    groups
        .into_iter()
        .filter(|(_, count)| *count > 1)
        .map(|(bounds, _)| bounds)
        .collect()
}

/// Snap the entire selection together, in screen pixels, to the edges and
/// centers of nearby objects.
pub fn align(moving: Rect, targets: &[Rect], zoom: f32) -> (Vec2, Vec<Guide>) {
    let near = near(moving, targets, zoom);
    let (x, y) = nearest(&x_lines(moving), &y_lines(moving), &near, zoom);
    let shift = vec2(x.unwrap_or(0.0), y.unwrap_or(0.0));
    let moved = Rect::from_min_size(moving.min + shift, moving.size());
    let guides = guides(moved, &x_lines(moved), &y_lines(moved), &near, zoom);
    (shift, guides)
}

/// Snap `point`, the corner of a box being drawn out from `anchor`.
pub fn point(point: Pos2, anchor: Pos2, targets: &[Rect], zoom: f32) -> (Pos2, Vec<Guide>) {
    let near = near(Rect::from_two_pos(anchor, point), targets, zoom);
    let (x, y) = nearest(&[point.x], &[point.y], &near, zoom);
    let point = point + vec2(x.unwrap_or(0.0), y.unwrap_or(0.0));
    let drawn = Rect::from_two_pos(anchor, point);
    (point, guides(drawn, &[point.x], &[point.y], &near, zoom))
}

/// `bounds` with its `corner` dragged by `delta` while the opposite corner
/// stays put, at least `min` in size. A `uniform` resize keeps its proportions.
/// The dragged corner snaps to nearby `targets`; a uniform resize snaps
/// whichever side lies closer and the other follows. Sides thinner than a
/// board unit, like a straight stroke's, keep their size.
pub fn resize(
    bounds: Rect,
    corner: Corner,
    delta: Vec2,
    uniform: bool,
    min: Vec2,
    targets: &[Rect],
    zoom: f32,
) -> (Rect, Vec<Guide>) {
    let (size, sign) = (bounds.size(), corner.sign());
    let anchor = corner.opposite().of(bounds);
    let least = |min: f32, side: f32| if side < 1.0 { 0.0 } else { min / side };
    let smallest = least(min.x, size.x).max(least(min.y, size.y));
    // How far the dragged corner may reach from the anchor, along each side.
    let fit = |reach: Vec2| {
        if uniform {
            // Project the dragged corner onto the diagonal.
            size * (reach.dot(size) / size.length_sq().max(1.0)).max(smallest)
        } else {
            let extent = |reach: f32, side: f32, min: f32| {
                if side < 1.0 { side } else { reach.max(min) }
            };
            vec2(extent(reach.x, size.x, min.x), extent(reach.y, size.y, min.y))
        }
    };
    let rect = |reach: Vec2| Rect::from_two_pos(anchor, anchor + reach * sign);
    let mut reach = fit((corner.of(bounds) + delta - anchor) * sign);
    let near = near(rect(reach), targets, zoom);
    let tip = anchor + reach * sign;
    let (x, y) = nearest(&[tip.x], &[tip.y], &near, zoom);
    reach = if uniform {
        // How far each side's snap moves the corner, and the scale it needs.
        let snapped = |shift: Option<f32>, reach: f32, sign: f32| {
            shift
                .filter(|_| reach >= 1.0)
                .map(|shift| (shift.abs(), (reach + shift * sign) / reach))
        };
        [snapped(x, reach.x, sign.x), snapped(y, reach.y, sign.y)]
            .into_iter()
            .flatten()
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map_or(reach, |(_, factor)| fit(reach * factor))
    } else {
        fit(reach + vec2(x.unwrap_or(0.0), y.unwrap_or(0.0)) * sign)
    };
    let (resized, tip) = (rect(reach), anchor + reach * sign);
    (resized, guides(resized, &[tip.x], &[tip.y], &near, zoom))
}

/// The left edge, center and right edge of `r`.
fn x_lines(r: Rect) -> [f32; 3] {
    [r.min.x, r.center().x, r.max.x]
}

/// The top edge, center and bottom edge of `r`.
fn y_lines(r: Rect) -> [f32; 3] {
    [r.min.y, r.center().y, r.max.y]
}

/// The targets within range of `moving` on screen.
fn near(moving: Rect, targets: &[Rect], zoom: f32) -> Vec<Rect> {
    targets
        .iter()
        .copied()
        .filter(|target| {
            let gap = vec2(
                (target.min.x - moving.max.x).max(moving.min.x - target.max.x),
                (target.min.y - moving.max.y).max(moving.min.y - target.max.y),
            );
            gap.max(Vec2::ZERO).length() * zoom <= RANGE
        })
        .collect()
}

/// The smallest shifts within the snap threshold that bring one of `xs`, and
/// one of `ys`, onto a target edge or center.
fn nearest(xs: &[f32], ys: &[f32], targets: &[Rect], zoom: f32) -> (Option<f32>, Option<f32>) {
    let threshold = THRESHOLD / zoom;
    let closest = |values: &[f32], lines: fn(Rect) -> [f32; 3]| {
        let mut best: Option<f32> = None;
        for &target in targets {
            for line in lines(target) {
                for value in values {
                    let d = line - value;
                    if d.abs() <= threshold && best.is_none_or(|old| d.abs() < old.abs()) {
                        best = Some(d);
                    }
                }
            }
        }
        best
    };
    (closest(xs, x_lines), closest(ys, y_lines))
}

/// A guide along each of `xs` and `ys` that meets target edges or centers,
/// spanning `moving` and every target on that line.
fn guides(moving: Rect, xs: &[f32], ys: &[f32], targets: &[Rect], zoom: f32) -> Vec<Guide> {
    // Within half a pixel, lines look aligned.
    let (tolerance, margin) = (0.5 / zoom, 12.0 / zoom);
    let span = |at: f32, lines: fn(Rect) -> [f32; 3]| {
        targets
            .iter()
            .copied()
            .filter(|target| lines(*target).iter().any(|line| (line - at).abs() <= tolerance))
            .reduce(Rect::union)
            .map(|aligned| aligned.union(moving))
    };
    let mut guides = Vec::new();
    for &x in xs {
        if let Some(span) = span(x, x_lines) {
            guides.push(Guide {
                start: pos2(x, span.min.y - margin),
                end: pos2(x, span.max.y + margin),
            });
        }
    }
    for &y in ys {
        if let Some(span) = span(y, y_lines) {
            guides.push(Guide {
                start: pos2(span.min.x - margin, y),
                end: pos2(span.max.x + margin, y),
            });
        }
    }
    // A box without width or height repeats its lines.
    guides.dedup();
    guides
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::PALETTE;

    #[test]
    fn group_alignment_preserves_offsets_and_uses_screen_distance() {
        let target = Rect::from_min_size(pos2(100.0, 100.0), vec2(100.0, 100.0));
        let moving = Rect::from_min_size(pos2(96.0, 300.0), vec2(30.0, 30.0));
        let (delta, guides) = align(moving, &[target], 1.0);
        assert_eq!(delta, vec2(4.0, 0.0));
        assert_eq!(guides.len(), 1);
        assert_eq!(align(moving, &[target], 3.0).0, Vec2::ZERO);
        assert_eq!(align(moving, &[], 1.0).0, Vec2::ZERO);
    }

    #[test]
    fn only_nearby_objects_attract_and_guides_span_every_aligned_one() {
        let left = Rect::from_min_max(pos2(0.0, 0.0), pos2(100.0, 100.0));
        let right = Rect::from_min_max(pos2(300.0, 60.0), pos2(340.0, 100.0));
        // Lines up more closely, but lies 800 units away.
        let far = Rect::from_min_max(pos2(1000.0, 101.0), pos2(1100.0, 201.0));
        let moving = Rect::from_min_max(pos2(150.0, 103.0), pos2(200.0, 153.0));
        let (shift, guides) = align(moving, &[left, far, right], 1.0);
        assert_eq!(shift, vec2(0.0, -3.0));
        assert_eq!(
            guides,
            vec![Guide {
                start: pos2(-12.0, 100.0),
                end: pos2(352.0, 100.0),
            }]
        );
        // Zoomed out, the far object is near on screen.
        assert_eq!(align(moving, &[left, far, right], 0.25).0, vec2(0.0, -2.0));
    }

    #[test]
    fn corners_resize_from_the_opposite_corner_and_snap() {
        let bounds = Rect::from_min_max(Pos2::ZERO, pos2(200.0, 100.0));
        let min = vec2(40.0, 40.0);
        let (r, _) = resize(bounds, Corner::TopLeft, vec2(50.0, -20.0), false, min, &[], 1.0);
        assert_eq!(r, Rect::from_min_max(pos2(50.0, -20.0), pos2(200.0, 100.0)));
        // Dragged past the opposite corner, it stops at the smallest size.
        let (r, _) = resize(bounds, Corner::TopLeft, vec2(500.0, 500.0), false, min, &[], 1.0);
        assert_eq!(r, Rect::from_min_max(pos2(160.0, 60.0), pos2(200.0, 100.0)));
        let (r, _) = resize(bounds, Corner::BottomLeft, vec2(-200.0, 0.0), true, min, &[], 1.0);
        assert_eq!(r, Rect::from_min_max(pos2(-160.0, 0.0), pos2(200.0, 180.0)));
        // The free corner meets the target on both axes.
        let target = Rect::from_min_max(pos2(403.0, 150.0), pos2(500.0, 202.0));
        let delta = vec2(200.0, 100.0);
        let (r, guides) = resize(bounds, Corner::BottomRight, delta, false, min, &[target], 1.0);
        assert_eq!(r, Rect::from_min_max(Pos2::ZERO, pos2(403.0, 202.0)));
        assert_eq!(guides.len(), 2);
        // A uniform resize snaps the closer side and keeps its proportions.
        let (r, guides) = resize(bounds, Corner::BottomRight, delta, true, min, &[target], 1.0);
        assert!(r.min == Pos2::ZERO && r.max.distance(pos2(404.0, 202.0)) < 0.001);
        assert_eq!(guides.len(), 1);
        assert!(guides[0].start.y == guides[0].end.y);
    }

    #[test]
    fn touching_strokes_snap_as_one_drawing_and_lone_marks_are_skipped() {
        let mut board = Board::default();
        let mut stroke = |from: Pos2, to: Pos2| {
            let id = board.add(Kind::Pen, from, PALETTE[7]);
            board.get_mut(id).unwrap().points = vec![Pos2::ZERO, (to - from).to_pos2()];
            id
        };
        let first = stroke(pos2(0.0, 0.0), pos2(100.0, 40.0));
        stroke(pos2(105.0, 20.0), pos2(160.0, 90.0));
        stroke(pos2(600.0, 600.0), pos2(640.0, 620.0));
        let note = board.add(Kind::Note, pos2(300.0, 0.0), PALETTE[0]);
        let arrow = board.add(Kind::Arrow, pos2(0.0, 300.0), PALETTE[7]);
        board.get_mut(arrow).unwrap().points = vec![Pos2::ZERO, pos2(200.0, 0.0)];
        let note = board.get(note).unwrap().rect();
        let found = targets(&board, |_| false);
        assert_eq!(found.len(), 2);
        assert!(found.contains(&note));
        assert!(found.contains(&Rect::from_min_max(Pos2::ZERO, pos2(160.0, 90.0))));
        // Moving one stroke of a pair leaves its partner as a lone, ignored mark.
        assert_eq!(targets(&board, |id| id == first), vec![note]);
    }
}
