use crate::{
    geometry::{Corner, Pos2, Rect, pos2, vec2},
    images::ImageAsset,
    model::{Board, Item, Kind, PointsId, View, connector_ends},
};
use gpui::{
    App, AssetSource, Bounds, ContentMask, PathBuilder, Pixels, SharedString, TextRun,
    TransformationMatrix, Window, WrappedLine, fill, point, px, quad, rgb, size,
};
use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
    f32::consts::TAU,
    hash::{Hash, Hasher},
    time::{Duration, Instant},
};

pub const BASE: u32 = 0x09090b;
pub const SURFACE: u32 = 0x18181b;
pub const BORDER: u32 = 0x27272a;
pub const MUTED: u32 = 0xa1a1aa;
pub const FG: u32 = 0xfafafa;

const HAND: &str = "icons/hand.svg";
const LOGO: &str = "icons/logo.svg";

/// Bundled SVG icons. GPUI paints them as single-color masks.
pub struct Assets;
impl AssetSource for Assets {
    fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
        let bytes: &'static [u8] = match path {
            HAND => include_bytes!("../assets/icons/hand.svg"),
            LOGO => include_bytes!("../assets/icons/logo.svg"),
            _ => return Ok(None),
        };
        Ok(Some(Cow::Borrowed(bytes)))
    }
    fn list(&self, _: &str) -> gpui::Result<Vec<SharedString>> {
        Ok(Vec::new())
    }
}
pub fn p(p: Pos2) -> gpui::Point<Pixels> {
    point(px(p.x), px(p.y))
}
pub fn bounds(r: Rect) -> Bounds<Pixels> {
    Bounds::new(
        p(r.min),
        size(px(r.width().max(0.1)), px(r.height().max(0.1))),
    )
}
pub fn rect(window: &mut Window, r: Rect, color: u32, border: Option<u32>, radius: f32) {
    if let Some(border) = border {
        window.paint_quad(quad(
            bounds(r),
            px(radius),
            rgb(color),
            px(1.0),
            rgb(border),
            gpui::BorderStyle::Solid,
        ));
    } else {
        window.paint_quad(fill(bounds(r), rgb(color)).corner_radii(px(radius)));
    }
}
pub fn path(
    window: &mut Window,
    points: &[Pos2],
    color: u32,
    width: f32,
    closed: bool,
    filled: bool,
) {
    if points.len() < 2 {
        return;
    }
    let mut builder = if filled {
        PathBuilder::fill()
    } else {
        PathBuilder::stroke(px(width))
    };
    builder.move_to(p(points[0]));
    for point in &points[1..] {
        builder.line_to(p(*point));
    }
    if closed {
        builder.close();
    }
    if let Ok(path) = builder.build() {
        window.paint_path(path, rgb(color));
    }
}
/// Text smaller than this on screen is unreadable, so it is drawn as soft bars.
const GREEK_BELOW: f32 = 5.0;
/// Text is shaped once at this size and painted at any size by scaling glyph
/// positions, so zooming never reshapes it.
const SHAPED_AT: f32 = 16.0;
/// Time per frame for shaping newly visible text and refining strokes after
/// the zoom changes detail. Work past it waits for later frames.
const TEXT_BUDGET: Duration = Duration::from_millis(4);
const STROKE_BUDGET: Duration = Duration::from_millis(3);

#[derive(Clone, Copy)]
pub struct TextStyle {
    /// Size on screen, in pixels.
    pub font: f32,
    pub color: u32,
    pub centered: bool,
}

/// Paint `text` in `r`, reusing shaped text from `cache` when given.
pub fn text(window: &mut Window, cache: Option<&mut Cache>, text: &str, r: Rect, style: TextStyle) {
    if text.is_empty() {
        return;
    }
    if style.font < GREEK_BELOW {
        return greek(window, text, r, style);
    }
    // Wrap at a width measured in ems, so line breaks are the same at every size.
    let wrap = r.width().max(10.0) / style.font;
    let shape = |window: &mut Window| -> Vec<WrappedLine> {
        let run = TextRun {
            len: text.len(),
            font: gpui::font("Inter Variable"),
            color: rgb(style.color).into(),
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        window
            .text_system()
            .shape_text(
                text.to_owned().into(),
                px(SHAPED_AT),
                &[run],
                Some(px(wrap * SHAPED_AT)),
                None,
            )
            .map(|lines| lines.into_iter().collect())
            .unwrap_or_default()
    };
    let Some(cache) = cache else {
        let lines = shape(window);
        return paint_lines(window, &lines, r, style, true);
    };
    let mut hasher = std::hash::DefaultHasher::new();
    (text, style.color, (wrap * 256.0).round() as i64).hash(&mut hasher);
    let key = hasher.finish();
    if !cache.texts.contains_key(&key) && cache.text_time < TEXT_BUDGET {
        let started = Instant::now();
        let lines = shape(window);
        cache.text_time += started.elapsed();
        cache.texts.insert(key, CachedText { lines, used: 0 });
    }
    let (frame, exact) = (cache.frame, cache.exact_glyphs);
    match cache.texts.get_mut(&key) {
        Some(cached) => {
            cached.used = frame;
            paint_lines(window, &cached.lines, r, style, exact);
        }
        // Soft bars until the text's turn to be shaped comes.
        None => greek(window, text, r, style),
    }
}

/// Paint lines shaped at `SHAPED_AT` at the style's size, scaling glyph positions
/// the way GPUI places them.
fn paint_lines(window: &mut Window, lines: &[WrappedLine], r: Rect, style: TextStyle, exact: bool) {
    let TextStyle {
        font,
        color,
        centered,
    } = style;
    let scale = font / SHAPED_AT;
    // While zooming, glyph images come in 24 sizes per doubling, which the zoom
    // reuses. Settled text uses its exact size, matching the text editor.
    let glyph_size = px(if exact {
        font
    } else {
        2f32.powf((font.log2() * 24.0).round() / 24.0)
    });
    let color = gpui::Hsla::from(rgb(color));
    let line_height = font * 1.35;
    let rows: usize = lines.iter().map(|l| l.wrap_boundaries.len() + 1).sum();
    let mut top = if centered {
        r.center().y - rows as f32 * line_height * 0.5
    } else {
        r.min.y
    };
    let visible = window.content_mask().bounds;
    let (above, below) = (f32::from(visible.top()), f32::from(visible.bottom()));
    // One layer for every glyph spares GPUI ordering each glyph separately.
    window.paint_layer(bounds(r), |window| {
        for line in lines {
            let layout = &line.unwrapped_layout;
            let (ascent, descent) = (f32::from(layout.ascent), f32::from(layout.descent));
            let baseline = (line_height - (ascent + descent) * scale) * 0.5 + ascent * scale;
            // Each wrap boundary starts a row at that glyph's shaped position.
            let mut starts = vec![0.0];
            for boundary in &line.wrap_boundaries {
                let glyph = &layout.runs[boundary.run_ix].glyphs[boundary.glyph_ix];
                starts.push(f32::from(glyph.position.x));
            }
            let origins: Vec<f32> = starts
                .iter()
                .enumerate()
                .map(|(row, start)| {
                    let end = starts
                        .get(row + 1)
                        .copied()
                        .unwrap_or(f32::from(layout.width));
                    if centered {
                        r.center().x - (end - start) * scale * 0.5
                    } else {
                        r.min.x
                    }
                })
                .collect();
            let mut wraps = line.wrap_boundaries.iter().peekable();
            let mut row = 0;
            for (run_ix, run) in layout.runs.iter().enumerate() {
                for (glyph_ix, glyph) in run.glyphs.iter().enumerate() {
                    if wraps
                        .next_if(|b| b.run_ix == run_ix && b.glyph_ix == glyph_ix)
                        .is_some()
                    {
                        row += 1;
                    }
                    let y = top + row as f32 * line_height;
                    if y > below || y + line_height < above {
                        continue;
                    }
                    let x = origins[row] + (f32::from(glyph.position.x) - starts[row]) * scale;
                    let origin = point(px(x), px(y + baseline));
                    let _ = if glyph.is_emoji {
                        window.paint_emoji(origin, run.font_id, glyph.id, glyph_size)
                    } else {
                        window.paint_glyph(origin, run.font_id, glyph.id, glyph_size, color)
                    };
                }
            }
            top += starts.len() as f32 * line_height;
        }
    });
}

/// Soft bars in place of text too small to read, skipping shaping and glyphs.
fn greek(window: &mut Window, text: &str, r: Rect, style: TextStyle) {
    let TextStyle {
        font,
        color,
        centered,
    } = style;
    let line_height = font * 1.35;
    let advance = font * 0.55;
    let columns = ((r.width() / advance) as usize).max(1);
    let mut widths = Vec::new();
    for paragraph in text.split('\n') {
        let mut remaining = paragraph.chars().count();
        if remaining == 0 {
            widths.push(0.0);
        }
        while remaining > 0 && widths.len() < 256 {
            let line = remaining.min(columns);
            widths.push(line as f32 * advance);
            remaining -= line;
        }
    }
    let thickness = (font * 0.5).max(0.5);
    let mut tint = rgb(color);
    tint.a *= 0.5;
    let mut y = if centered {
        r.center().y - widths.len() as f32 * line_height * 0.5
    } else {
        r.min.y
    };
    window.paint_layer(bounds(r), |window| {
        for width in widths {
            if y > r.max.y {
                break;
            }
            if width > 0.0 {
                let x = if centered {
                    r.center().x - width * 0.5
                } else {
                    r.min.x
                };
                let bar = Rect::from_min_size(
                    pos2(x, y + (line_height - thickness) * 0.5),
                    vec2(width, thickness),
                );
                window.paint_quad(fill(bounds(bar), tint).corner_radii(px(thickness * 0.5)));
            }
            y += line_height;
        }
    });
}

/// A soft ribbon behind the eraser that narrows and fades toward its oldest
/// point, one translucent path so its overlaps don't darken.
fn trail(window: &mut Window, points: &[(Pos2, f32)], radius: f32) {
    let [.., (before, _), (head, life)] = points else {
        return;
    };
    let mut outline = Vec::with_capacity(points.len() * 2 + 8);
    let mut back = Vec::with_capacity(points.len());
    for (index, &(point, life)) in points.iter().enumerate() {
        let previous = points[index.saturating_sub(1)].0;
        let next = points[(index + 1).min(points.len() - 1)].0;
        let along = (next - previous).normalized();
        let across = vec2(-along.y, along.x) * radius * life;
        outline.push(point + across);
        back.push(point - across);
    }
    // A round cap leads the ribbon.
    let along = (*head - *before).normalized();
    let across = vec2(-along.y, along.x);
    for step in 1..8 {
        let angle = step as f32 / 8.0 * std::f32::consts::PI;
        outline.push(*head + (across * angle.cos() + along * angle.sin()) * radius * *life);
    }
    outline.extend(back.into_iter().rev());
    let mut builder =
        PathBuilder::fill().with_style(gpui::PathStyle::Fill(gpui::FillOptions::non_zero()));
    builder.move_to(p(outline[0]));
    for point in &outline[1..] {
        builder.line_to(p(*point));
    }
    builder.close();
    let mut tint = rgb(FG);
    tint.a = 0.24;
    if let Ok(path) = builder.build() {
        window.paint_path(path, tint);
    }
}

/// A 1 px rectangle outline, as one quad.
pub fn outline(window: &mut Window, r: Rect, color: u32) {
    window.paint_quad(quad(
        bounds(r),
        px(0.0),
        gpui::transparent_black(),
        px(1.0),
        rgb(color),
        gpui::BorderStyle::Solid,
    ));
}

/// `r` with its edges moved onto whole device pixels.
fn pixel_aligned(window: &Window, r: Rect) -> Rect {
    let scale = window.scale_factor();
    let snap = |v: f32| (v * scale).round() / scale;
    Rect::from_min_max(
        pos2(snap(r.min.x), snap(r.min.y)),
        pos2(snap(r.max.x), snap(r.max.y)),
    )
}

/// An outline that stays sharp while the board pans or zooms. GPUI
/// antialiases a line at a fractional position across two pixels, which
/// shimmers as it moves, so this one sits on whole device pixels.
fn crisp_outline(window: &mut Window, r: Rect, color: u32) {
    let scale = window.scale_factor();
    window.paint_quad(quad(
        bounds(pixel_aligned(window, r)),
        px(0.0),
        gpui::transparent_black(),
        px(scale.round().max(1.0) / scale),
        rgb(color),
        gpui::BorderStyle::Solid,
    ));
}

pub struct Overlay<'a> {
    pub selected: &'a HashSet<u64>,
    /// Bounds with resize or scale handles, and the corners that have them.
    pub handles: Option<(Rect, &'a [Corner])>,
    /// The image being cropped, shown faintly in full around its crop.
    pub cropping: Option<u64>,
    pub grid: bool,
    pub marquee: Option<Rect>,
    pub editing: Option<u64>,
    /// The object being dragged out, which shows no placeholder yet.
    pub creating: Option<u64>,
    pub guides: &'a [crate::snapping::Guide],
    pub empty_hint: bool,
    /// Eraser outline center and radius, in screen pixels.
    pub eraser: Option<(Pos2, f32)>,
    /// Recent eraser positions on screen, oldest first, with the life left in each (1 to 0).
    pub trail: &'a [(Pos2, f32)],
    /// The zoom has stopped changing, so text renders at exact sizes.
    pub settled: bool,
}

/// Work kept between frames: pen strokes tessellated in board coordinates, so
/// panning and zooming only transform their triangles, and shaped text.
#[derive(Default)]
pub struct Cache {
    strokes: HashMap<u64, CachedStroke>,
    texts: HashMap<u64, CachedText>,
    stroke_time: Duration,
    text_time: Duration,
    frame: u64,
    /// The zoom has settled, so glyphs render at exact sizes.
    exact_glyphs: bool,
}
struct CachedStroke {
    /// The points and line width the triangles were made from.
    points: PointsId,
    width: f32,
    level: i32,
    triangles: Vec<[Pos2; 3]>,
}
struct CachedText {
    lines: Vec<WrappedLine>,
    used: u64,
}
impl Cache {
    fn begin_frame(&mut self, settled: bool) {
        self.stroke_time = Duration::ZERO;
        self.text_time = Duration::ZERO;
        self.exact_glyphs = settled;
        self.frame += 1;
        // Forget text that has been off screen for a few seconds.
        if self.frame.is_multiple_of(120) {
            let frame = self.frame;
            self.texts.retain(|_, text| frame - text.used < 240);
        }
    }
    /// Triangles for a stroke in its local coordinates, detailed for `zoom`.
    /// After the zoom crosses an octave, strokes refine within a per-frame
    /// budget and briefly keep their previous detail.
    fn triangles(&mut self, item: &Item, zoom: f32) -> &[[Pos2; 3]] {
        let level = zoom.log2().floor() as i32;
        let width = item.line_width();
        let current = self.strokes.get(&item.id).is_some_and(|cached| {
            cached.points.is(&item.points)
                && cached.width == width
                && (cached.level == level || self.stroke_time >= STROKE_BUDGET)
        });
        if !current {
            let started = Instant::now();
            let triangles = tessellate(&item.points, width, level);
            self.stroke_time += started.elapsed();
            self.strokes.insert(
                item.id,
                CachedStroke {
                    points: item.points.id(),
                    width,
                    level,
                    triangles,
                },
            );
        }
        &self.strokes[&item.id].triangles
    }
}

/// Tessellate a smoothed stroke at the detail of zoom level `2^level`.
fn tessellate(points: &[Pos2], width: f32, level: i32) -> Vec<[Pos2; 3]> {
    use lyon::tessellation::{
        BuffersBuilder, LineCap, LineJoin, StrokeOptions, StrokeTessellator, StrokeVertex,
        VertexBuffers,
    };
    let scale = 2f32.powi(level);
    // Samples closer than about a pixel add no detail. Keep the exact pen tip.
    let mut kept: Vec<Pos2> = Vec::with_capacity(points.len());
    for &point in points {
        if kept
            .last()
            .is_none_or(|last| last.distance(point) >= 0.75 / scale)
        {
            kept.push(point);
        }
    }
    if let Some(&tip) = points.last()
        && kept.last() != Some(&tip)
    {
        kept.push(tip);
    }
    let Some(&start) = kept.first() else {
        return Vec::new();
    };
    let to = |point: Pos2| lyon::math::point(point.x, point.y);
    let mut builder = lyon::path::Path::builder();
    builder.begin(to(start));
    for curve in crate::stroke::curves(&kept) {
        builder.quadratic_bezier_to(to(curve.control), to(curve.end));
    }
    builder.end(false);
    // Below about two pixels on screen, round caps and joins look no different
    // from square ones and bevels, and take many more triangles.
    let (cap, join) = if width * scale < 1.0 {
        (LineCap::Square, LineJoin::Bevel)
    } else {
        (LineCap::Round, LineJoin::Round)
    };
    let options = StrokeOptions::default()
        .with_line_width(width)
        .with_line_cap(cap)
        .with_line_join(join)
        .with_tolerance(0.1 / scale);
    let mut buffers: VertexBuffers<lyon::math::Point, u32> = VertexBuffers::new();
    let mut output = BuffersBuilder::new(&mut buffers, |vertex: StrokeVertex| vertex.position());
    if StrokeTessellator::new()
        .tessellate_path(&builder.build(), &options, &mut output)
        .is_err()
    {
        return Vec::new();
    }
    let vertex = |index: u32| {
        let point = buffers.vertices[index as usize];
        pos2(point.x, point.y)
    };
    let (triangles, _) = buffers.indices.as_chunks::<3>();
    triangles
        .iter()
        .map(|triangle| triangle.map(vertex))
        .collect()
}

/// Dots every 40 board units, doubling the spacing until they are at least
/// 24 px apart. One layer keeps thousands of dots cheap to order.
fn grid(window: &mut Window, view: View, canvas: Rect) {
    let mut spacing = 40.0 * view.zoom;
    while spacing < 24.0 {
        spacing *= 2.0;
    }
    let origin = view.to_screen(Pos2::ZERO, canvas);
    let color = rgb(BORDER);
    window.paint_layer(bounds(canvas), |window| {
        let mut x = canvas.min.x + (origin.x - canvas.min.x).rem_euclid(spacing);
        while x < canvas.max.x {
            let mut y = canvas.min.y + (origin.y - canvas.min.y).rem_euclid(spacing);
            while y < canvas.max.y {
                let dot = Rect::from_center_size(pos2(x, y), vec2(1.5, 1.5));
                window.paint_quad(fill(bounds(dot), color).corner_radii(px(1.0)));
                y += spacing;
            }
            x += spacing;
        }
    });
}

/// What every item needs to paint itself.
struct Painter<'a> {
    view: View,
    canvas: Rect,
    ends: &'a dyn Fn(&Item) -> (Pos2, Pos2),
    cache: &'a mut Cache,
    /// Pen strokes of one color painted in a row, gathered into one path:
    /// GPUI handles each path separately, which costs more than its triangles.
    strokes: Option<(gpui::Path<Pixels>, u32)>,
}

impl Painter<'_> {
    /// Paint the gathered strokes, before anything that may cover them.
    fn flush(&mut self, window: &mut Window) {
        if let Some((path, color)) = self.strokes.take() {
            window.paint_path(path, rgb(color));
        }
    }
}

pub fn board(
    window: &mut Window,
    board: &Board,
    view: View,
    canvas: Rect,
    overlay: Overlay<'_>,
    cache: &mut Cache,
) {
    let Overlay {
        selected,
        handles,
        cropping,
        grid: show_grid,
        marquee,
        editing,
        creating,
        guides,
        empty_hint,
        eraser,
        trail: eraser_trail,
        settled,
    } = overlay;
    cache.begin_frame(settled);
    window.with_content_mask(
        Some(ContentMask {
            bounds: bounds(canvas),
        }),
        |window| {
            if show_grid {
                grid(window, view, canvas);
            }
            let index: HashMap<u64, &Item> =
                board.items.iter().map(|item| (item.id, item)).collect();
            let ends = |item: &Item| connector_ends(item, |id| index.get(&id).copied());
            let item_rect = |item: &Item| {
                if item.kind == Kind::Arrow {
                    let (a, b) = ends(item);
                    Rect::from_two_pos(a, b)
                } else {
                    item.rect()
                }
            };
            // Frames stay behind connectors, and connectors behind everything else.
            let mut layers: [Vec<(&Item, Rect)>; 3] = Default::default();
            for item in &board.items {
                let screen = view.screen_rect(item_rect(item), canvas);
                if screen.expand(50.0).intersects(canvas) {
                    let layer = match item.kind {
                        Kind::Frame => 0,
                        Kind::Arrow => 1,
                        _ => 2,
                    };
                    layers[layer].push((item, screen));
                }
            }
            let mut painter = Painter {
                view,
                canvas,
                ends: &ends,
                cache,
                strokes: None,
            };
            for &(item, screen) in layers.iter().flatten() {
                paint_item(
                    window,
                    &mut painter,
                    item,
                    screen,
                    editing == Some(item.id),
                    creating == Some(item.id),
                );
            }
            painter.flush(window);
            painter.cache.strokes.retain(|id, _| index.contains_key(id));
            // While cropping, the rest of the picture shows dimmed around the crop.
            if let Some(item) = cropping.and_then(|id| index.get(&id))
                && let Some(asset) = &item.image
            {
                let whole = view.screen_rect(item.uncropped(), canvas);
                picture(window, asset, whole, whole);
                let mut veil = rgb(BASE);
                veil.a = 0.75;
                window.paint_quad(fill(bounds(whole), veil));
                outline(window, whole, BORDER);
                picture(window, asset, whole, view.screen_rect(item.rect(), canvas));
            }
            if board.items.is_empty() && empty_hint {
                text(
                    window,
                    None,
                    "Room for a new idea.",
                    Rect::from_center_size(canvas.center() - vec2(0.0, 28.0), vec2(500.0, 50.0)),
                    TextStyle {
                        font: 32.0,
                        color: FG,
                        centered: true,
                    },
                );
                text(
                    window,
                    None,
                    "Choose a tool on the left, or press N to add a note.",
                    Rect::from_center_size(canvas.center() + vec2(0.0, 28.0), vec2(500.0, 32.0)),
                    TextStyle {
                        font: 15.0,
                        color: MUTED,
                        centered: true,
                    },
                );
            }
            // Several objects share one outline around their combined bounds.
            if let Some(r) = board
                .items
                .iter()
                .filter(|item| selected.contains(&item.id))
                .map(item_rect)
                .reduce(|a, b| a.union(b))
            {
                crisp_outline(window, view.screen_rect(r, canvas).expand(4.0), FG);
            }
            if let Some((r, corners)) = handles {
                // Handles sit on the corners of the selection outline.
                let around = view.screen_rect(r, canvas).expand(4.0);
                for corner in corners {
                    let handle = Rect::from_center_size(corner.of(around), vec2(8.0, 8.0));
                    rect(window, pixel_aligned(window, handle), FG, None, 1.0);
                }
            }
            for guide in guides {
                let line = Rect::from_two_pos(
                    view.to_screen(guide.start, canvas),
                    view.to_screen(guide.end, canvas),
                );
                window.paint_quad(fill(bounds(line.expand(0.5)), rgb(MUTED)));
            }
            if let Some(r) = marquee {
                let mut tint = rgb(FG);
                tint.a = 0.04;
                window.paint_quad(quad(
                    bounds(view.screen_rect(r, canvas)),
                    px(0.0),
                    tint,
                    px(1.0),
                    rgb(FG),
                    gpui::BorderStyle::Solid,
                ));
            }
            trail(
                window,
                eraser_trail,
                eraser.map_or(10.0, |(_, radius)| radius),
            );
            if let Some((center, radius)) = eraser {
                let mut tint = rgb(FG);
                tint.a = 0.08;
                window.paint_quad(quad(
                    bounds(Rect::from_center_size(center, vec2(radius, radius) * 2.0)),
                    px(radius),
                    tint,
                    px(1.0),
                    rgb(MUTED),
                    gpui::BorderStyle::Solid,
                ));
            }
        },
    );
}

fn paint_item(
    window: &mut Window,
    painter: &mut Painter<'_>,
    item: &Item,
    r: Rect,
    editing: bool,
    creating: bool,
) {
    let view = painter.view;
    let color = item.color.rgb();
    if item.kind != Kind::Pen {
        painter.flush(window);
    }
    match item.kind {
        Kind::Image => {
            if let Some(asset) = &item.image {
                let whole = view.screen_rect(item.uncropped(), painter.canvas);
                picture(window, asset, whole, r);
            }
        }
        Kind::Note | Kind::Rectangle => rect(window, r, color, Some(0x3f3f46), 4.0),
        Kind::Frame => outline(window, r, color),
        Kind::Ellipse | Kind::Diamond => shape(window, item.kind, r, color),
        Kind::Pen => stroke(window, painter, item, r),
        Kind::Arrow => {
            let (a, b) = (painter.ends)(item);
            let (a, b) = (
                view.to_screen(a, painter.canvas),
                view.to_screen(b, painter.canvas),
            );
            // The line and its head share one path.
            let mut builder = PathBuilder::stroke(px(item.stroke_width * view.zoom));
            builder.move_to(p(a));
            builder.line_to(p(b));
            let dir = (b - a).normalized();
            let n = vec2(-dir.y, dir.x);
            let l = (12.0 * view.zoom).max(4.0);
            builder.move_to(p(b - dir * l + n * l * 0.45));
            builder.line_to(p(b));
            builder.line_to(p(b - dir * l - n * l * 0.45));
            if let Ok(path) = builder.build() {
                window.paint_path(path, rgb(color));
            }
        }
        Kind::Text => {}
    }
    if editing || matches!(item.kind, Kind::Arrow | Kind::Pen | Kind::Image) {
        return;
    }
    let placeholder = if creating {
        ""
    } else if item.kind == Kind::Note {
        "Write a thought…"
    } else if item.kind == Kind::Text {
        "Add your text"
    } else {
        ""
    };
    let content = if item.text.is_empty() {
        placeholder
    } else {
        &item.text
    };
    let luminance = item.color.r() as f32 * 0.299
        + item.color.g() as f32 * 0.587
        + item.color.b() as f32 * 0.114;
    let text_color = if item.text.is_empty() {
        MUTED
    } else if matches!(item.kind, Kind::Text | Kind::Frame) {
        color
    } else if luminance > 140.0 {
        SURFACE
    } else {
        FG
    };
    let font = if item.kind == Kind::Frame {
        13.0
    } else {
        item.font_size
    } * view.zoom;
    let style = TextStyle {
        font,
        color: text_color,
        centered: !matches!(item.kind, Kind::Text | Kind::Frame),
    };
    let cache = Some(&mut *painter.cache);
    if item.kind == Kind::Frame {
        let label = Rect::from_min_size(
            r.min - vec2(0.0, 30.0 * view.zoom),
            vec2(r.width(), 24.0 * view.zoom),
        );
        text(window, cache, content, label, style);
    } else {
        let (text_rect, clip) = match item.kind {
            // Auto-width text already fits its box, so it never wraps or clips.
            Kind::Text if item.auto_width => (
                Rect::from_min_size(r.min, vec2(r.width() + font * 4.0, r.height())),
                r.expand(font * 0.5),
            ),
            Kind::Text => (r, r),
            _ => (r.shrink(16.0 * view.zoom), r),
        };
        window.with_content_mask(
            Some(ContentMask {
                bounds: bounds(clip),
            }),
            |window| text(window, cache, content, text_rect, style),
        );
    }
}

/// The part of an image within `shown`, with the whole picture at `whole`, on screen.
fn picture(window: &mut Window, asset: &ImageAsset, whole: Rect, shown: Rect) {
    window.with_content_mask(
        Some(ContentMask {
            bounds: bounds(shown),
        }),
        |window| {
            let _ = window.paint_image(
                bounds(whole),
                gpui::Corners::default(),
                asset.rendered.clone(),
                0,
                false,
            );
        },
    );
}

/// Ellipses and diamonds, simplified when small on screen.
fn shape(window: &mut Window, kind: Kind, r: Rect, color: u32) {
    let extent = r.width().max(r.height());
    let round = kind == Kind::Ellipse;
    if extent < 3.0 {
        return rect(
            window,
            r,
            color,
            None,
            if round { extent * 0.5 } else { 0.0 },
        );
    }
    // A circle is one rounded quad with its outline.
    if round && (r.width() - r.height()).abs() < 0.5 {
        return rect(window, r, color, Some(0x3f3f46), r.width() * 0.5);
    }
    let points: Vec<Pos2> = if round {
        let segments = ((r.width() + r.height()) * 0.25).clamp(12.0, 64.0) as usize;
        (0..segments)
            .map(|i| {
                let a = i as f32 / segments as f32 * TAU;
                r.center() + vec2(a.cos() * r.width() * 0.5, a.sin() * r.height() * 0.5)
            })
            .collect()
    } else {
        vec![
            r.center_top(),
            r.right_center(),
            r.center_bottom(),
            r.left_center(),
        ]
    };
    path(window, &points, color, 1.0, true, true);
    // A 1 px outline is invisible on tiny shapes.
    if r.width().min(r.height()) >= 8.0 {
        path(window, &points, 0x3f3f46, 1.0, true, false);
    }
}

/// A pen stroke from cached triangles. Taps and strokes too small to show
/// detail are round dots.
fn stroke(window: &mut Window, painter: &mut Painter<'_>, item: &Item, r: Rect) {
    let view = painter.view;
    let color = item.color.rgb();
    if r.width().max(r.height()) < 1.5 {
        painter.flush(window);
        let size = (item.line_width() * view.zoom).max(1.0);
        let dot = Rect::from_center_size(r.center(), vec2(size, size));
        window.paint_quad(fill(bounds(dot), rgb(color)).corner_radii(px(size * 0.5)));
        return;
    }
    let origin = painter.canvas.center() + view.pan + item.pos.to_vec2() * view.zoom;
    let at = |local: Pos2| p(origin + local.to_vec2() * view.zoom);
    if painter
        .strokes
        .as_ref()
        .is_some_and(|(_, other)| *other != color)
    {
        painter.flush(window);
    }
    let triangles = painter.cache.triangles(item, view.zoom);
    let Some(first) = triangles.first() else {
        return;
    };
    let solid = (point(0.0, 1.0), point(0.0, 1.0), point(0.0, 1.0));
    let (path, _) = painter
        .strokes
        .get_or_insert_with(|| (gpui::Path::new(at(first[0])), color));
    for [a, b, c] in triangles {
        path.push_triangle((at(*a), at(*b), at(*c)), solid);
    }
}

#[derive(Clone, Copy)]
pub enum Icon {
    Settings,
    Image,
    Palette,
    Select,
    Hand,
    Note,
    Text,
    Rectangle,
    Ellipse,
    Diamond,
    Arrow,
    Pen,
    Frame,
    Undo,
    Redo,
    Minus,
    Plus,
    Fit,
    Grid,
    Help,
    Trash,
    Copy,
    Map,
    Close,
    Front,
    Back,
    Eraser,
    Save,
    Menu,
    Chevron,
    Logo,
}
pub fn icon(window: &mut Window, cx: &App, r: Rect, icon: Icon, color: u32) {
    let c = r.center();
    let p = |x, y| c + vec2(x, y);
    let mut paths: Vec<Vec<Pos2>> = Vec::new();
    match icon {
        Icon::Hand | Icon::Logo => {
            let _ = window.paint_svg(
                bounds(r),
                if matches!(icon, Icon::Hand) {
                    HAND
                } else {
                    LOGO
                }
                .into(),
                TransformationMatrix::unit(),
                rgb(color).into(),
                cx,
            );
        }
        Icon::Settings => {
            for y in [-5.0, 0.0, 5.0] {
                paths.push(vec![p(-8.0, y), p(8.0, y)]);
            }
            for (x, y) in [(-3.0, -5.0), (4.0, 0.0), (-1.0, 5.0)] {
                paths.push(vec![p(x, y - 2.0), p(x, y + 2.0)]);
            }
        }
        Icon::Image => {
            paths.push(vec![
                p(-8.0, -7.0),
                p(8.0, -7.0),
                p(8.0, 7.0),
                p(-8.0, 7.0),
                p(-8.0, -7.0),
            ]);
            paths.push(vec![
                p(-7.0, 5.0),
                p(-2.0, 0.0),
                p(2.0, 4.0),
                p(5.0, 1.0),
                p(8.0, 5.0),
            ]);
            paths.push(vec![p(3.0, -3.0), p(4.0, -3.0)]);
        }
        Icon::Palette => {
            paths.push(vec![
                p(0.0, -8.0),
                p(8.0, 0.0),
                p(0.0, 8.0),
                p(-8.0, 0.0),
                p(0.0, -8.0),
            ]);
            paths.push(vec![p(-5.0, 3.0), p(5.0, 3.0)]);
        }
        Icon::Select => paths.push(vec![
            p(-6.0, -8.0),
            p(7.0, 1.0),
            p(1.0, 2.0),
            p(-2.0, 8.0),
            p(-6.0, -8.0),
        ]),
        Icon::Note => {
            paths.push(vec![
                p(-7.0, -7.0),
                p(7.0, -7.0),
                p(7.0, 2.0),
                p(2.0, 7.0),
                p(-7.0, 7.0),
                p(-7.0, -7.0),
            ]);
            paths.push(vec![p(7.0, 2.0), p(2.0, 2.0), p(2.0, 7.0)]);
        }
        Icon::Text => {
            paths.push(vec![p(-7.0, -7.0), p(7.0, -7.0)]);
            paths.push(vec![p(0.0, -7.0), p(0.0, 7.0)]);
            paths.push(vec![p(-4.0, 7.0), p(4.0, 7.0)]);
        }
        Icon::Rectangle => paths.push(vec![
            p(-7.0, -6.0),
            p(7.0, -6.0),
            p(7.0, 6.0),
            p(-7.0, 6.0),
            p(-7.0, -6.0),
        ]),
        Icon::Ellipse => paths.push(
            (0..=32)
                .map(|i| {
                    let a = i as f32 / 32.0 * std::f32::consts::TAU;
                    p(a.cos() * 7.0, a.sin() * 7.0)
                })
                .collect(),
        ),
        Icon::Diamond => paths.push(vec![
            p(0.0, -8.0),
            p(8.0, 0.0),
            p(0.0, 8.0),
            p(-8.0, 0.0),
            p(0.0, -8.0),
        ]),
        Icon::Arrow => {
            paths.push(vec![p(-7.0, 6.0), p(7.0, -6.0)]);
            paths.push(vec![p(0.0, -6.0), p(7.0, -6.0), p(7.0, 1.0)]);
        }
        Icon::Pen => paths.push(vec![
            p(-7.0, 7.0),
            p(-5.0, 0.0),
            p(3.0, -8.0),
            p(8.0, -3.0),
            p(0.0, 5.0),
            p(-7.0, 7.0),
        ]),
        Icon::Frame | Icon::Fit => {
            for (x, y) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
                paths.push(vec![
                    p(x * 3.0, y * 7.0),
                    p(x * 7.0, y * 7.0),
                    p(x * 7.0, y * 3.0),
                ]);
            }
        }
        Icon::Undo | Icon::Redo => {
            let s = if matches!(icon, Icon::Undo) {
                1.0
            } else {
                -1.0
            };
            paths.push(vec![
                p(-7.0 * s, 0.0),
                p(-3.0 * s, -5.0),
                p(4.0 * s, -5.0),
                p(7.0 * s, 0.0),
                p(5.0 * s, 6.0),
            ]);
            paths.push(vec![p(-7.0 * s, -5.0), p(-7.0 * s, 0.0), p(-2.0 * s, 0.0)]);
        }
        Icon::Minus | Icon::Plus => {
            paths.push(vec![p(-6.0, 0.0), p(6.0, 0.0)]);
            if matches!(icon, Icon::Plus) {
                paths.push(vec![p(0.0, -6.0), p(0.0, 6.0)]);
            }
        }
        Icon::Grid => {
            for x in [-5.0, 0.0, 5.0] {
                for y in [-5.0, 0.0, 5.0] {
                    rect(
                        window,
                        Rect::from_center_size(p(x, y), vec2(2.0, 2.0)),
                        color,
                        None,
                        1.0,
                    );
                }
            }
        }
        Icon::Help => {
            paths.push(
                (0..=32)
                    .map(|i| {
                        let a = i as f32 / 32.0 * std::f32::consts::TAU;
                        p(a.cos() * 8.0, a.sin() * 8.0)
                    })
                    .collect(),
            );
            paths.push(vec![
                p(-3.0, -3.0),
                p(-2.0, -5.0),
                p(2.0, -5.0),
                p(3.0, -3.0),
                p(0.0, 0.0),
                p(0.0, 2.0),
            ]);
            paths.push(vec![p(0.0, 5.0), p(0.0, 6.0)]);
        }
        Icon::Trash => {
            paths.push(vec![p(-5.0, -4.0), p(-4.0, 7.0), p(4.0, 7.0), p(5.0, -4.0)]);
            paths.push(vec![p(-7.0, -4.0), p(7.0, -4.0)]);
            paths.push(vec![p(-3.0, -7.0), p(3.0, -7.0)]);
        }
        Icon::Copy | Icon::Front | Icon::Back => {
            paths.push(vec![
                p(-3.0, -3.0),
                p(7.0, -3.0),
                p(7.0, 7.0),
                p(-3.0, 7.0),
                p(-3.0, -3.0),
            ]);
            paths.push(vec![p(-7.0, 3.0), p(-7.0, -7.0), p(3.0, -7.0)]);
        }
        Icon::Map => paths.push(vec![
            p(-8.0, -6.0),
            p(-3.0, -8.0),
            p(3.0, -5.0),
            p(8.0, -7.0),
            p(8.0, 6.0),
            p(3.0, 8.0),
            p(-3.0, 5.0),
            p(-8.0, 7.0),
            p(-8.0, -6.0),
        ]),
        Icon::Close => {
            paths.push(vec![p(-5.0, -5.0), p(5.0, 5.0)]);
            paths.push(vec![p(-5.0, 5.0), p(5.0, -5.0)]);
        }
        Icon::Eraser => {
            paths.push(vec![
                p(-7.0, 2.5),
                p(1.0, -5.5),
                p(6.0, -0.5),
                p(-2.0, 7.5),
                p(-7.0, 2.5),
            ]);
            paths.push(vec![p(-3.4, -1.1), p(1.6, 3.9)]);
            paths.push(vec![p(-2.0, 7.5), p(7.0, 7.5)]);
        }
        Icon::Save => {
            paths.push(vec![
                p(-7.0, -7.0),
                p(4.0, -7.0),
                p(7.0, -4.0),
                p(7.0, 7.0),
                p(-7.0, 7.0),
                p(-7.0, -7.0),
            ]);
            paths.push(vec![
                p(-4.0, -7.0),
                p(-4.0, -3.0),
                p(3.0, -3.0),
                p(3.0, -7.0),
            ]);
            paths.push(vec![p(-4.0, 7.0), p(-4.0, 2.0), p(4.0, 2.0), p(4.0, 7.0)]);
        }
        Icon::Menu => {
            for y in [-5.0, 0.0, 5.0] {
                paths.push(vec![p(-7.0, y), p(7.0, y)]);
            }
        }
        Icon::Chevron => paths.push(vec![p(-3.5, -1.75), p(0.0, 1.75), p(3.5, -1.75)]),
    }
    for points in paths {
        path(window, &points, color, 1.5, false, false);
    }
}
