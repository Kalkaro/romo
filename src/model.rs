use crate::geometry::{Color32, Pos2, Rect, Vec2, pos2, vec2};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

pub const PALETTE: [Color32; 8] = [
    Color32::from_rgb(24, 24, 27),
    Color32::from_rgb(39, 39, 42),
    Color32::from_rgb(63, 63, 70),
    Color32::from_rgb(82, 82, 91),
    Color32::from_rgb(113, 113, 122),
    Color32::from_rgb(161, 161, 170),
    Color32::from_rgb(212, 212, 216),
    Color32::from_rgb(250, 250, 250),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kind {
    Note,
    Rectangle,
    Ellipse,
    Diamond,
    Text,
    Frame,
    Pen,
    Arrow,
    Image,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Note => "Sticky note",
            Self::Rectangle => "Rectangle",
            Self::Ellipse => "Ellipse",
            Self::Diamond => "Diamond",
            Self::Text => "Text",
            Self::Frame => "Frame",
            Self::Pen => "Drawing",
            Self::Arrow => "Connector",
            Self::Image => "Image",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Item {
    pub id: u64,
    pub kind: Kind,
    pub pos: Pos2,
    pub size: Vec2,
    pub color: Color32,
    pub text: String,
    pub font_size: f32,
    pub stroke_width: f32,
    pub points: Vec<Pos2>,
    pub start_anchor: Option<u64>,
    pub end_anchor: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<std::sync::Arc<crate::images::ImageAsset>>,
    /// How much a pen stroke has been scaled since it was drawn.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<f32>,
    /// Text whose box follows its longest line instead of wrapping.
    #[serde(default, skip_serializing_if = "is_false")]
    pub auto_width: bool,
    /// The part of an image that shows, in its pixels. The rest is kept, so
    /// the crop can be undone by dragging back out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crop: Option<Rect>,
}

fn is_false(value: &bool) -> bool {
    !value
}

/// Valid cumulative scales for pen strokes.
const SCALES: std::ops::RangeInclusive<f32> = 1e-4..=1e4;

impl Item {
    pub fn new(id: u64, kind: Kind, pos: Pos2, color: Color32) -> Self {
        let size = match kind {
            Kind::Note => vec2(190.0, 190.0),
            Kind::Text => vec2(320.0, 100.0),
            Kind::Frame => vec2(540.0, 380.0),
            _ => vec2(220.0, 130.0),
        };
        Self {
            id,
            kind,
            pos,
            size,
            color,
            text: if kind == Kind::Frame {
                "New frame".into()
            } else {
                String::new()
            },
            font_size: if kind == Kind::Text { 32.0 } else { 20.0 },
            stroke_width: 3.0,
            points: Vec::new(),
            start_anchor: None,
            end_anchor: None,
            image: None,
            scale: None,
            auto_width: false,
            crop: None,
        }
    }

    /// The width a line is drawn with. Pen strokes thin out when their drawing
    /// shrinks below the size it was drawn at, but never thicken when it grows.
    pub fn line_width(&self) -> f32 {
        (self.stroke_width * self.scale.unwrap_or(1.0).min(1.0)).max(0.5)
    }

    /// This pen stroke scaled by `factor` about `anchor`.
    pub fn scaled(&self, anchor: Pos2, factor: Vec2) -> Self {
        let mut item = self.clone();
        item.pos = anchor + (self.pos - anchor) * factor;
        for point in &mut item.points {
            *point = (point.to_vec2() * factor).to_pos2();
        }
        let scale = self.scale.unwrap_or(1.0) * (factor.x * factor.y).sqrt();
        item.scale = Some(scale.clamp(*SCALES.start(), *SCALES.end()));
        item
    }

    pub fn rect(&self) -> Rect {
        if matches!(self.kind, Kind::Pen | Kind::Arrow) && !self.points.is_empty() {
            let mut rect = Rect::NOTHING;
            for point in &self.points {
                rect.extend_with(self.pos + point.to_vec2());
            }
            rect
        } else {
            Rect::from_min_size(self.pos, self.size)
        }
    }

    /// Where the whole of an image lies on the board, including any part
    /// cropped away. Other objects just have their rectangle.
    pub fn uncropped(&self) -> Rect {
        match (&self.image, self.crop) {
            (Some(asset), Some(crop)) => {
                let scale = self.size / crop.size();
                Rect::from_min_size(
                    self.pos - crop.min.to_vec2() * scale,
                    vec2(asset.width as f32, asset.height as f32) * scale,
                )
            }
            _ => self.rect(),
        }
    }

    /// This image cropped to show what lies within `visible` on the board, as
    /// far as the whole image reaches. The picture itself stays in place.
    pub fn cropped(&self, visible: Rect) -> Self {
        let mut item = self.clone();
        let Some(asset) = &self.image else {
            return item;
        };
        let whole = self.uncropped();
        let visible = visible.intersect(whole);
        if visible.width() <= 0.0 || visible.height() <= 0.0 {
            return item;
        }
        let pixels = vec2(asset.width as f32, asset.height as f32);
        let to_pixels = |point: Pos2| {
            let at = (point - whole.min) / whole.size() * pixels;
            pos2(at.x.clamp(0.0, pixels.x), at.y.clamp(0.0, pixels.y))
        };
        let crop = Rect::from_min_max(to_pixels(visible.min), to_pixels(visible.max));
        item.pos = visible.min;
        item.size = visible.size();
        // Dragged all the way back out, nothing is cropped.
        item.crop = (crop.min.distance(Pos2::ZERO) > 0.01
            || crop.max.distance(pixels.to_pos2()) > 0.01)
            .then_some(crop);
        item
    }

    pub fn resize(&mut self, size: Vec2) {
        let size = size.max(vec2(40.0, 40.0));
        // A text box given a width keeps it and wraps.
        self.auto_width = false;
        if self.kind == Kind::Pen {
            let old = self.rect().size().max(vec2(1.0, 1.0));
            let origin = self.rect().min - self.pos;
            for point in &mut self.points {
                *point = pos2(
                    origin.x + (point.x - origin.x) * size.x / old.x,
                    origin.y + (point.y - origin.y) * size.y / old.y,
                );
            }
        }
        self.size = size;
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Board {
    pub title: String,
    pub items: Vec<Item>,
    pub next_id: u64,
}

impl Default for Board {
    fn default() -> Self {
        Self {
            title: "Untitled board".into(),
            items: Vec::new(),
            next_id: 1,
        }
    }
}

impl Board {
    pub fn add(&mut self, kind: Kind, pos: Pos2, color: Color32) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.items.push(Item::new(id, kind, pos, color));
        id
    }

    pub fn get(&self, id: u64) -> Option<&Item> {
        self.items.iter().find(|item| item.id == id)
    }

    pub fn get_mut(&mut self, id: u64) -> Option<&mut Item> {
        self.items.iter_mut().find(|item| item.id == id)
    }

    pub fn bounds(&self) -> Option<Rect> {
        self.items
            .iter()
            .map(|item| self.item_rect(item).expand(30.0))
            .reduce(|a, b| a.union(b))
    }

    pub fn item_rect(&self, item: &Item) -> Rect {
        if item.kind == Kind::Arrow {
            let (start, end) = self.arrow_ends(item);
            Rect::from_two_pos(start, end)
        } else {
            item.rect()
        }
    }

    pub fn arrow_ends(&self, item: &Item) -> (Pos2, Pos2) {
        connector_ends(item, |id| self.get(id))
    }

    pub fn hit_test(&self, point: Pos2, tolerance: f32, nodes_only: bool) -> Option<u64> {
        // Frames stay behind their contents, regardless of creation order.
        for frames in [false, true] {
            for item in self
                .items
                .iter()
                .rev()
                .filter(|item| (item.kind == Kind::Frame) == frames)
            {
                if nodes_only && matches!(item.kind, Kind::Arrow | Kind::Pen | Kind::Frame) {
                    continue;
                }
                let rect = item.rect();
                let hit = match item.kind {
                    Kind::Arrow => {
                        let (a, b) = self.arrow_ends(item);
                        distance_to_segment(point, a, b) <= tolerance + item.stroke_width
                    }
                    Kind::Pen => pen_segments(item).into_iter().any(|(a, b)| {
                        distance_to_segment(point, a, b) <= tolerance + item.line_width()
                    }),
                    Kind::Frame => {
                        (rect.expand(tolerance).contains(point)
                            && !rect.shrink(tolerance).contains(point))
                            || Rect::from_min_size(
                                rect.min - vec2(0.0, 30.0),
                                vec2(rect.width(), 30.0),
                            )
                            .contains(point)
                    }
                    Kind::Ellipse => {
                        let delta = (point - rect.center()) / (rect.size() * 0.5);
                        delta.length_sq() <= 1.0
                    }
                    Kind::Diamond => {
                        let delta = (point - rect.center()) / (rect.size() * 0.5);
                        delta.x.abs() + delta.y.abs() <= 1.0
                    }
                    _ => rect.contains(point),
                };
                if hit {
                    return Some(item.id);
                }
            }
        }
        None
    }

    /// Pen strokes within `radius` of an eraser swept from `from` to `to`.
    pub fn strokes_near(&self, from: Pos2, to: Pos2, radius: f32) -> HashSet<u64> {
        let swept = Rect::from_two_pos(from, to);
        self.items
            .iter()
            .filter(|item| {
                let reach = radius + item.line_width() * 0.5;
                item.kind == Kind::Pen
                    && item.rect().expand(reach).intersects(swept)
                    && pen_segments(item)
                        .into_iter()
                        .any(|(a, b)| segment_distance(from, to, a, b) <= reach)
            })
            .map(|item| item.id)
            .collect()
    }

    pub fn remove(&mut self, ids: &HashSet<u64>) {
        let detached: Vec<_> = self
            .items
            .iter()
            .filter(|item| item.kind == Kind::Arrow)
            .map(|item| (item.id, self.arrow_ends(item)))
            .collect();
        for (id, (a, b)) in detached {
            if let Some(item) = self.get_mut(id) {
                if item.start_anchor.is_some_and(|id| ids.contains(&id)) {
                    item.start_anchor = None;
                    if !item.points.is_empty() {
                        item.points[0] = (a - item.pos).to_pos2();
                    }
                }
                if item.end_anchor.is_some_and(|id| ids.contains(&id)) {
                    item.end_anchor = None;
                    if let Some(point) = item.points.last_mut() {
                        *point = (b - item.pos).to_pos2();
                    }
                }
            }
        }
        self.items.retain(|item| !ids.contains(&item.id));
    }

    pub fn paste(&mut self, items: &[Item], offset: Vec2) -> HashSet<u64> {
        let remap: HashMap<_, _> = items
            .iter()
            .map(|item| {
                let id = self.next_id;
                self.next_id += 1;
                (item.id, id)
            })
            .collect();
        let mut selected = HashSet::new();
        for original in items {
            let mut item = original.clone();
            item.id = remap[&original.id];
            item.pos += offset;
            item.start_anchor = original.start_anchor.and_then(|id| remap.get(&id).copied());
            item.end_anchor = original.end_anchor.and_then(|id| remap.get(&id).copied());
            selected.insert(item.id);
            self.items.push(item);
        }
        selected
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.items.len() > 50_000 || self.title.len() > 4096 {
            return Err("This board exceeds Romo's document limits.".into());
        }
        let mut ids = HashSet::new();
        for item in &self.items {
            let values = [
                item.pos.x,
                item.pos.y,
                item.size.x,
                item.size.y,
                item.font_size,
                item.stroke_width,
            ];
            if !values
                .iter()
                .all(|v| v.is_finite() && v.abs() < 10_000_000.0)
                || item.size.min_elem() <= 0.0
                || !(8.0..=160.0).contains(&item.font_size)
                || !(0.5..=24.0).contains(&item.stroke_width)
                || item.scale.is_some_and(|scale| !SCALES.contains(&scale))
                || item.crop.is_some_and(|crop| !valid_crop(item, crop))
                || item.text.len() > 65_536
                || item.points.len() > 100_000
                || !item
                    .points
                    .iter()
                    .all(|p| p.is_finite() && p.x.abs() < 10_000_000.0 && p.y.abs() < 10_000_000.0)
                || !ids.insert(item.id)
                || item.id == 0
                || item.id >= self.next_id
            {
                return Err("The board contains an invalid object.".into());
            }
            if matches!(item.kind, Kind::Arrow | Kind::Pen) && item.points.len() < 2 {
                return Err("The board contains an incomplete drawing.".into());
            }
            if (item.kind == Kind::Image) != item.image.is_some() {
                return Err("The board contains an image without valid image data.".into());
            }
        }
        if self.next_id == u64::MAX {
            return Err("The board's object counter is invalid.".into());
        }
        for item in &self.items {
            for anchor in [item.start_anchor, item.end_anchor].into_iter().flatten() {
                if self
                    .get(anchor)
                    .is_none_or(|node| matches!(node.kind, Kind::Arrow | Kind::Pen | Kind::Frame))
                {
                    return Err("The board contains a broken connector.".into());
                }
            }
        }
        Ok(())
    }

    pub fn welcome() -> Self {
        let mut board = Self {
            title: "A little room for big ideas".into(),
            ..Self::default()
        };
        let title = board.add(Kind::Text, pos2(-475.0, -290.0), PALETTE[7]);
        let item = board.get_mut(title).unwrap();
        item.text = "Your next big idea\nstarts here.".into();
        item.font_size = 32.0;
        item.size = vec2(740.0, 100.0);
        let sub = board.add(Kind::Text, pos2(-470.0, -190.0), PALETTE[5]);
        let item = board.get_mut(sub).unwrap();
        item.text = "A space to think, connect, and make a little sense of it all.".into();
        item.font_size = 15.0;
        item.size = vec2(800.0, 40.0);
        let frame = board.add(Kind::Frame, pos2(-490.0, -75.0), PALETTE[2]);
        let item = board.get_mut(frame).unwrap();
        item.text = "01  /  THE POSSIBILITIES".into();
        item.font_size = 13.0;
        item.size = vec2(650.0, 300.0);
        for (pos, color, text) in [
            (
                pos2(-460.0, -32.0),
                PALETTE[0],
                "What if we made\nsomething great?\n\nStart with a thought.",
            ),
            (
                pos2(-250.0, -32.0),
                PALETTE[2],
                "Let it be a little\nmessy.\n\nGood ideas take room.",
            ),
            (
                pos2(-40.0, -32.0),
                PALETTE[1],
                "Connect the dots.\n\nFind the idea hiding\nbetween the ideas.",
            ),
        ] {
            let id = board.add(Kind::Note, pos, color);
            board.get_mut(id).unwrap().text = text.into();
        }
        let next = board.add(Kind::Rectangle, pos2(300.0, 4.0), PALETTE[1]);
        let item = board.get_mut(next).unwrap();
        item.text = "Make it happen".into();
        item.size = vec2(220.0, 112.0);
        let arrow = board.add(
            Kind::Arrow,
            pos2(150.0, 60.0),
            Color32::from_rgb(113, 113, 122),
        );
        let item = board.get_mut(arrow).unwrap();
        item.points = vec![Pos2::ZERO, pos2(150.0, 0.0)];
        item.start_anchor = Some(6);
        item.end_anchor = Some(next);
        let hint = board.add(Kind::Text, pos2(285.0, 148.0), PALETTE[5]);
        let item = board.get_mut(hint).unwrap();
        item.text = "Double-click anything to edit.\nThis whole space is yours.".into();
        item.font_size = 15.0;
        item.size = vec2(260.0, 72.0);
        board
    }
}

/// Whether `crop` lies within the image of `item`, and the whole image within
/// the board's limits.
fn valid_crop(item: &Item, crop: Rect) -> bool {
    let Some(asset) = &item.image else {
        return false;
    };
    let whole = item.uncropped();
    crop.min.x >= 0.0
        && crop.min.y >= 0.0
        && crop.max.x <= asset.width as f32
        && crop.max.y <= asset.height as f32
        && crop.width() > 0.0
        && crop.height() > 0.0
        && [whole.min, whole.max]
            .iter()
            .all(|p| p.is_finite() && p.x.abs() < 10_000_000.0 && p.y.abs() < 10_000_000.0)
}

/// A connector's endpoints, finding anchored objects with `get`.
pub fn connector_ends<'a>(item: &Item, get: impl Fn(u64) -> Option<&'a Item>) -> (Pos2, Pos2) {
    let start = item.pos + item.points.first().copied().unwrap_or(Pos2::ZERO).to_vec2();
    let end = item.pos
        + item
            .points
            .last()
            .copied()
            .unwrap_or(pos2(180.0, 0.0))
            .to_vec2();
    let a = item.start_anchor.and_then(&get);
    let b = item.end_anchor.and_then(&get);
    let start_center = a.map_or(start, |node| node.rect().center());
    let end_center = b.map_or(end, |node| node.rect().center());
    (
        a.map_or(start, |node| border_point(node, end_center)),
        b.map_or(end, |node| border_point(node, start_center)),
    )
}

pub fn distance_to_segment(point: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let t = if ab.length_sq() > 0.0 {
        ((point - a).dot(ab) / ab.length_sq()).clamp(0.0, 1.0)
    } else {
        0.0
    };
    point.distance(a + ab * t)
}

fn segment_distance(a: Pos2, b: Pos2, c: Pos2, d: Pos2) -> f32 {
    let side = |o: Pos2, p: Pos2, q: Pos2| {
        let (u, v) = (p - o, q - o);
        u.x * v.y - u.y * v.x
    };
    if side(a, b, c) * side(a, b, d) < 0.0 && side(c, d, a) * side(c, d, b) < 0.0 {
        return 0.0;
    }
    distance_to_segment(a, c, d)
        .min(distance_to_segment(b, c, d))
        .min(distance_to_segment(c, a, b))
        .min(distance_to_segment(d, a, b))
}

/// Short segments along a pen stroke's smoothed curves, in board coordinates.
fn pen_segments(item: &Item) -> Vec<(Pos2, Pos2)> {
    let mut start = item.points.first().copied().unwrap_or(Pos2::ZERO);
    let mut segments = Vec::new();
    for curve in crate::stroke::curves(&item.points) {
        let mut previous = start;
        for i in 1..=8 {
            let t = i as f32 / 8.0;
            let current = start
                + (curve.control - start) * (2.0 * (1.0 - t) * t)
                + (curve.end - start) * (t * t);
            segments.push((item.pos + previous.to_vec2(), item.pos + current.to_vec2()));
            previous = current;
        }
        start = curve.end;
    }
    segments
}

fn border_point(item: &Item, target: Pos2) -> Pos2 {
    let rect = item.rect();
    let delta = target - rect.center();
    if delta.length_sq() < 0.01 {
        return rect.center();
    }
    let normalized = delta / (rect.size() * 0.5);
    let divisor = match item.kind {
        Kind::Ellipse => normalized.length(),
        Kind::Diamond => normalized.x.abs() + normalized.y.abs(),
        _ => normalized.x.abs().max(normalized.y.abs()),
    };
    rect.center() + delta / divisor
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct View {
    pub pan: Vec2,
    pub zoom: f32,
}

impl Default for View {
    fn default() -> Self {
        Self {
            pan: Vec2::ZERO,
            zoom: 1.0,
        }
    }
}

impl View {
    pub fn to_screen(self, point: Pos2, canvas: Rect) -> Pos2 {
        canvas.center() + self.pan + point.to_vec2() * self.zoom
    }

    pub fn to_world(self, point: Pos2, canvas: Rect) -> Pos2 {
        ((point - canvas.center() - self.pan) / self.zoom).to_pos2()
    }

    pub fn screen_rect(self, rect: Rect, canvas: Rect) -> Rect {
        Rect::from_min_max(
            self.to_screen(rect.min, canvas),
            self.to_screen(rect.max, canvas),
        )
    }

    pub fn zoom_at(&mut self, factor: f32, cursor: Pos2, canvas: Rect) {
        let world = self.to_world(cursor, canvas);
        self.zoom = (self.zoom * factor).clamp(0.15, 3.0);
        self.pan = cursor - canvas.center() - world.to_vec2() * self.zoom;
    }

    pub fn fit(&mut self, bounds: Option<Rect>, canvas: Rect) {
        if let Some(bounds) = bounds {
            self.zoom = ((canvas.width() - 220.0).max(100.0) / bounds.width().max(100.0))
                .min((canvas.height() - 180.0).max(100.0) / bounds.height().max(100.0))
                .clamp(0.15, 1.3);
            self.pan = -bounds.center().to_vec2() * self.zoom;
        } else {
            *self = Self::default();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zoom_keeps_the_point_under_the_cursor() {
        let canvas = Rect::from_min_size(pos2(30.0, 60.0), vec2(1200.0, 800.0));
        let cursor = pos2(180.0, 250.0);
        let mut view = View {
            pan: vec2(-120.0, 37.0),
            zoom: 0.8,
        };
        let before = view.to_world(cursor, canvas);
        view.zoom_at(1.7, cursor, canvas);
        assert!(view.to_world(cursor, canvas).distance(before) < 0.001);
        assert!(view.to_screen(before, canvas).distance(cursor) < 0.001);
    }

    #[test]
    fn connectors_follow_nodes_and_survive_node_deletion() {
        let mut board = Board::default();
        let a = board.add(Kind::Rectangle, Pos2::ZERO, PALETTE[0]);
        let b = board.add(Kind::Rectangle, pos2(400.0, 0.0), PALETTE[0]);
        let arrow = board.add(Kind::Arrow, Pos2::ZERO, PALETTE[7]);
        let item = board.get_mut(arrow).unwrap();
        item.points = vec![Pos2::ZERO, pos2(400.0, 0.0)];
        item.start_anchor = Some(a);
        item.end_anchor = Some(b);
        assert_eq!(
            board.arrow_ends(board.get(arrow).unwrap()),
            (pos2(220.0, 65.0), pos2(400.0, 65.0))
        );
        board.get_mut(b).unwrap().pos.x = 600.0;
        let ends = board.arrow_ends(board.get(arrow).unwrap());
        assert_eq!(ends.1, pos2(600.0, 65.0));
        board.remove(&HashSet::from([b]));
        assert_eq!(board.arrow_ends(board.get(arrow).unwrap()), ends);
        assert_eq!(board.get(arrow).unwrap().end_anchor, None);
        board.validate().unwrap();
    }

    #[test]
    fn copied_connectors_bind_to_the_copied_nodes() {
        let mut board = Board::welcome();
        let originals = board.items.clone();
        let copied = board.paste(&originals, vec2(1000.0, 1000.0));
        for item in board.items.iter().filter(|item| copied.contains(&item.id)) {
            for anchor in [item.start_anchor, item.end_anchor].into_iter().flatten() {
                assert!(copied.contains(&anchor));
            }
        }
        assert_eq!(copied.len(), originals.len());
        board.validate().unwrap();
    }

    #[test]
    fn frame_interiors_allow_selecting_contents_or_empty_canvas() {
        let mut board = Board::default();
        let frame = board.add(Kind::Frame, Pos2::ZERO, PALETTE[2]);
        let note = board.add(Kind::Note, pos2(50.0, 50.0), PALETTE[0]);
        assert_eq!(board.hit_test(pos2(100.0, 100.0), 5.0, false), Some(note));
        assert_eq!(board.hit_test(pos2(400.0, 200.0), 5.0, false), None);
        assert_eq!(board.hit_test(pos2(0.0, 200.0), 5.0, false), Some(frame));
    }

    #[test]
    fn eraser_sweeps_remove_whole_pen_strokes_only() {
        let mut board = Board::default();
        let stroke = board.add(Kind::Pen, Pos2::ZERO, PALETTE[7]);
        board.get_mut(stroke).unwrap().points =
            vec![Pos2::ZERO, pos2(100.0, 0.0), pos2(200.0, 0.0)];
        board.add(Kind::Note, pos2(0.0, 40.0), PALETTE[0]);
        // A fast sweep crosses the stroke between pointer samples and over the note.
        assert_eq!(
            board.strokes_near(pos2(50.0, -60.0), pos2(60.0, 120.0), 4.0),
            HashSet::from([stroke])
        );
        assert!(
            board
                .strokes_near(pos2(50.0, -60.0), pos2(60.0, -20.0), 4.0)
                .is_empty()
        );
        assert_eq!(
            board.strokes_near(pos2(150.0, -5.0), pos2(150.0, -5.0), 4.0),
            HashSet::from([stroke])
        );
    }

    #[test]
    fn scaled_strokes_thin_when_shrunk_and_never_thicken() {
        let mut stroke = Item::new(1, Kind::Pen, pos2(10.0, 10.0), PALETTE[7]);
        stroke.points = vec![Pos2::ZERO, pos2(100.0, 50.0)];
        let larger = stroke.scaled(Pos2::ZERO, vec2(2.0, 2.0));
        assert_eq!(
            larger.rect(),
            Rect::from_min_max(pos2(20.0, 20.0), pos2(220.0, 120.0))
        );
        assert_eq!(larger.line_width(), 3.0);
        let smaller = larger.scaled(Pos2::ZERO, vec2(0.25, 0.25));
        assert_eq!(smaller.line_width(), 1.5);
        // Back at the drawn size, the drawn width returns.
        assert_eq!(smaller.scaled(Pos2::ZERO, vec2(2.0, 2.0)).line_width(), 3.0);
        let mut board = Board::default();
        board.items.push(smaller);
        board.next_id = 2;
        board.validate().unwrap();
        board.items[0].scale = Some(f32::NAN);
        assert!(board.validate().is_err());
    }

    #[test]
    fn crops_keep_the_picture_in_place_and_drag_back_out_to_the_whole_image() {
        let mut board = Board::default();
        let id = board.add(Kind::Image, pos2(10.0, 20.0), PALETTE[7]);
        let image = board.get_mut(id).unwrap();
        image.image = Some(std::sync::Arc::new(crate::images::ImageAsset::sample(
            200, 100,
        )));
        // Two board units per pixel.
        image.size = vec2(400.0, 200.0);
        let image = image.clone();
        let visible = Rect::from_min_max(pos2(110.0, 20.0), pos2(410.0, 120.0));
        let cropped = image.cropped(visible);
        assert_eq!(
            cropped.crop,
            Some(Rect::from_min_max(pos2(50.0, 0.0), pos2(200.0, 50.0)))
        );
        assert_eq!(cropped.rect(), visible);
        assert_eq!(cropped.uncropped(), image.rect());
        let everything = Rect::from_min_max(pos2(-500.0, -500.0), pos2(1000.0, 1000.0));
        assert_eq!(cropped.cropped(everything), image);
        board.items[0] = cropped;
        board.validate().unwrap();
        board.items[0].crop = Some(Rect::from_min_max(pos2(50.0, 0.0), pos2(201.0, 50.0)));
        assert!(board.validate().is_err());
        board.items[0].crop = Some(Rect::from_min_max(pos2(50.0, 0.0), pos2(50.0, 50.0)));
        assert!(board.validate().is_err());
    }

    #[test]
    fn invalid_geometry_and_duplicate_ids_are_rejected() {
        let mut board = Board::welcome();
        board.validate().unwrap();
        board.items[0].pos.x = f32::NAN;
        assert!(board.validate().is_err());
        let mut board = Board::welcome();
        board.items[1].id = board.items[0].id;
        assert!(board.validate().is_err());
    }
}
