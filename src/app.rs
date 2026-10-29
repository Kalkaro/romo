use crate::{
    geometry::{Color32, Corner, Pos2, Rect, Vec2, pos2, vec2},
    images::ImageAsset,
    input::{EditorStyle, TextChanged, TextInput},
    model::{Board, Item, Kind, PALETTE, View},
    paint::{self, BASE, BORDER, FG, Icon, MUTED, SURFACE},
    snapping::{self, Guide},
    storage::{self, Document},
    svg,
};
use gpui::{
    Bounds, Context, Div, Entity, FocusHandle, KeyDownEvent, KeyUpEvent, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, PlatformInput, Render, ScrollWheelEvent,
    SharedString, Stateful, Subscription, Window, canvas, div, prelude::*, px, rgb,
};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet, VecDeque},
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

const DECLUTTER_FADE: Duration = Duration::from_millis(200);
/// Eraser reach around the pointer, in screen pixels.
const ERASER_RADIUS: f32 = 10.0;
/// How long the eraser's trail lingers.
const TRAIL: Duration = Duration::from_millis(250);
/// After this long without zooming, text renders at exact sizes again.
const ZOOM_SETTLE: Duration = Duration::from_millis(150);
/// Floating panels sit 24 px from the window edge and are 46 px thick.
const PANEL_REACH: f32 = 70.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tool {
    Select,
    Hand,
    Note,
    Text,
    Rectangle,
    Ellipse,
    Diamond,
    Arrow,
    Pen,
    Eraser,
    Frame,
}
impl Tool {
    fn kind(self) -> Option<Kind> {
        match self {
            Self::Select | Self::Hand | Self::Eraser => None,
            Self::Note => Some(Kind::Note),
            Self::Text => Some(Kind::Text),
            Self::Rectangle => Some(Kind::Rectangle),
            Self::Ellipse => Some(Kind::Ellipse),
            Self::Diamond => Some(Kind::Diamond),
            Self::Arrow => Some(Kind::Arrow),
            Self::Pen => Some(Kind::Pen),
            Self::Frame => Some(Kind::Frame),
        }
    }
    fn label(self) -> &'static str {
        match self {
            Self::Select => "Select",
            Self::Hand => "Hand",
            Self::Note => "Sticky note",
            Self::Text => "Text",
            Self::Rectangle => "Rectangle",
            Self::Ellipse => "Ellipse",
            Self::Diamond => "Diamond",
            Self::Arrow => "Connector",
            Self::Pen => "Pen",
            Self::Eraser => "Eraser",
            Self::Frame => "Frame",
        }
    }
    fn icon(self) -> Icon {
        match self {
            Self::Select => Icon::Select,
            Self::Hand => Icon::Hand,
            Self::Note => Icon::Note,
            Self::Text => Icon::Text,
            Self::Rectangle => Icon::Rectangle,
            Self::Ellipse => Icon::Ellipse,
            Self::Diamond => Icon::Diamond,
            Self::Arrow => Icon::Arrow,
            Self::Pen => Icon::Pen,
            Self::Eraser => Icon::Eraser,
            Self::Frame => Icon::Frame,
        }
    }
    fn key(self) -> &'static str {
        match self {
            Self::Select => "V",
            Self::Hand => "H",
            Self::Note => "N",
            Self::Text => "T",
            Self::Rectangle => "R",
            Self::Ellipse => "O",
            Self::Diamond => "D",
            Self::Arrow => "L",
            Self::Pen => "P or B",
            Self::Eraser => "E",
            Self::Frame => "F",
        }
    }
}
enum Gesture {
    Pan {
        previous: Pos2,
    },
    Move {
        start: Pos2,
        originals: Vec<Item>,
        before: Board,
    },
    Marquee {
        start: Pos2,
        current: Pos2,
        base: HashSet<u64>,
    },
    /// Resize one object from a corner handle, keeping the opposite corner in place.
    Resize {
        id: u64,
        start: Pos2,
        corner: Corner,
        original: Item,
        before: Board,
    },
    /// Crop an image from a corner handle dragged with Alt, leaving the picture in place.
    Crop {
        id: u64,
        start: Pos2,
        corner: Corner,
        original: Item,
        before: Board,
    },
    Create {
        id: u64,
        start: Pos2,
        before: Board,
    },
    Erase {
        previous: Pos2,
        before: Board,
    },
    /// Scale a selection of pen strokes from a corner handle of `bounds`,
    /// keeping the opposite corner in place.
    Scale {
        start: Pos2,
        corner: Corner,
        bounds: Rect,
        originals: Vec<Item>,
        before: Board,
    },
}
#[derive(Clone, Copy)]
enum Action {
    Tool(Tool),
    Undo,
    Redo,
    New,
    Open,
    Save,
    SaveAs,
    Export,
    Copy,
    Paste,
    Duplicate,
    Delete,
    Front,
    Back,
    Zoom(f32),
    ResetZoom,
    Fit,
    Grid,
    Map,
    Declutter,
    Help,
    Snap,
    FileMenu,
    Edit,
    Title,
    Color(Color32),
    Font(f32),
    Width(f32),
    Height(f32),
    LineWidth(f32),
    HideProperties,
    Boards,
    Board(usize),
    Settings,
    Colors,
    Sizes,
    ImportImage,
    ApplyEdit,
    CancelEdit,
}
#[derive(Clone, Copy)]
enum BoardCommand {
    New,
    Open,
}
#[derive(Clone, Copy)]
enum EditTarget {
    Title,
    Object(u64),
}
struct Editing {
    target: EditTarget,
    input: Entity<TextInput>,
    before: Board,
    _subscription: Option<Subscription>,
}
/// Frame timings recorded by `--bench`.
#[derive(Default)]
struct Bench {
    frame: usize,
    last_frame: Option<Instant>,
    board_paint: Duration,
    samples: Vec<(&'static str, f32, f32)>,
}

pub struct Romo {
    board: Board,
    paint_cache: paint::Cache,
    view: View,
    tool: Tool,
    color: Color32,
    selected: HashSet<u64>,
    gesture: Option<Gesture>,
    undo: VecDeque<Board>,
    redo: Vec<Board>,
    clipboard: Vec<Item>,
    image_clipboard: Option<arboard::Clipboard>,
    paste_count: u32,
    /// Where on the board the last paste went, so pasting there again cascades.
    paste_spot: Option<Pos2>,
    path: Option<PathBuf>,
    dirty: bool,
    session_path: Option<PathBuf>,
    session_dirty: bool,
    /// Snapshots taken for saving, which numbers them.
    saves: u64,
    /// The newest snapshot written, locked while one is being written.
    written: Arc<Mutex<u64>>,
    /// An autosave is being written in the background.
    saving: bool,
    toast: Option<(String, bool, Instant)>,
    show_help: bool,
    show_grid: bool,
    snap: bool,
    show_map: bool,
    show_properties: bool,
    show_boards: bool,
    show_settings: bool,
    show_colors: bool,
    show_sizes: bool,
    library: Vec<storage::BoardEntry>,
    library_dir: Option<PathBuf>,
    settings_path: Option<PathBuf>,
    guides: Vec<Guide>,
    declutter: bool,
    chrome_fade: Option<(Instant, f32)>,
    file_menu: bool,
    fit_pending: bool,
    canvas: Rect,
    space: bool,
    /// Pointer over the board while erasing, for the eraser outline.
    eraser: Option<Pos2>,
    /// Where the mouse or pen last moved in the window, on screen.
    pointer: Option<Pos2>,
    /// The selection handle under the pointer, for a resize cursor.
    hovered: Option<Corner>,
    /// A control pressed with a tablet pen, run if the pen lifts over it.
    pen_control: Option<(Rect, Action)>,
    /// Where the eraser passed recently, on the board.
    trail: VecDeque<(Pos2, Instant)>,
    /// The zoom at the last render, and when it last changed.
    zoom_seen: (f32, Instant),
    focus: FocusHandle,
    editing: Option<Editing>,
    smoke: bool,
    bench: Option<Bench>,
    mini_rect: Rect,
    mini_world: Rect,
}

impl Romo {
    /// Animate the view each frame over `board`, or the board already open,
    /// then report timings and quit.
    pub fn start_bench(
        &mut self,
        board: Option<Board>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(board) = board {
            self.board = board;
        }
        self.selected.clear();
        self.bench = Some(Bench::default());
        window.refresh();
        cx.notify();
    }
    fn step_bench(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(bench) = &mut self.bench else {
            return;
        };
        let Some((phase, view)) = crate::bench::view(bench.frame) else {
            crate::bench::report(&bench.samples);
            cx.quit();
            return;
        };
        let now = Instant::now();
        if let Some(last) = bench.last_frame.replace(now) {
            bench.samples.push((
                phase,
                (now - last).as_secs_f32() * 1000.0,
                bench.board_paint.as_secs_f32() * 1000.0,
            ));
        }
        bench.frame += 1;
        // The views circle the middle of the board.
        let middle = self
            .board
            .bounds()
            .map_or(Pos2::ZERO, |bounds| bounds.center());
        self.view = View {
            pan: view.pan - middle.to_vec2() * view.zoom,
            zoom: view.zoom,
        };
        window.request_animation_frame();
    }
    fn chrome_opacity(&self, now: Instant) -> f32 {
        let target = if self.declutter { 0.0 } else { 1.0 };
        match self.chrome_fade {
            Some((start, from)) => {
                let t = (now.duration_since(start).as_secs_f32() / DECLUTTER_FADE.as_secs_f32())
                    .clamp(0.0, 1.0);
                let eased = t * t * (3.0 - 2.0 * t);
                from + (target - from) * eased
            }
            None => target,
        }
    }
    fn selection_bounds(&self) -> Option<Rect> {
        self.board
            .items
            .iter()
            .filter(|i| self.selected.contains(&i.id))
            .map(|i| self.board.item_rect(i))
            .reduce(|a, b| a.union(b))
    }
    /// Bounds with selection handles, and the corners that have them. A single
    /// shape resizes from its corners; a selection of pen strokes scales from them.
    fn handles(&self) -> Option<(Rect, &'static [Corner])> {
        if self
            .editing
            .as_ref()
            .is_some_and(|edit| matches!(edit.target, EditTarget::Object(_)))
        {
            return None;
        }
        let mut chosen = self
            .board
            .items
            .iter()
            .filter(|i| self.selected.contains(&i.id));
        let first = chosen.next()?;
        let single = self.selected.len() == 1 && !matches!(first.kind, Kind::Arrow | Kind::Pen);
        let bounds = if single {
            first.rect()
        } else {
            std::iter::once(first)
                .chain(chosen)
                .try_fold(Rect::NOTHING, |bounds, item| {
                    (item.kind == Kind::Pen).then(|| bounds.union(item.rect()))
                })?
        };
        // Small on screen, a shape keeps only its bottom-right handle and a
        // drawing none, so they stay draggable from anywhere inside.
        let screen = self.view.screen_rect(bounds, self.canvas);
        if screen.width().max(screen.height()) >= 24.0 {
            Some((bounds, &Corner::ALL))
        } else {
            single.then_some((bounds, &[Corner::BottomRight]))
        }
    }
    /// The selection handle under `pointer`, on screen, and the bounds it belongs to.
    fn handle_at(&self, pointer: Pos2) -> Option<(Rect, Corner)> {
        let (bounds, corners) = self.handles()?;
        // Handles sit on the corners of the selection outline.
        let around = self.view.screen_rect(bounds, self.canvas).expand(4.0);
        corners
            .iter()
            .find(|corner| {
                Rect::from_center_size(corner.of(around), vec2(16.0, 16.0)).contains(pointer)
            })
            .map(|&corner| (bounds, corner))
    }
    fn refresh_library(&mut self) {
        self.library = self
            .library_dir
            .as_ref()
            .map(|dir| storage::library(dir))
            .unwrap_or_default();
    }
    fn context_menu(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_edit(window, cx);
        self.cancel_gesture();
        let world = self.view.to_world(
            pos2(event.position.x.into(), event.position.y.into()),
            self.canvas,
        );
        if !self
            .selection_bounds()
            .is_some_and(|r| r.expand(4.0 / self.view.zoom).contains(world))
        {
            self.selected = self
                .board
                .hit_test(world, 6.0 / self.view.zoom, false)
                .map(|id| HashSet::from([id]))
                .unwrap_or_default();
        }
        self.show_properties = !self.selected.is_empty();
        self.show_colors = false;
        self.show_sizes = false;
        cx.stop_propagation();
        cx.notify();
    }
    /// Add an image centered at `center` on the board.
    fn add_image(&mut self, asset: ImageAsset, center: Pos2) {
        let before = self.board.clone();
        let scale = (640.0 / asset.width as f32)
            .min(480.0 / asset.height as f32)
            .min(1.0);
        let size = vec2(asset.width as f32, asset.height as f32) * scale;
        let id = self.board.add(Kind::Image, center - size * 0.5, PALETTE[7]);
        let item = self.board.get_mut(id).unwrap();
        item.size = size;
        item.image = Some(std::sync::Arc::new(asset));
        self.selected = HashSet::from([id]);
        self.show_properties = false;
        self.tool = Tool::Select;
        self.changed(before);
    }
    fn board_picker(&self, cx: &Context<Self>) -> Div {
        // Below the board title in the top bar.
        let mut picker = panel()
            .absolute()
            .left(px(122.0))
            .top(px(PANEL_REACH + 8.0))
            .w(px(300.0))
            .max_h(px(500.0))
            .p_2()
            .flex()
            .flex_col()
            .gap_1();
        picker = picker.child(div().px_2().py_2().child(label("BOARDS")));
        let mut list = div()
            .id("board-picker-list")
            .max_h(px(300.0))
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap_1();
        for (index, entry) in self.library.iter().enumerate() {
            let active = self.path.as_ref() == Some(&entry.path);
            list = list.child(
                self.button(
                    &format!("board-{index}"),
                    &format!("{}{}", if active { "• " } else { "" }, entry.title),
                    Action::Board(index),
                    false,
                    cx,
                )
                .justify_start()
                .flex_shrink_0()
                .overflow_hidden(),
            );
        }
        picker
            .child(list)
            .child(div().h(px(1.0)).bg(rgb(BORDER)).my_1())
            .child(
                self.button("picker-new", "New board", Action::New, false, cx)
                    .justify_start(),
            )
            .child(
                self.button("picker-open", "Import board…", Action::Open, false, cx)
                    .justify_start(),
            )
            .child(
                self.button(
                    "picker-rename",
                    "Rename current board",
                    Action::Title,
                    false,
                    cx,
                )
                .justify_start(),
            )
    }
    fn settings(&self, cx: &Context<Self>) -> Div {
        panel()
            .absolute()
            .left_6()
            .bottom(px(76.0))
            .w(px(300.0))
            .p_3()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(label("SETTINGS"))
                    .child(self.icon_button(
                        "close-settings",
                        Icon::Close,
                        Action::Settings,
                        false,
                        "Close settings",
                        cx,
                    )),
            )
            .child(
                self.button(
                    "settings-snap",
                    if self.snap {
                        "Object snapping  ·  On"
                    } else {
                        "Object snapping  ·  Off"
                    },
                    Action::Snap,
                    false,
                    cx,
                )
                .justify_start(),
            )
            .child(label(
                "Align to the edges and centers of nearby objects. Hold Alt while dragging to bypass.",
            ))
    }
    pub fn smoke_ui(&self) -> (bool, usize, bool) {
        (
            self.declutter,
            self.selected.len(),
            self.editing
                .as_ref()
                .is_some_and(|edit| matches!(edit.target, EditTarget::Object(_))),
        )
    }
    pub fn smoke_interaction(&self) -> (bool, bool, bool) {
        (self.show_map, self.gesture.is_some(), self.show_properties)
    }
    pub fn smoke_tool(&self) -> &'static str {
        self.tool.label()
    }
    /// Clicked text hugs what is typed, an abandoned empty one leaves no trace,
    /// and erasing leaves a trail.
    pub fn smoke_text_check(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> anyhow::Result<()> {
        self.board = Board::default();
        self.view = View::default();
        self.undo.clear();
        let point = self.view.to_screen(pos2(-300.0, -100.0), self.canvas);
        self.action(Action::Tool(Tool::Text), window, cx);
        self.smoke_mouse(point, point, window, cx);
        let input = self
            .editing
            .as_ref()
            .map(|edit| edit.input.clone())
            .ok_or_else(|| anyhow::anyhow!("Clicking with the text tool did not start editing"))?;
        input.update(cx, |input, cx| {
            gpui::EntityInputHandler::replace_text_in_range(input, None, "Hi", window, cx)
        });
        self.sync_inline_editor(window, cx);
        self.apply_edit(window, cx);
        let text = self
            .board
            .items
            .first()
            .cloned()
            .unwrap_or_else(|| Item::new(0, Kind::Text, Pos2::ZERO, PALETTE[7]));
        anyhow::ensure!(
            text.text == "Hi" && text.auto_width && text.size.x < 3.0 * text.font_size,
            "A clicked text box does not hug one word: {:?}",
            text.size
        );
        let (items, steps) = (self.board.items.len(), self.undo.len());
        let point = self.view.to_screen(pos2(-300.0, 100.0), self.canvas);
        self.action(Action::Tool(Tool::Text), window, cx);
        self.smoke_mouse(point, point, window, cx);
        self.apply_edit(window, cx);
        anyhow::ensure!(
            self.board.items.len() == items && self.undo.len() == steps,
            "An abandoned empty text box stayed or left an undo step"
        );
        self.action(Action::Tool(Tool::Eraser), window, cx);
        let point = self.view.to_screen(pos2(200.0, 200.0), self.canvas);
        self.smoke_mouse(point, point + vec2(120.0, 30.0), window, cx);
        anyhow::ensure!(self.trail.len() >= 2, "Erasing left no trail");
        self.action(Action::Tool(Tool::Select), window, cx);
        Ok(())
    }
    /// A tablet pen taps the Pen tool in the toolbar, then draws on the board.
    pub fn smoke_pen_check(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> anyhow::Result<()> {
        let button = CONTROLS.with_borrow(|controls| {
            controls
                .iter()
                .find(|(_, action)| matches!(action, Some(Action::Tool(Tool::Pen))))
                .map(|(rect, _)| paint::p(rect.center()))
        });
        let button =
            button.ok_or_else(|| anyhow::anyhow!("The toolbar was not recorded for pens"))?;
        let modifiers = Default::default();
        let press = |position| {
            PlatformInput::MouseDown(MouseDownEvent {
                button: MouseButton::Left,
                position,
                modifiers,
                click_count: 1,
                first_mouse: false,
            })
        };
        let release = |position| {
            PlatformInput::MouseUp(MouseUpEvent {
                button: MouseButton::Left,
                position,
                modifiers,
                click_count: 1,
            })
        };
        self.pen(press(button), window, cx);
        self.pen(release(button), window, cx);
        anyhow::ensure!(
            self.tool == Tool::Pen,
            "A pen tap did not choose the Pen tool"
        );
        let strokes = self.board.items.len();
        let (start, end) = (
            paint::p(self.canvas.center()),
            paint::p(self.canvas.center() + vec2(90.0, 40.0)),
        );
        self.pen(press(start), window, cx);
        self.pen(
            PlatformInput::MouseMove(MouseMoveEvent {
                position: end,
                pressed_button: Some(MouseButton::Left),
                modifiers,
            }),
            window,
            cx,
        );
        self.pen(release(end), window, cx);
        anyhow::ensure!(
            self.board.items.len() == strokes + 1
                && self
                    .board
                    .items
                    .last()
                    .is_some_and(|i| i.kind == Kind::Pen && i.rect().width() > 1.0),
            "A pen drag did not draw on the board"
        );
        Ok(())
    }
    /// Pasted objects land centered under the pointer and cascade when pasted
    /// again in the same spot. From a control, they go beside the originals.
    pub fn smoke_paste_check(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> anyhow::Result<()> {
        self.board = Board::default();
        self.view = View::default();
        let note = self.board.add(Kind::Note, pos2(-300.0, -300.0), PALETTE[0]);
        self.selected = HashSet::from([note]);
        self.tool = Tool::Select;
        self.copy(cx);
        let hover = |point: Pos2| MouseMoveEvent {
            position: paint::p(point),
            ..Default::default()
        };
        let spot = pos2(200.0, 100.0);
        self.motion(&hover(self.view.to_screen(spot, self.canvas)), window, cx);
        self.paste_objects();
        self.paste_objects();
        let centers: Vec<Pos2> = self.board.items[1..]
            .iter()
            .map(|i| i.rect().center())
            .collect();
        anyhow::ensure!(
            centers.len() == 2
                && centers[0].distance(spot) < 0.01
                && centers[1].distance(spot + vec2(32.0, 32.0)) < 0.01,
            "Pasting did not center the copy under the pointer, then cascade: {centers:?}"
        );
        let button = CONTROLS.with_borrow(|controls| {
            controls
                .iter()
                .find(|(_, action)| matches!(action, Some(Action::Tool(Tool::Pen))))
                .map(|(rect, _)| rect.center())
        });
        let button =
            button.ok_or_else(|| anyhow::anyhow!("The toolbar was not recorded for pasting"))?;
        self.motion(&hover(button), window, cx);
        self.paste_objects();
        anyhow::ensure!(
            self.board
                .items
                .last()
                .is_some_and(|i| i.pos == pos2(-236.0, -236.0)),
            "Pasting over the toolbar did not place the copy beside the original"
        );
        self.selected.clear();
        Ok(())
    }
    /// Scales two selected strokes up and back down through the selection handle.
    pub fn smoke_scale_check(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> anyhow::Result<()> {
        self.board = Board::default();
        self.view = View::default();
        for (from, to) in [
            (pos2(-100.0, -50.0), Pos2::ZERO),
            (Pos2::ZERO, pos2(100.0, 50.0)),
        ] {
            let id = self.board.add(Kind::Pen, from, PALETTE[7]);
            self.board.get_mut(id).unwrap().points = vec![Pos2::ZERO, (to - from).to_pos2()].into();
        }
        self.selected = self.board.items.iter().map(|i| i.id).collect();
        self.tool = Tool::Select;
        let corner = self.view.to_screen(pos2(100.0, 50.0), self.canvas);
        self.smoke_mouse(corner, corner + vec2(200.0, 100.0), window, cx);
        let bounds = self.selection_bounds().unwrap();
        anyhow::ensure!(
            bounds == Rect::from_min_max(pos2(-100.0, -50.0), pos2(300.0, 150.0)),
            "Dragging the handle did not double the drawing: {bounds:?}"
        );
        anyhow::ensure!(
            self.board.items.iter().all(|i| i.line_width() == 3.0),
            "Scaling a drawing up thickened its strokes"
        );
        let corner = self.view.to_screen(bounds.max, self.canvas);
        self.smoke_mouse(corner, corner - vec2(300.0, 150.0), window, cx);
        anyhow::ensure!(
            self.board
                .items
                .iter()
                .all(|i| (i.line_width() - 1.5).abs() < 0.001),
            "Shrinking a drawing below its drawn size kept its full stroke width"
        );
        self.action(Action::Undo, window, cx);
        anyhow::ensure!(
            self.selection_bounds() == Some(bounds),
            "Undo did not restore the scaled drawing"
        );
        self.selected.clear();
        Ok(())
    }
    /// Leaves a note, a lone stroke at y = -150 and a two-stroke drawing on the board.
    pub fn smoke_drawing_snap_check(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> anyhow::Result<()> {
        self.board = Board::default();
        self.view = View::default();
        let stroke = |board: &mut Board, from: Pos2, to: Pos2| {
            let id = board.add(Kind::Pen, from, PALETTE[7]);
            board.get_mut(id).unwrap().points = vec![Pos2::ZERO, (to - from).to_pos2()].into();
        };
        let note = self.board.add(Kind::Note, pos2(-300.0, -300.0), PALETTE[0]);
        stroke(&mut self.board, pos2(0.0, -150.0), pos2(80.0, -150.0));
        stroke(&mut self.board, pos2(100.0, 100.0), pos2(200.0, 160.0));
        stroke(&mut self.board, pos2(205.0, 120.0), pos2(300.0, 200.0));
        self.selected = HashSet::from([note]);
        self.tool = Tool::Select;
        self.snap = true;
        // The note's top edge passes 3 px from the lone stroke, which is no target.
        let start = self.view.to_screen(pos2(-205.0, -205.0), self.canvas);
        self.smoke_mouse(start, start + vec2(0.0, 147.0), window, cx);
        anyhow::ensure!(
            self.board.get(note).unwrap().pos == pos2(-300.0, -153.0),
            "Snapping aligned to a lone pen stroke"
        );
        // Beside the touching strokes, the note aligns to the whole drawing.
        let start = start + vec2(0.0, 147.0);
        self.smoke_mouse(start, start + vec2(604.0, 250.0), window, cx);
        anyhow::ensure!(
            self.board.get(note).unwrap().pos == pos2(300.0, 100.0),
            "Snapping ignored the pen drawing"
        );
        self.selected.clear();
        Ok(())
    }
    pub fn smoke_snap_check(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> anyhow::Result<()> {
        let original_board = self.board.clone();
        let original_view = self.view;
        self.board = Board::default();
        self.view = View::default();
        let id = self
            .board
            .add(Kind::Rectangle, pos2(-250.0, -80.0), PALETTE[0]);
        self.board.get_mut(id).unwrap().size = vec2(100.0, 100.0);
        let target = self
            .board
            .add(Kind::Rectangle, pos2(100.0, 80.0), PALETTE[0]);
        self.board.get_mut(target).unwrap().size = vec2(100.0, 100.0);
        self.selected = HashSet::from([id]);
        self.tool = Tool::Select;
        self.snap = true;
        let start = self.view.to_screen(pos2(-200.0, -30.0), self.canvas);
        self.smoke_mouse(start, start + vec2(346.0, 160.0), window, cx);
        anyhow::ensure!(
            self.board.get(id).unwrap().pos == pos2(100.0, 80.0),
            "Object move did not snap to target edges"
        );
        self.board.get_mut(id).unwrap().pos = pos2(-250.0, -80.0);
        self.snap = false;
        self.smoke_mouse(start, start + vec2(346.0, 160.0), window, cx);
        anyhow::ensure!(
            self.board.get(id).unwrap().pos == pos2(96.0, 80.0),
            "Disabled snapping still altered the drag"
        );
        self.board = original_board;
        self.view = original_view;
        self.selected.clear();
        self.undo.clear();
        self.redo.clear();
        Ok(())
    }
    pub fn smoke_library_check(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> anyhow::Result<()> {
        let folder =
            std::env::temp_dir().join(format!("romo-library-native-{}", std::process::id()));
        let previous_session = self.session_path.clone();
        self.library_dir = Some(folder.join("boards"));
        self.session_path = Some(folder.join("session.json"));
        self.settings_path = Some(folder.join("settings.json"));
        self.path = None;
        self.smoke = false;
        self.board.title = "Native first board".into();
        self.session_dirty = true;
        self.persist();
        anyhow::ensure!(!self.session_dirty, "Library save failed");
        let first_path = self.path.clone().unwrap();
        let first = self.board.clone();
        self.perform(BoardCommand::New);
        self.board.title = "Native second board".into();
        let id = self.board.add(Kind::Text, Pos2::ZERO, PALETTE[7]);
        self.board.get_mut(id).unwrap().text = "Content survives switching".into();
        self.session_dirty = true;
        self.persist();
        let second_path = self.path.clone().unwrap();
        self.refresh_library();
        anyhow::ensure!(self.library.len() == 2, "Board picker omitted a board");
        let index = self
            .library
            .iter()
            .position(|e| e.path == first_path)
            .unwrap();
        self.action(Action::Board(index), window, cx);
        anyhow::ensure!(self.board == first, "Board switch discarded saved objects");
        anyhow::ensure!(
            storage::load(&second_path)
                .map_err(anyhow::Error::msg)?
                .board
                .items[0]
                .text
                == "Content survives switching",
            "Switch did not save outgoing content"
        );
        let (session, restored) = storage::load_session(self.session_path.as_ref().unwrap())
            .map_err(anyhow::Error::msg)?;
        anyhow::ensure!(
            session.path == Some(first_path) && restored.board == first,
            "Restart would restore the wrong board"
        );
        anyhow::ensure!(
            session.document.is_none(),
            "The session kept a second copy of a board saved in the library"
        );
        let external = folder.join("external.romo");
        storage::save(&external, &Document::new(Board::default(), View::default()))
            .map_err(anyhow::Error::msg)?;
        let before = std::fs::read(&external)?;
        self.load(external.clone());
        self.board.title = "Imported copy".into();
        self.persist();
        anyhow::ensure!(
            std::fs::read(&external)? == before && self.path.as_ref() != Some(&external),
            "Import overwrote the original file"
        );
        self.snap = true;
        self.action(Action::Snap, window, cx);
        anyhow::ensure!(
            !storage::load_settings(self.settings_path.as_ref().unwrap()).snapping,
            "Snap opt-out did not persist"
        );
        self.smoke = true;
        self.library_dir = None;
        self.settings_path = None;
        self.session_path = previous_session;
        self.path = None;
        std::fs::remove_dir_all(folder)?;
        Ok(())
    }
    pub fn smoke_snapshot(&self) -> (Document, Rect, bool) {
        (
            Document::new(self.board.clone(), self.view),
            self.canvas,
            self.editing.is_some(),
        )
    }

    pub fn smoke_mouse(
        &mut self,
        start: Pos2,
        end: Pos2,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.down(
            &MouseDownEvent {
                button: MouseButton::Left,
                position: paint::p(start),
                modifiers: Default::default(),
                click_count: 1,
                first_mouse: false,
            },
            window,
            cx,
        );
        if start != end {
            self.motion(
                &MouseMoveEvent {
                    position: paint::p(end),
                    pressed_button: Some(MouseButton::Left),
                    ..Default::default()
                },
                window,
                cx,
            );
        }
        self.up(
            &MouseUpEvent {
                button: MouseButton::Left,
                position: paint::p(end),
                ..Default::default()
            },
            window,
            cx,
        );
    }

    pub fn new(
        path: Option<PathBuf>,
        smoke: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus = cx.focus_handle();
        window.focus(&focus);
        let mut app = Self {
            board: Board::welcome(),
            paint_cache: paint::Cache::default(),
            view: View::default(),
            tool: Tool::Select,
            color: PALETTE[0],
            selected: HashSet::new(),
            gesture: None,
            undo: VecDeque::new(),
            redo: Vec::new(),
            clipboard: Vec::new(),
            image_clipboard: arboard::Clipboard::new().ok(),
            paste_count: 0,
            paste_spot: None,
            path: None,
            dirty: false,
            session_path: storage::session_path(),
            session_dirty: true,
            saves: 0,
            written: Arc::default(),
            saving: false,
            toast: None,
            show_help: false,
            show_grid: true,
            snap: if smoke {
                false
            } else {
                storage::data_dir()
                    .map(|d| storage::load_settings(&d.join("settings.json")).snapping)
                    .unwrap_or(true)
            },
            show_map: false,
            show_properties: false,
            show_boards: false,
            show_settings: false,
            show_colors: false,
            show_sizes: false,
            library: Vec::new(),
            library_dir: if smoke {
                None
            } else {
                storage::data_dir().map(|d| d.join("boards"))
            },
            settings_path: if smoke {
                None
            } else {
                storage::data_dir().map(|d| d.join("settings.json"))
            },
            guides: Vec::new(),
            declutter: false,
            chrome_fade: None,
            file_menu: false,
            fit_pending: true,
            canvas: Rect::from_min_size(Pos2::ZERO, vec2(1440.0, 900.0)),
            space: false,
            eraser: None,
            pointer: None,
            hovered: None,
            pen_control: None,
            trail: VecDeque::new(),
            zoom_seen: (1.0, Instant::now()),
            focus,
            editing: None,
            smoke,
            bench: None,
            mini_rect: Rect::NOTHING,
            mini_world: Rect::NOTHING,
        };
        if let Some(path) = path {
            app.load(path);
        } else if !smoke
            && let Some(path) = app.session_path.clone()
            && path.exists()
        {
            match storage::load_session(&path) {
                Ok((session, document)) => {
                    app.board = document.board;
                    app.view = document.view;
                    app.path = session.path;
                    app.dirty = session.dirty;
                    app.fit_pending = false;
                }
                Err(error) => {
                    app.session_path = Some(path.with_extension("recovered.json"));
                    app.notify(format!("{error} Original recovery preserved."), true);
                }
            }
        }
        if !smoke {
            // Import legacy sessions and external files into the library without rewriting originals.
            if let Some(folder) = &app.library_dir {
                if app
                    .path
                    .as_ref()
                    .is_none_or(|path| path.parent() != Some(folder.as_path()))
                {
                    app.path = Some(storage::board_path(folder));
                }
                app.autosave(cx);
                app.refresh_library();
            }
        }
        cx.on_release(|app, cx| {
            app.commit_edit(cx);
            app.cancel_gesture();
            app.persist();
        })
        .detach();
        cx.on_app_quit(|app, cx| {
            app.commit_edit(cx);
            app.cancel_gesture();
            app.persist();
            app.image_clipboard.take();
            async {}
        })
        .detach();
        let entity = cx.entity().downgrade();
        window.on_window_should_close(cx, move |_, cx| {
            entity
                .update(cx, |app, cx| {
                    app.commit_edit(cx);
                    app.cancel_gesture();
                    app.persist();
                    !app.session_dirty || app.smoke
                })
                .unwrap_or(true)
        });
        cx.spawn(async move |entity, cx| {
            loop {
                gpui::Timer::after(Duration::from_secs(1)).await;
                if entity
                    .update(cx, |app, cx| {
                        if app.session_dirty && app.gesture.is_none() {
                            app.autosave(cx);
                            cx.notify();
                        }
                        if app.toast.as_ref().is_some_and(|(_, error, time)| {
                            time.elapsed() > Duration::from_secs(if *error { 12 } else { 4 })
                        }) {
                            app.toast = None;
                            cx.notify();
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        app
    }
    fn notify(&mut self, message: impl Into<String>, error: bool) {
        self.toast = Some((message.into(), error, Instant::now()));
    }
    /// Tablet pens, which GPUI doesn't deliver, arrive as mouse input. On a
    /// control they press it; anywhere else they act on the board like a mouse.
    pub fn pen(&mut self, input: PlatformInput, window: &mut Window, cx: &mut Context<Self>) {
        let at = |position: gpui::Point<Pixels>| pos2(position.x.into(), position.y.into());
        let control = |position| {
            CONTROLS.with_borrow(|controls| {
                controls
                    .iter()
                    .rev()
                    .find(|(rect, _)| rect.contains(at(position)))
                    .copied()
            })
        };
        match input {
            PlatformInput::MouseDown(event) => match control(event.position) {
                Some((rect, action)) => {
                    self.pen_control = action
                        .filter(|_| event.button == MouseButton::Left)
                        .map(|action| (rect, action))
                }
                None if event.button == MouseButton::Right => self.context_menu(&event, window, cx),
                None => self.down(&event, window, cx),
            },
            // Hovering a control keeps the eraser outline off it, as with a mouse.
            PlatformInput::MouseMove(event)
                if self.gesture.is_none() && control(event.position).is_some() =>
            {
                self.pointer = Some(at(event.position));
                if self.eraser.take().is_some() {
                    cx.notify();
                }
            }
            PlatformInput::MouseMove(event) => self.motion(&event, window, cx),
            PlatformInput::MouseUp(event) => match self.pen_control.take() {
                Some((rect, action)) if rect.contains(at(event.position)) => {
                    self.action(action, window, cx)
                }
                Some(_) => {}
                None => self.up(&event, window, cx),
            },
            PlatformInput::MouseExited(_) if self.eraser.take().is_some() => cx.notify(),
            _ => {}
        }
    }
    fn down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self
            .editing
            .as_ref()
            .is_some_and(|edit| matches!(edit.target, EditTarget::Object(_)))
        {
            self.apply_edit(window, cx);
        }
        if self.editing.is_some() || self.show_help {
            return;
        }
        window.focus(&self.focus);
        self.file_menu = false;
        self.show_boards = false;
        self.show_settings = false;
        self.show_properties = false;
        self.show_colors = false;
        self.show_sizes = false;
        let pointer = pos2(event.position.x.into(), event.position.y.into());
        let world = self.view.to_world(pointer, self.canvas);
        if event.button == MouseButton::Middle || self.space || self.tool == Tool::Hand {
            self.gesture = Some(Gesture::Pan { previous: pointer });
        } else if self.tool == Tool::Select {
            if event.click_count >= 2
                && let Some(id) = self.board.hit_test(world, 6.0 / self.view.zoom, false)
            {
                self.selected = HashSet::from([id]);
                self.show_properties = true;
                if self
                    .board
                    .get(id)
                    .is_some_and(|i| !matches!(i.kind, Kind::Arrow | Kind::Pen | Kind::Image))
                {
                    self.begin_edit(EditTarget::Object(id), window, cx);
                }
                cx.notify();
                return;
            }
            if let Some((bounds, corner)) = self.handle_at(pointer) {
                let mut originals: Vec<Item> = self
                    .board
                    .items
                    .iter()
                    .filter(|i| self.selected.contains(&i.id))
                    .cloned()
                    .collect();
                let before = self.board.clone();
                self.gesture = Some(if originals[0].kind == Kind::Pen {
                    Gesture::Scale {
                        start: world,
                        corner,
                        bounds,
                        originals,
                        before,
                    }
                } else {
                    let original = originals.remove(0);
                    if original.kind == Kind::Image && event.modifiers.alt {
                        Gesture::Crop {
                            id: original.id,
                            start: world,
                            corner,
                            original,
                            before,
                        }
                    } else {
                        Gesture::Resize {
                            id: original.id,
                            start: world,
                            corner,
                            original,
                            before,
                        }
                    }
                });
            } else if let Some(id) = if !event.modifiers.shift
                && self
                    .selection_bounds()
                    .is_some_and(|r| r.expand(4.0 / self.view.zoom).contains(world))
            {
                self.selected.iter().next().copied()
            } else {
                self.board.hit_test(world, 6.0 / self.view.zoom, false)
            } {
                if event.modifiers.shift {
                    if !self.selected.insert(id) {
                        self.selected.remove(&id);
                        cx.notify();
                        return;
                    }
                } else if !self.selected.contains(&id) {
                    self.selected = HashSet::from([id]);
                }
                let mut moving = self.selected.clone();
                for frame in self
                    .board
                    .items
                    .iter()
                    .filter(|i| self.selected.contains(&i.id) && i.kind == Kind::Frame)
                {
                    for item in &self.board.items {
                        if frame.rect().contains_rect(self.board.item_rect(item)) {
                            moving.insert(item.id);
                        }
                    }
                }
                self.gesture = Some(Gesture::Move {
                    start: world,
                    originals: self
                        .board
                        .items
                        .iter()
                        .filter(|i| moving.contains(&i.id))
                        .cloned()
                        .collect(),
                    before: self.board.clone(),
                });
            } else {
                let base = if event.modifiers.shift {
                    self.selected.clone()
                } else {
                    HashSet::new()
                };
                self.selected = base.clone();
                self.gesture = Some(Gesture::Marquee {
                    start: world,
                    current: world,
                    base,
                });
            }
        } else if self.tool == Tool::Eraser {
            let before = self.board.clone();
            self.trail.push_back((world, Instant::now()));
            self.erase(world, world);
            self.gesture = Some(Gesture::Erase {
                previous: world,
                before,
            });
        } else if let Some(kind) = self.tool.kind() {
            let before = self.board.clone();
            let color = match kind {
                Kind::Text | Kind::Arrow | Kind::Pen => Color32::from_rgb(250, 250, 250),
                Kind::Frame => PALETTE[4],
                _ => self.color,
            };
            let anchor = if kind == Kind::Arrow {
                self.board.hit_test(world, 10.0 / self.view.zoom, true)
            } else {
                None
            };
            let id = self.board.add(kind, world, color);
            let item = self.board.get_mut(id).unwrap();
            if matches!(kind, Kind::Pen | Kind::Arrow) {
                item.points = vec![Pos2::ZERO, Pos2::ZERO].into();
                item.start_anchor = anchor;
            } else if kind == Kind::Text {
                // Clicked text grows with what is typed; dragging out a box fixes its width.
                item.auto_width = true;
                item.size = fitted_text_size(window, item);
            } else {
                item.pos = world - item.size * 0.5;
            }
            self.selected = if matches!(kind, Kind::Pen | Kind::Arrow) {
                HashSet::new()
            } else {
                HashSet::from([id])
            };
            self.gesture = Some(Gesture::Create {
                id,
                start: world,
                before,
            });
        }
        cx.stop_propagation();
        cx.notify();
    }
    fn motion(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        let pointer = pos2(event.position.x.into(), event.position.y.into());
        self.pointer = Some(pointer);
        if self.tool == Tool::Eraser && self.eraser != Some(pointer) {
            self.eraser = Some(pointer);
            cx.notify();
        }
        if self.gesture.is_none() {
            let hovered = if self.tool == Tool::Select {
                self.handle_at(pointer).map(|(_, corner)| corner)
            } else {
                None
            };
            if self.hovered != hovered {
                self.hovered = hovered;
                cx.notify();
            }
            return;
        }
        let world = self.view.to_world(pointer, self.canvas);
        let zoom = self.view.zoom;
        let snap = self.snap && !event.modifiers.alt;
        self.guides.clear();
        let mut erased_from = None;
        match self.gesture.as_mut().unwrap() {
            Gesture::Pan { previous } => {
                self.view.pan += pointer - *previous;
                *previous = pointer;
                self.session_dirty = true;
            }
            Gesture::Move {
                start,
                originals,
                before,
            } => {
                let mut delta = world - *start;
                let moving: HashSet<_> = originals.iter().map(|i| i.id).collect();
                if snap {
                    // The whole selection snaps as one box.
                    let bounds = originals
                        .iter()
                        .map(|i| before.item_rect(i))
                        .reduce(|a, b| a.union(b));
                    if let Some(bounds) = bounds {
                        let shifted = Rect::from_min_size(bounds.min + delta, bounds.size());
                        let (adjust, guides) = snapping::align(
                            shifted,
                            &snapping::targets(before, |id| moving.contains(&id)),
                            zoom,
                        );
                        delta += adjust;
                        self.guides = guides;
                    }
                }
                if delta.length() * self.view.zoom > 2.0 {
                    // One pass over the board, however many objects move.
                    let originals: HashMap<u64, &Item> =
                        originals.iter().map(|i| (i.id, i)).collect();
                    for item in &mut self.board.items {
                        let Some(original) = originals.get(&item.id) else {
                            continue;
                        };
                        item.pos = original.pos + delta;
                        if original.kind == Kind::Arrow {
                            let (a, b) = before.arrow_ends(original);
                            item.points =
                                vec![(a - original.pos).to_pos2(), (b - original.pos).to_pos2()]
                                    .into();
                            if !item.start_anchor.is_some_and(|id| moving.contains(&id)) {
                                item.start_anchor = None;
                            }
                            if !item.end_anchor.is_some_and(|id| moving.contains(&id)) {
                                item.end_anchor = None;
                            }
                        }
                    }
                }
            }
            Gesture::Resize {
                id,
                start,
                corner,
                original,
                ..
            } => {
                let targets = if snap {
                    snapping::targets(&self.board, |other| other == *id)
                } else {
                    Vec::new()
                };
                let (rect, guides) = snapping::resize(
                    original.rect(),
                    *corner,
                    world - *start,
                    // Images keep their proportions unless Shift is held.
                    original.kind == Kind::Image && !event.modifiers.shift,
                    vec2(40.0, 40.0),
                    &targets,
                    zoom,
                );
                self.guides = guides;
                if let Some(item) = self.board.get_mut(*id) {
                    item.resize(rect.size());
                    item.pos = rect.min;
                }
            }
            Gesture::Crop {
                id,
                start,
                corner,
                original,
                ..
            } => {
                // Alt is held, so the crop never snaps.
                let (visible, _) = snapping::resize(
                    original.rect(),
                    *corner,
                    world - *start,
                    false,
                    vec2(40.0, 40.0),
                    &[],
                    zoom,
                );
                if let Some(item) = self.board.get_mut(*id) {
                    *item = original.cropped(visible);
                }
            }
            Gesture::Marquee {
                start,
                current,
                base,
            } => {
                *current = world;
                let rect = Rect::from_two_pos(*start, world);
                self.selected = base.clone();
                for item in &self.board.items {
                    let r = self.board.item_rect(item);
                    if if item.kind == Kind::Frame {
                        rect.contains_rect(r)
                    } else {
                        rect.intersects(r)
                    } {
                        self.selected.insert(item.id);
                    }
                }
            }
            Gesture::Create { id, start, .. } => {
                let mut world = world;
                // A shape being drawn out snaps its dragged corner.
                if snap
                    && world.distance(*start) * zoom > 8.0
                    && self
                        .board
                        .get(*id)
                        .is_some_and(|i| !matches!(i.kind, Kind::Pen | Kind::Arrow | Kind::Text))
                {
                    let targets = snapping::targets(&self.board, |other| other == *id);
                    (world, self.guides) = snapping::point(world, *start, &targets, zoom);
                }
                let anchor = self.board.hit_test(world, 10.0 / self.view.zoom, true);
                if let Some(item) = self.board.get_mut(*id) {
                    match item.kind {
                        Kind::Pen => {
                            let point = (world - item.pos).to_pos2();
                            if item
                                .points
                                .last()
                                .is_none_or(|p| p.distance(point) * self.view.zoom > 0.35)
                            {
                                item.points.push(point);
                            }
                        }
                        Kind::Arrow => {
                            item.points = vec![Pos2::ZERO, (world - *start).to_pos2()].into();
                            item.end_anchor = anchor.filter(|id| Some(*id) != item.start_anchor);
                        }
                        _ if world.distance(*start) * self.view.zoom > 8.0 => {
                            let rect = Rect::from_two_pos(*start, world);
                            item.pos = rect.min;
                            item.size = rect.size().max(vec2(40.0, 40.0));
                            item.auto_width = false;
                        }
                        _ => {}
                    }
                }
            }
            Gesture::Erase { previous, .. } => {
                erased_from = Some(std::mem::replace(previous, world));
            }
            Gesture::Scale {
                start,
                corner,
                bounds,
                originals,
                ..
            } => {
                let targets = if snap {
                    let moving: HashSet<_> = originals.iter().map(|i| i.id).collect();
                    snapping::targets(&self.board, |id| moving.contains(&id))
                } else {
                    Vec::new()
                };
                // Drawings scale uniformly unless Shift is held.
                let uniform = !event.modifiers.shift;
                let size = bounds.size();
                let (scaled, guides) = snapping::resize(
                    *bounds,
                    *corner,
                    world - *start,
                    uniform,
                    // Keep drawings at least 8 board units across.
                    size * (8.0 / size.x.max(size.y).max(1.0)),
                    &targets,
                    zoom,
                );
                self.guides = guides;
                let factor = if uniform {
                    Vec2::splat(if size.x >= size.y {
                        scaled.width() / size.x
                    } else {
                        scaled.height() / size.y
                    })
                } else {
                    // Sides thinner than a board unit keep their size.
                    vec2(
                        if size.x >= 1.0 { scaled.width() / size.x } else { 1.0 },
                        if size.y >= 1.0 { scaled.height() / size.y } else { 1.0 },
                    )
                };
                let anchor = corner.opposite().of(*bounds);
                for original in originals.iter() {
                    if let Some(item) = self.board.get_mut(original.id) {
                        *item = original.scaled(anchor, factor);
                    }
                }
            }
        }
        if let Some(from) = erased_from {
            self.trail.push_back((world, Instant::now()));
            self.erase(from, world);
        }
        cx.notify();
    }
    /// Remove the pen strokes the eraser touched while moving from `from` to `to`.
    fn erase(&mut self, from: Pos2, to: Pos2) {
        let erased = self
            .board
            .strokes_near(from, to, ERASER_RADIUS / self.view.zoom);
        if !erased.is_empty() {
            self.board.remove(&erased);
            self.selected.retain(|id| !erased.contains(id));
        }
    }
    fn up(&mut self, event: &MouseUpEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.guides.clear();
        if let Some(Gesture::Create { id, .. }) = &self.gesture {
            let world = self.view.to_world(
                pos2(event.position.x.into(), event.position.y.into()),
                self.canvas,
            );
            if let Some(item) = self.board.get_mut(*id)
                && item.kind == Kind::Pen
            {
                let point = (world - item.pos).to_pos2();
                if item
                    .points
                    .last()
                    .is_none_or(|last| last.distance(point) > 0.0001)
                {
                    item.points.push(point);
                }
            }
        }
        if let Some(gesture) = self.gesture.take() {
            match gesture {
                Gesture::Move { before, .. }
                | Gesture::Resize { before, .. }
                | Gesture::Crop { before, .. }
                | Gesture::Erase { before, .. }
                | Gesture::Scale { before, .. } => self.changed(before),
                Gesture::Create { id, before, .. } => {
                    let drawing = self
                        .board
                        .get(id)
                        .is_some_and(|i| matches!(i.kind, Kind::Arrow | Kind::Pen));
                    if drawing
                        && self.board.get(id).is_some_and(|i| {
                            i.kind == Kind::Arrow && i.rect().size().length() < 3.0
                        })
                    {
                        self.board = before;
                        self.selected.clear();
                    } else {
                        self.changed(before);
                        let text = self
                            .board
                            .get(id)
                            .is_some_and(|item| matches!(item.kind, Kind::Text | Kind::Note));
                        if text {
                            self.tool = Tool::Select;
                            self.show_properties = false;
                            self.begin_edit(EditTarget::Object(id), window, cx);
                        } else {
                            self.selected.clear();
                            self.show_properties = false;
                            if !drawing {
                                self.tool = Tool::Select;
                            }
                        }
                    }
                }
                Gesture::Pan { .. } | Gesture::Marquee { .. } => {}
            }
            cx.notify();
        }
    }
    fn scroll(&mut self, event: &ScrollWheelEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.editing.is_some() || self.show_help || self.gesture.is_some() {
            return;
        }
        let delta = event.delta.pixel_delta(px(30.0));
        let delta = vec2(delta.x.into(), delta.y.into());
        if event.modifiers.shift
            || (event.delta.precise() && !event.modifiers.control && !event.modifiers.platform)
        {
            self.view.pan += delta;
        } else {
            self.view.zoom_at(
                (delta.y * 0.0025).exp(),
                pos2(event.position.x.into(), event.position.y.into()),
                self.canvas,
            );
        }
        self.session_dirty = true;
        cx.stop_propagation();
        cx.notify();
    }
    fn key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let key = event.keystroke.key.as_str();
        let m = event.keystroke.modifiers;
        let command = m.control || m.platform;
        if self.editing.is_some() {
            if key == "escape" {
                self.action(Action::CancelEdit, window, cx);
                cx.stop_propagation();
            } else if key == "enter" && command {
                self.action(Action::ApplyEdit, window, cx);
                cx.stop_propagation();
            } else if command
                && matches!(key, "s" | "z" | "y")
                && self
                    .editing
                    .as_ref()
                    .is_some_and(|edit| matches!(edit.target, EditTarget::Object(_)))
            {
                self.apply_edit(window, cx);
                self.action(
                    match key {
                        "s" => {
                            if m.shift {
                                Action::SaveAs
                            } else {
                                Action::Save
                            }
                        }
                        "z" => {
                            if m.shift {
                                Action::Redo
                            } else {
                                Action::Undo
                            }
                        }
                        _ => Action::Redo,
                    },
                    window,
                    cx,
                );
                cx.stop_propagation();
            }
            return;
        }
        if !command && key == "y" && !event.is_held {
            self.action(Action::Declutter, window, cx);
            cx.stop_propagation();
            return;
        }
        if self.show_help {
            if key == "escape" {
                self.show_help = false;
                cx.notify();
                cx.stop_propagation();
            }
            return;
        }
        let action = if command {
            match key {
                "s" => Some(if m.shift {
                    Action::SaveAs
                } else {
                    Action::Save
                }),
                "o" => Some(Action::Open),
                "n" => Some(Action::New),
                "e" => Some(Action::Export),
                "z" => Some(if m.shift { Action::Redo } else { Action::Undo }),
                "y" => Some(Action::Redo),
                "c" => Some(Action::Copy),
                "v" => Some(Action::Paste),
                "d" => Some(Action::Duplicate),
                "x" => {
                    self.copy(cx);
                    Some(Action::Delete)
                }
                "a" => {
                    self.selected = self.board.items.iter().map(|i| i.id).collect();
                    cx.notify();
                    None
                }
                _ => None,
            }
        } else {
            match key {
                "v" => Some(Action::Tool(Tool::Select)),
                "h" => Some(Action::Tool(Tool::Hand)),
                "n" => Some(Action::Tool(Tool::Note)),
                "t" => Some(Action::Tool(Tool::Text)),
                "r" => Some(Action::Tool(Tool::Rectangle)),
                "o" => Some(Action::Tool(Tool::Ellipse)),
                "d" => Some(Action::Tool(Tool::Diamond)),
                "l" => Some(Action::Tool(Tool::Arrow)),
                "p" | "b" => Some(Action::Tool(Tool::Pen)),
                "e" => Some(Action::Tool(Tool::Eraser)),
                "f" => Some(Action::Tool(Tool::Frame)),
                "g" => Some(Action::Grid),
                "m" => Some(Action::Map),
                "?" => Some(Action::Help),
                "1" => Some(Action::ResetZoom),
                "2" => Some(Action::Fit),
                "+" | "=" => Some(Action::Zoom(1.2)),
                "-" => Some(Action::Zoom(1.0 / 1.2)),
                "x" | "backspace" | "delete" => Some(Action::Delete),
                "enter" => Some(Action::Edit),
                "f10" if m.shift => {
                    self.show_properties = !self.selected.is_empty();
                    cx.notify();
                    None
                }
                "space" => {
                    self.space = true;
                    cx.notify();
                    None
                }
                "escape" => {
                    self.show_properties = false;
                    self.show_boards = false;
                    self.show_settings = false;
                    self.file_menu = false;
                    self.cancel_gesture();
                    self.selected.clear();
                    self.tool = Tool::Select;
                    self.file_menu = false;
                    cx.notify();
                    None
                }
                "left" | "right" | "up" | "down" => {
                    let step = if m.shift { 1.0 } else { 10.0 };
                    let delta = match key {
                        "left" => vec2(-step, 0.0),
                        "right" => vec2(step, 0.0),
                        "up" => vec2(0.0, -step),
                        _ => vec2(0.0, step),
                    };
                    let before = self.board.clone();
                    for item in &mut self.board.items {
                        if self.selected.contains(&item.id) {
                            item.pos += delta;
                        }
                    }
                    self.changed(before);
                    cx.notify();
                    None
                }
                _ => None,
            }
        };
        if let Some(action) = action {
            self.action(action, window, cx);
            cx.stop_propagation();
        }
    }
    fn key_up(&mut self, event: &KeyUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.key == "space" {
            self.space = false;
            cx.notify();
        }
    }
    fn changed(&mut self, before: Board) {
        if before != self.board {
            if self.undo.len() >= 80 {
                self.undo.pop_front();
            }
            self.undo.push_back(before);
            self.redo.clear();
            self.dirty = true;
            self.session_dirty = true;
        }
    }
    fn persist(&mut self) {
        if self.smoke {
            self.session_dirty = false;
            return;
        }
        let dirty = self.dirty;
        let result = self.snapshot().write(&self.written);
        self.saved(result, dirty);
    }
    /// Persist on a background thread, so saving a large board never stalls
    /// the board after a drag. One autosave is written at a time.
    fn autosave(&mut self, cx: &mut Context<Self>) {
        if self.smoke {
            return self.persist();
        }
        if self.saving {
            return;
        }
        self.saving = true;
        let dirty = self.dirty;
        let snapshot = self.snapshot();
        let written = self.written.clone();
        cx.spawn(async move |app, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { snapshot.write(&written) })
                .await;
            app.update(cx, |app, cx| {
                app.saving = false;
                app.saved(result, dirty);
                cx.notify();
            })
        })
        .detach();
    }
    /// What `persist` writes. The board counts as saved from here on, so
    /// changes made while it is written leave it unsaved again.
    fn snapshot(&mut self) -> storage::Snapshot {
        let board = self.library_dir.as_ref().map(|folder| {
            self.path
                .get_or_insert_with(|| storage::board_path(folder))
                .clone()
        });
        if board.is_some() {
            self.dirty = false;
        }
        self.session_dirty = false;
        self.saves += 1;
        storage::Snapshot {
            number: self.saves,
            document: Document::new(self.board.clone(), self.view),
            board,
            session: self.session_path.clone(),
            path: self.path.clone(),
            dirty: self.dirty,
        }
    }
    /// After a failed save, the board is as unsaved as it was before.
    fn saved(&mut self, result: Result<(), String>, dirty: bool) {
        if let Err(error) = result {
            self.dirty |= dirty;
            self.session_dirty = true;
            self.notify(error, true);
        }
    }
    fn save(&mut self, save_as: bool) -> bool {
        if !save_as && !self.smoke {
            self.session_dirty = true;
            self.persist();
            self.refresh_library();
            if !self.session_dirty {
                self.notify("Board saved", false);
                return true;
            }
            return false;
        }
        let path = if save_as || self.path.is_none() {
            rfd::FileDialog::new()
                .add_filter("Romo board", &["romo"])
                .set_file_name(format!("{}.romo", filename(&self.board.title)))
                .save_file()
        } else {
            self.path.clone()
        };
        let Some(path) = path else {
            return false;
        };
        let path = if path.extension().is_none() {
            path.with_extension("romo")
        } else {
            path
        };
        match storage::save(&path, &Document::new(self.board.clone(), self.view)) {
            Ok(()) => {
                if !save_as {
                    self.path = Some(path);
                }
                self.dirty = false;
                self.session_dirty = true;
                self.persist();
                self.notify("Board saved", false);
                true
            }
            Err(error) => {
                self.notify(error, true);
                false
            }
        }
    }
    fn load(&mut self, path: PathBuf) {
        match storage::load(&path) {
            Ok(doc) => {
                self.board = doc.board;
                self.view = doc.view;
                self.path = if self
                    .library_dir
                    .as_ref()
                    .is_some_and(|folder| path.parent() != Some(folder.as_path()))
                {
                    self.library_dir
                        .as_ref()
                        .map(|folder| storage::board_path(folder))
                } else {
                    Some(path)
                };
                self.dirty = false;
                self.selected.clear();
                self.undo.clear();
                self.redo.clear();
                self.tool = Tool::Select;
                self.fit_pending = false;
                self.session_dirty = true;
                self.notify("Board opened", false);
            }
            Err(error) => self.notify(error, true),
        }
    }
    fn request(&mut self, action: BoardCommand) {
        self.cancel_gesture();
        self.session_dirty = true;
        self.persist();
        if self.smoke || !self.session_dirty {
            self.perform(action);
        }
    }
    fn perform(&mut self, action: BoardCommand) {
        match action {
            BoardCommand::New => {
                self.board = Board::default();
                let mut number = 2;
                while self
                    .library
                    .iter()
                    .any(|entry| entry.title == self.board.title)
                {
                    self.board.title = format!("Untitled board {number}");
                    number += 1;
                }
                self.view = View::default();
                self.selected.clear();
                self.undo.clear();
                self.redo.clear();
                self.path = None;
                self.dirty = false;
                self.session_dirty = true;
                self.tool = Tool::Select;
                self.show_boards = false;
                self.persist();
                self.refresh_library();
            }
            BoardCommand::Open => {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("Romo board", &["romo", "json"])
                    .pick_file()
                {
                    self.load(path);
                    self.persist();
                    self.refresh_library();
                }
            }
        }
    }
    fn export(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("SVG image", &["svg"])
            .set_file_name(format!("{}.svg", filename(&self.board.title)))
            .save_file()
        {
            let path = if path.extension().is_none() {
                path.with_extension("svg")
            } else {
                path
            };
            match storage::atomic_write(&path, svg::export(&self.board).as_bytes()) {
                Ok(()) => self.notify("Board exported as SVG", false),
                Err(error) => self.notify(error, true),
            }
        }
    }
    fn cancel_gesture(&mut self) {
        self.guides.clear();
        if let Some(gesture) = self.gesture.take() {
            match gesture {
                Gesture::Move { before, .. }
                | Gesture::Resize { before, .. }
                | Gesture::Crop { before, .. }
                | Gesture::Create { before, .. }
                | Gesture::Erase { before, .. }
                | Gesture::Scale { before, .. } => {
                    self.board = before;
                    self.selected.retain(|id| self.board.get(*id).is_some());
                }
                Gesture::Marquee { base, .. } => self.selected = base,
                Gesture::Pan { .. } => {}
            }
        }
    }
    fn copy(&mut self, cx: &mut Context<Self>) {
        self.clipboard = self
            .board
            .items
            .iter()
            .filter(|i| self.selected.contains(&i.id))
            .map(|i| {
                let mut i = i.clone();
                if i.kind == Kind::Arrow {
                    let (a, b) = self.board.arrow_ends(&i);
                    i.points = vec![(a - i.pos).to_pos2(), (b - i.pos).to_pos2()].into();
                    i.start_anchor = i.start_anchor.filter(|id| self.selected.contains(id));
                    i.end_anchor = i.end_anchor.filter(|id| self.selected.contains(id));
                }
                i
            })
            .collect();
        self.paste_count = 0;
        if self.clipboard.len() == 1
            && let Some(asset) = &self.clipboard[0].image
        {
            // Other applications get only the part of a cropped image that shows.
            let crop = self.clipboard[0].crop;
            let result = asset.png(crop).and_then(|png| {
                cx.write_to_clipboard(gpui::ClipboardItem::new_image(&gpui::Image::from_bytes(
                    gpui::ImageFormat::Png,
                    png,
                )));
                // GPUI 0.2.2 only offers text to other Linux applications. Keep a native
                // image clipboard alive so the PNG remains available outside Romo.
                if self.image_clipboard.is_none() {
                    self.image_clipboard =
                        Some(arboard::Clipboard::new().map_err(|e| e.to_string())?);
                }
                let rgba = asset.pixels(crop);
                self.image_clipboard
                    .as_mut()
                    .unwrap()
                    .set_image(arboard::ImageData {
                        width: rgba.width() as usize,
                        height: rgba.height() as usize,
                        bytes: std::borrow::Cow::Owned(rgba.into_raw()),
                    })
                    .map_err(|e| e.to_string())
            });
            if let Err(error) = result {
                self.notify(format!("Cannot copy image: {error}"), true);
            }
        } else if !self.clipboard.is_empty()
            && let Ok(data) = serde_json::to_string(&self.clipboard)
        {
            cx.write_to_clipboard(gpui::ClipboardItem::new_string_with_metadata(
                "Romo objects".into(),
                data,
            ));
        }
    }
    fn paste(&mut self, cx: &mut Context<Self>) {
        if let Some(clipboard) = cx.read_from_clipboard() {
            if let Some(image) = clipboard.entries().iter().find_map(|entry| match entry {
                gpui::ClipboardEntry::Image(image) => Some(image),
                _ => None,
            }) {
                match ImageAsset::from_bytes(&image.bytes) {
                    Ok(asset) => {
                        // An image copied from Romo pastes as the same object, crop and all.
                        let copied = self.clipboard.len() == 1
                            && self.clipboard[0]
                                .image
                                .as_ref()
                                .is_some_and(|old| old.matches(self.clipboard[0].crop, &asset));
                        if copied {
                            self.paste_objects();
                        } else {
                            let center = self.view.to_world(self.canvas.center(), self.canvas);
                            let center = center + self.paste_offset(center);
                            self.add_image(asset, center);
                        }
                    }
                    Err(error) => self.notify(error, true),
                }
                return;
            }
            if let Some(data) = clipboard
                .metadata()
                .filter(|_| clipboard.text().as_deref() == Some("Romo objects"))
            {
                self.clipboard.clear();
                if let Ok(items) = serde_json::from_str::<Vec<Item>>(data) {
                    let candidate = Board {
                        title: String::new(),
                        next_id: items
                            .iter()
                            .map(|i| i.id)
                            .max()
                            .unwrap_or(0)
                            .saturating_add(1),
                        items: items.clone(),
                    };
                    if candidate.validate().is_ok() {
                        self.clipboard = items;
                    }
                }
            } else {
                self.clipboard.clear();
                return;
            }
        }
        self.paste_objects();
    }
    fn paste_objects(&mut self) {
        let Some(bounds) = self.clipboard.iter().map(Item::rect).reduce(Rect::union) else {
            return;
        };
        let before = self.board.clone();
        let offset = self.paste_offset(bounds.center());
        self.selected = self.board.paste(&self.clipboard, offset);
        self.tool = Tool::Select;
        self.show_properties = false;
        self.changed(before);
    }
    /// How far to move pasted objects centered at `center`: under the pointer
    /// while it is over the board, or else beside the originals. Pasting again
    /// in the same spot cascades the copies, so none hides another.
    fn paste_offset(&mut self, center: Pos2) -> Vec2 {
        let spot = self
            .pointer
            .filter(|&pointer| {
                self.canvas.contains(pointer)
                    && !CONTROLS.with_borrow(|controls| {
                        controls.iter().any(|(rect, _)| rect.contains(pointer))
                    })
            })
            .map(|pointer| self.view.to_world(pointer, self.canvas));
        self.paste_count = if spot.is_some() && spot != self.paste_spot {
            0
        } else {
            self.paste_count + 1
        };
        self.paste_spot = spot;
        let cascade = Vec2::splat(self.paste_count as f32 * 32.0);
        spot.map_or(cascade, |spot| spot - center + cascade)
    }
    fn begin_edit(&mut self, target: EditTarget, window: &mut Window, cx: &mut Context<Self>) {
        self.apply_edit(window, cx);
        self.cancel_gesture();
        self.show_boards = false;
        let text = match target {
            EditTarget::Title => self.board.title.clone(),
            EditTarget::Object(id) => self
                .board
                .get(id)
                .map_or_else(String::new, |i| i.text.clone()),
        };
        let input = cx.new(|cx| TextInput::new(text, cx));
        let subscription = if let EditTarget::Object(id) = target {
            Some(
                cx.subscribe(&input, move |app, input, _: &TextChanged, cx| {
                    if app.editing.as_ref().is_some_and(
                        |edit| matches!(edit.target, EditTarget::Object(current) if current == id),
                    ) {
                        app.live_text(id, input.read(cx).content.clone());
                        cx.notify();
                    }
                }),
            )
        } else {
            None
        };
        window.focus(&input.read(cx).focus);
        self.editing = Some(Editing {
            target,
            input,
            before: self.board.clone(),
            _subscription: subscription,
        });
        self.file_menu = false;
        self.show_boards = false;
    }
    fn live_text(&mut self, id: u64, text: String) {
        if let Some(item) = self.board.get_mut(id)
            && item.text != text
        {
            item.text = text;
            self.dirty = true;
            self.session_dirty = true;
        }
    }
    fn commit_edit(&mut self, cx: &gpui::App) {
        if let Some(edit) = self.editing.take() {
            let text = edit.input.read(cx).content.clone();
            match edit.target {
                EditTarget::Title => {
                    self.board.title = text.replace('\n', " ").chars().take(256).collect()
                }
                EditTarget::Object(id) => {
                    self.live_text(id, text);
                    // Text boxes left empty disappear; a brand-new one leaves no undo step.
                    if self
                        .board
                        .get(id)
                        .is_some_and(|item| item.kind == Kind::Text && item.text.is_empty())
                    {
                        self.board.remove(&HashSet::from([id]));
                        self.selected.remove(&id);
                        if let Some(before) = self.undo.back()
                            && before.items == self.board.items
                            && before.title == self.board.title
                        {
                            self.board = self.undo.pop_back().unwrap_or_default();
                            return;
                        }
                    }
                }
            }
            self.changed(edit.before);
        }
    }
    fn apply_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.commit_edit(cx);
        window.focus(&self.focus);
    }
    fn sync_inline_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(edit) = &self.editing else {
            return;
        };
        let EditTarget::Object(id) = edit.target else {
            return;
        };
        let input = edit.input.clone();
        let minimum_height = edit.before.get(id).map_or(40.0, |item| item.size.y);
        self.live_text(id, input.read(cx).content.clone());
        let Some(item) = self.board.get_mut(id) else {
            return;
        };
        if item.kind == Kind::Text && item.auto_width {
            let size = fitted_text_size(window, item);
            if item.size != size {
                item.size = size;
                self.session_dirty = true;
            }
        } else if item.kind == Kind::Text {
            let run = gpui::TextRun {
                len: item.text.len(),
                font: gpui::font("Inter Variable"),
                color: rgb(item.color.rgb()).into(),
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            if let Ok(lines) = window.text_system().shape_text(
                item.text.clone().into(),
                px(item.font_size),
                &[run],
                Some(px(item.size.x)),
                None,
            ) {
                let height: f32 = lines
                    .iter()
                    .map(|line| f32::from(line.size(px(item.font_size * 1.35)).height))
                    .sum();
                let height = minimum_height.max(height).min(100_000.0);
                if item.size.y != height {
                    item.size.y = height;
                    self.session_dirty = true;
                }
            }
        }
        let luminance = item.color.r() as f32 * 0.299
            + item.color.g() as f32 * 0.587
            + item.color.b() as f32 * 0.114;
        let style = EditorStyle {
            font_size: if item.kind == Kind::Frame {
                13.0
            } else {
                item.font_size
            } * self.view.zoom,
            color: if matches!(item.kind, Kind::Text | Kind::Frame) {
                item.color.rgb()
            } else if luminance > 140.0 {
                SURFACE
            } else {
                FG
            },
            centered: !matches!(item.kind, Kind::Text | Kind::Frame),
            // Auto-width text grows instead of wrapping.
            wrap: !item.auto_width,
        };
        input.update(cx, |input, cx| {
            if input.style != style {
                input.style = style;
                cx.notify();
            }
        });
    }
    fn inline_editor(&self) -> Option<Div> {
        let edit = self.editing.as_ref()?;
        let EditTarget::Object(id) = edit.target else {
            return None;
        };
        let item = self.board.get(id)?;
        let mut rect = self.view.screen_rect(item.rect(), self.canvas);
        if item.kind == Kind::Frame {
            rect = Rect::from_min_size(
                rect.min - vec2(0.0, 30.0 * self.view.zoom),
                vec2(rect.width(), 24.0 * self.view.zoom),
            );
        } else if item.kind != Kind::Text {
            rect = rect.shrink(16.0 * self.view.zoom);
        } else if item.auto_width {
            // Room past the last glyph for the caret, so the editor never scrolls.
            rect.max.x += 8.0;
        }
        Some(
            div()
                .absolute()
                .left(px(rect.min.x - self.canvas.min.x))
                .top(px(rect.min.y - self.canvas.min.y))
                .w(px(rect.width().max(10.0)))
                .h(px(rect.height().max(10.0)))
                .child(edit.input.clone()),
        )
    }
    fn action(&mut self, action: Action, window: &mut Window, cx: &mut Context<Self>) {
        if !matches!(action, Action::ApplyEdit | Action::CancelEdit)
            && self
                .editing
                .as_ref()
                .is_some_and(|edit| matches!(edit.target, EditTarget::Object(_)))
        {
            self.apply_edit(window, cx);
        }
        match action {
            Action::Tool(tool) => {
                self.cancel_gesture();
                self.tool = tool;
                self.eraser = None;
                self.file_menu = false;
                window.focus(&self.focus);
            }
            Action::Undo => {
                self.cancel_gesture();
                if let Some(board) = self.undo.pop_back() {
                    self.redo.push(std::mem::replace(&mut self.board, board));
                    self.selected.retain(|id| self.board.get(*id).is_some());
                    self.dirty = true;
                    self.session_dirty = true;
                }
            }
            Action::Redo => {
                self.cancel_gesture();
                if let Some(board) = self.redo.pop() {
                    self.undo
                        .push_back(std::mem::replace(&mut self.board, board));
                    self.selected.retain(|id| self.board.get(*id).is_some());
                    self.dirty = true;
                    self.session_dirty = true;
                }
            }
            Action::New => {
                self.file_menu = false;
                self.request(BoardCommand::New);
            }
            Action::Open => {
                self.file_menu = false;
                self.request(BoardCommand::Open);
            }
            Action::Save => {
                self.file_menu = false;
                self.save(false);
            }
            Action::SaveAs => {
                self.file_menu = false;
                self.save(true);
            }
            Action::Export => {
                self.file_menu = false;
                self.export();
            }
            Action::Copy => self.copy(cx),
            Action::Paste => self.paste(cx),
            Action::Duplicate => {
                let before = self.board.clone();
                let objects: Vec<_> = self
                    .board
                    .items
                    .iter()
                    .filter(|i| self.selected.contains(&i.id))
                    .cloned()
                    .collect();
                self.selected = self.board.paste(&objects, Vec2::splat(32.0));
                self.changed(before);
            }
            Action::Delete => {
                let before = self.board.clone();
                self.board.remove(&self.selected);
                self.selected.clear();
                self.changed(before);
            }
            Action::Front | Action::Back => {
                let before = self.board.clone();
                let (selected, others): (Vec<_>, Vec<_>) = self
                    .board
                    .items
                    .drain(..)
                    .partition(|i| self.selected.contains(&i.id));
                self.board.items = if matches!(action, Action::Front) {
                    others.into_iter().chain(selected).collect()
                } else {
                    selected.into_iter().chain(others).collect()
                };
                self.changed(before);
            }
            Action::Zoom(factor) => {
                self.view.zoom_at(factor, self.canvas.center(), self.canvas);
                self.session_dirty = true;
            }
            Action::ResetZoom => {
                self.view
                    .zoom_at(1.0 / self.view.zoom, self.canvas.center(), self.canvas);
                self.session_dirty = true;
            }
            Action::Fit => {
                self.view.fit(self.board.bounds(), self.canvas);
                self.session_dirty = true;
            }
            Action::Grid => self.show_grid = !self.show_grid,
            Action::Map => self.show_map = !self.show_map,
            Action::Declutter => {
                let now = Instant::now();
                let opacity = self.chrome_opacity(now);
                self.declutter = !self.declutter;
                self.chrome_fade = Some((now, opacity));
                window.refresh();
            }
            Action::Snap => {
                self.snap = !self.snap;
                if let Some(path) = &self.settings_path
                    && let Err(error) = storage::save_settings(
                        path,
                        &storage::Settings {
                            snapping: self.snap,
                        },
                    )
                {
                    self.notify(error, true);
                }
            }
            Action::Boards => {
                self.refresh_library();
                self.show_boards = !self.show_boards;
                self.file_menu = false;
                self.show_settings = false;
            }
            Action::Board(index) => {
                if let Some(entry) = self.library.get(index).cloned() {
                    self.cancel_gesture();
                    self.session_dirty = true;
                    self.persist();
                    if self.smoke || !self.session_dirty {
                        self.load(entry.path);
                        self.persist();
                        self.show_boards = false;
                    }
                }
            }
            Action::Settings => {
                self.show_settings = !self.show_settings;
                self.show_boards = false;
                self.file_menu = false;
            }
            Action::Colors => {
                self.show_colors = !self.show_colors;
                self.show_sizes = false;
            }
            Action::Sizes => {
                self.show_sizes = !self.show_sizes;
                self.show_colors = false;
            }
            Action::ImportImage => {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("Image", &["png", "jpg", "jpeg", "webp", "gif", "bmp"])
                    .pick_file()
                {
                    let result = std::fs::read(path)
                        .map_err(|e| e.to_string())
                        .and_then(|bytes| ImageAsset::from_bytes(&bytes));
                    match result {
                        Ok(asset) => {
                            let center = self.view.to_world(self.canvas.center(), self.canvas);
                            self.add_image(asset, center)
                        }
                        Err(error) => self.notify(error, true),
                    }
                }
            }
            Action::Help => self.show_help = !self.show_help,
            Action::FileMenu => {
                self.file_menu = !self.file_menu;
                self.show_boards = false;
                self.show_settings = false;
            }
            Action::HideProperties => self.show_properties = false,
            Action::Edit => {
                if let Some(id) = self.selected.iter().next().copied().filter(|id| {
                    self.board
                        .get(*id)
                        .is_some_and(|i| !matches!(i.kind, Kind::Image | Kind::Pen | Kind::Arrow))
                }) {
                    self.begin_edit(EditTarget::Object(id), window, cx);
                }
            }
            Action::Title => self.begin_edit(EditTarget::Title, window, cx),
            Action::Color(color) => {
                self.color = color;
                let before = self.board.clone();
                for item in &mut self.board.items {
                    if self.selected.contains(&item.id) {
                        item.color = color;
                    }
                }
                self.changed(before);
            }
            Action::Font(amount)
            | Action::Width(amount)
            | Action::Height(amount)
            | Action::LineWidth(amount) => {
                let before = self.board.clone();
                for item in &mut self.board.items {
                    if self.selected.contains(&item.id) {
                        match action {
                            Action::Font(_) => {
                                item.font_size = (item.font_size + amount).clamp(8.0, 160.0);
                                if item.auto_width {
                                    item.size = fitted_text_size(window, item);
                                }
                            }
                            Action::Width(_) => item.resize(vec2(
                                (item.size.x + amount).clamp(40.0, 5000.0),
                                item.size.y,
                            )),
                            Action::Height(_) => item.resize(vec2(
                                item.size.x,
                                (item.size.y + amount).clamp(40.0, 5000.0),
                            )),
                            Action::LineWidth(_) => {
                                // Change the visible width of shrunken drawings by `amount`.
                                let shrink = item.scale.unwrap_or(1.0).min(1.0);
                                item.stroke_width =
                                    (item.stroke_width + amount / shrink).clamp(0.5, 24.0)
                            }
                            _ => {}
                        }
                    }
                }
                self.changed(before);
            }
            Action::ApplyEdit => self.apply_edit(window, cx),
            Action::CancelEdit => {
                if self
                    .editing
                    .as_ref()
                    .is_some_and(|edit| matches!(edit.target, EditTarget::Object(_)))
                {
                    self.commit_edit(cx);
                } else {
                    self.editing = None;
                }
                window.focus(&self.focus);
            }
        }
        cx.notify();
    }

    fn button(
        &self,
        id: &str,
        title: &str,
        action: Action,
        primary: bool,
        cx: &Context<Self>,
    ) -> Stateful<Div> {
        div()
            .id(SharedString::from(id.to_owned()))
            .h_9()
            .px_3()
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(4.0))
            .text_size(px(13.0))
            .cursor_pointer()
            .occlude()
            .bg(rgb(if primary { FG } else { SURFACE }))
            .text_color(rgb(if primary { BASE } else { FG }))
            .hover(move |style| style.bg(rgb(if primary { 0xd4d4d8 } else { BORDER })))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.action(action, window, cx);
                cx.stop_propagation();
            }))
            .child(control(Some(action)))
            .child(title.to_owned())
    }
    fn icon_button(
        &self,
        id: &str,
        icon: Icon,
        action: Action,
        active: bool,
        hint: &str,
        cx: &Context<Self>,
    ) -> Stateful<Div> {
        let hint = hint.to_owned();
        div()
            .id(SharedString::from(id.to_owned()))
            .size_9()
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(4.0))
            .cursor_pointer()
            .occlude()
            .bg(rgb(if active { FG } else { SURFACE }))
            .hover(move |s| s.bg(rgb(if active { 0xd4d4d8 } else { BORDER })))
            .tooltip(move |_, cx| {
                cx.new(|_| Tooltip {
                    title: hint.clone(),
                })
                .into()
            })
            .on_click(cx.listener(move |this, _, window, cx| {
                this.action(action, window, cx);
                cx.stop_propagation();
            }))
            .child(control(Some(action)))
            .child(icon_view(icon, if active { BASE } else { FG }, 20.0))
    }
    /// Save, file and board controls with undo and redo, floating in the top-left corner.
    fn top_bar(&self, cx: &Context<Self>) -> Div {
        let title = if self.board.title.is_empty() {
            "Untitled board"
        } else {
            &self.board.title
        };
        panel()
            .absolute()
            .top_6()
            .left_6()
            .p_1()
            .flex()
            .items_center()
            .gap_1()
            .child(self.icon_button("save", Icon::Save, Action::Save, false, "Save · Ctrl+S", cx))
            .child(self.icon_button(
                "file",
                Icon::Menu,
                Action::FileMenu,
                self.file_menu,
                "File",
                cx,
            ))
            .child(div().w(px(1.0)).h_6().mx_1().bg(rgb(BORDER)))
            .child(
                div()
                    .id("board-title")
                    .h_9()
                    .px_2()
                    .flex()
                    .items_center()
                    .gap_2()
                    .rounded(px(4.0))
                    .text_size(px(13.0))
                    .cursor_pointer()
                    .bg(rgb(if self.show_boards { BORDER } else { SURFACE }))
                    .hover(|style| style.bg(rgb(BORDER)))
                    .tooltip(|_, cx| {
                        cx.new(|_| Tooltip {
                            title: "Choose a board".into(),
                        })
                        .into()
                    })
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.action(Action::Boards, window, cx);
                        cx.stop_propagation();
                    }))
                    .child(control(Some(Action::Boards)))
                    .child(icon_view(Icon::Logo, FG, 18.0))
                    .child(div().max_w(px(160.0)).truncate().child(title.to_owned()))
                    .child(icon_view(Icon::Chevron, MUTED, 12.0)),
            )
            .child(div().w(px(1.0)).h_6().mx_1().bg(rgb(BORDER)))
            .child(self.icon_button("undo", Icon::Undo, Action::Undo, false, "Undo · Ctrl+Z", cx))
            .child(self.icon_button(
                "redo",
                Icon::Redo,
                Action::Redo,
                false,
                "Redo · Ctrl+Shift+Z",
                cx,
            ))
    }
    fn toolbar(&self, window: &Window, cx: &Context<Self>) -> Div {
        // Twelve buttons, two dividers and the panel padding take 444 px, plus 17
        // gaps. Tighten the gaps when the tools would not fit between the corners.
        // The viewport is current during render; the board canvas updates in paint.
        let height = f32::from(window.viewport_size().height);
        let gap = px(if height - 2.0 * PANEL_REACH - 16.0 < 444.0 + 17.0 * 4.0 {
            2.0
        } else {
            4.0
        });
        let mut tools = panel().p_1().flex().flex_col().gap(gap);
        for tool in [
            Tool::Select,
            Tool::Hand,
            Tool::Note,
            Tool::Text,
            Tool::Rectangle,
            Tool::Ellipse,
            Tool::Diamond,
            Tool::Arrow,
            Tool::Pen,
            Tool::Eraser,
            Tool::Frame,
        ] {
            tools = tools.child(self.icon_button(
                &format!("tool-{}", tool.key()),
                tool.icon(),
                Action::Tool(tool),
                self.tool == tool,
                &format!("{} · {}", tool.label(), tool.key()),
                cx,
            ));
            if tool == Tool::Hand {
                tools = tools.child(div().h(px(1.0)).w_full().bg(rgb(BORDER)).my(gap));
            }
        }
        let tools = tools
            .child(div().h(px(1.0)).w_full().bg(rgb(BORDER)).my(gap))
            .child(self.icon_button(
                "import-image",
                Icon::Image,
                Action::ImportImage,
                false,
                "Add image · paste with Ctrl+V",
                cx,
            ));
        // Center the tools between the top and bottom corner panels.
        div()
            .absolute()
            .left_6()
            .top(px(PANEL_REACH))
            .bottom(px(PANEL_REACH))
            .flex()
            .flex_col()
            .justify_center()
            .child(tools)
    }
    fn properties(&self, cx: &Context<Self>) -> Div {
        let single = self
            .selected
            .iter()
            .next()
            .and_then(|id| self.board.get(*id));
        let screen = self
            .selection_bounds()
            .map(|r| self.view.screen_rect(r, self.canvas))
            .unwrap_or(self.canvas);
        let left = (screen.center().x - self.canvas.min.x - 200.0)
            .clamp(80.0, (self.canvas.width() - 460.0).max(80.0));
        // Prefer the space above the selection, but stay below the top bar.
        let highest = PANEL_REACH + 8.0;
        let above = screen.min.y - self.canvas.min.y - 56.0;
        let top = if above >= highest {
            above
        } else {
            screen.max.y - self.canvas.min.y + 12.0
        };
        let mut bar = panel().p_1().flex().items_center().gap_1();
        if let Some(item) = single {
            bar = bar.child(div().px_2().child(label(item.kind.label())));
        }
        if single.is_some_and(|i| i.kind != Kind::Image) {
            bar = bar.child(self.icon_button(
                "object-colors",
                Icon::Palette,
                Action::Colors,
                self.show_colors,
                "Color",
                cx,
            ));
        }
        if single.is_some_and(|i| !matches!(i.kind, Kind::Image | Kind::Pen | Kind::Arrow)) {
            bar = bar.child(self.icon_button(
                "edit-content",
                Icon::Text,
                Action::Edit,
                false,
                "Edit text",
                cx,
            ));
        }
        bar = bar
            .child(self.icon_button(
                "object-size",
                Icon::Fit,
                Action::Sizes,
                self.show_sizes,
                "Size and weight",
                cx,
            ))
            .child(div().w(px(1.0)).h_6().mx_1().bg(rgb(BORDER)))
            .child(self.icon_button(
                "object-front",
                Icon::Front,
                Action::Front,
                false,
                "Bring forward",
                cx,
            ))
            .child(self.icon_button(
                "object-back",
                Icon::Back,
                Action::Back,
                false,
                "Send backward",
                cx,
            ))
            .child(self.icon_button(
                "object-copy",
                Icon::Copy,
                Action::Duplicate,
                false,
                "Duplicate",
                cx,
            ))
            .child(self.icon_button(
                "object-trash",
                Icon::Trash,
                Action::Delete,
                false,
                "Delete",
                cx,
            ))
            .child(self.icon_button(
                "hide-props",
                Icon::Close,
                Action::HideProperties,
                false,
                "Close",
                cx,
            ));
        let mut wrapper = div()
            .absolute()
            .left(px(left))
            .top(px(
                top.clamp(highest, (self.canvas.height() - 160.0).max(highest))
            ))
            .flex()
            .flex_col()
            .gap_2()
            .child(bar);
        if self.show_colors {
            let mut colors = panel().p_2().flex().gap_2();
            for (index, color) in PALETTE.iter().copied().enumerate() {
                colors = colors.child(
                    div()
                        .id(("context-color", index))
                        .size_6()
                        .rounded(px(4.0))
                        .bg(rgb(color.rgb()))
                        .border_1()
                        .border_color(rgb(if single.is_some_and(|i| i.color == color) {
                            FG
                        } else {
                            BORDER
                        }))
                        .cursor_pointer()
                        .occlude()
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.action(Action::Color(color), window, cx)
                        }))
                        .child(control(Some(Action::Color(color)))),
                );
            }
            wrapper = wrapper.child(colors);
        }
        if self.show_sizes
            && let Some(item) = single
        {
            let mut controls = panel().p_2().flex().flex_col().gap_2();
            let rows = if matches!(item.kind, Kind::Pen | Kind::Arrow) {
                vec![(
                    format!("Line · {:.1}", item.line_width()),
                    Action::LineWidth(-0.5),
                    Action::LineWidth(0.5),
                )]
            } else {
                let mut rows = vec![
                    (
                        format!("Width · {:.0}", item.size.x),
                        Action::Width(-20.0),
                        Action::Width(20.0),
                    ),
                    (
                        format!("Height · {:.0}", item.size.y),
                        Action::Height(-20.0),
                        Action::Height(20.0),
                    ),
                ];
                if item.kind != Kind::Image {
                    rows.push((
                        format!("Text · {:.0}", item.font_size),
                        Action::Font(-2.0),
                        Action::Font(2.0),
                    ));
                }
                rows
            };
            for (index, (title, minus, plus)) in rows.into_iter().enumerate() {
                controls = controls.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .child(label(title))
                        .child(self.icon_button(
                            &format!("size-down-{index}"),
                            Icon::Minus,
                            minus,
                            false,
                            "Decrease",
                            cx,
                        ))
                        .child(self.icon_button(
                            &format!("size-up-{index}"),
                            Icon::Plus,
                            plus,
                            false,
                            "Increase",
                            cx,
                        )),
                );
            }
            wrapper = wrapper.child(controls);
        }
        wrapper
    }
    fn minimap(&self, cx: &Context<Self>) -> Div {
        let entity = cx.entity();
        let paint_entity = entity.clone();
        panel()
            .absolute()
            .right_6()
            .bottom(px(84.0))
            .p_3()
            .w(px(232.0))
            .h(px(156.0))
            .child(
                div()
                    .id("minimap")
                    .size_full()
                    .cursor_pointer()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, event: &MouseDownEvent, _, cx| {
                            let r = this.mini_rect;
                            let b = this.mini_world;
                            let scale = (r.width() / b.width()).min(r.height() / b.height());
                            let origin = r.center() - b.center().to_vec2() * scale;
                            let world = (pos2(event.position.x.into(), event.position.y.into())
                                - origin)
                                / scale;
                            this.view.pan = -world * this.view.zoom;
                            this.session_dirty = true;
                            cx.notify();
                            cx.stop_propagation();
                        }),
                    )
                    .child(
                        canvas(
                            move |bounds, _, cx| {
                                entity.update(cx, |this, _| {
                                    this.mini_rect = world_rect(bounds);
                                    let visible = Rect::from_min_max(
                                        this.view.to_world(this.canvas.min, this.canvas),
                                        this.view.to_world(this.canvas.max, this.canvas),
                                    );
                                    this.mini_world = this
                                        .board
                                        .bounds()
                                        .unwrap_or(visible)
                                        .union(visible)
                                        .expand(40.0);
                                });
                            },
                            move |_, _, window, cx| {
                                let this = paint_entity.read(cx);
                                let r = this.mini_rect;
                                let b = this.mini_world;
                                let scale = (r.width() / b.width()).min(r.height() / b.height());
                                let origin = r.center() - b.center().to_vec2() * scale;
                                let map = |p: Pos2| origin + p.to_vec2() * scale;
                                for item in &this.board.items {
                                    let rect = this.board.item_rect(item);
                                    let rect = Rect::from_min_max(map(rect.min), map(rect.max));
                                    if matches!(item.kind, Kind::Frame | Kind::Arrow | Kind::Pen) {
                                        paint::outline(window, rect, MUTED);
                                    } else {
                                        paint::rect(
                                            window,
                                            rect,
                                            if item.kind == Kind::Text {
                                                MUTED
                                            } else {
                                                item.color.rgb()
                                            },
                                            None,
                                            1.0,
                                        );
                                    }
                                }
                                let visible = Rect::from_min_max(
                                    this.view.to_world(this.canvas.min, this.canvas),
                                    this.view.to_world(this.canvas.max, this.canvas),
                                );
                                let v = Rect::from_min_max(map(visible.min), map(visible.max));
                                paint::outline(window, v, FG);
                            },
                        )
                        .size_full(),
                    ),
            )
    }
    fn file_dropdown(&self, cx: &Context<Self>) -> Div {
        // Below the File button in the top bar.
        panel()
            .absolute()
            .top(px(PANEL_REACH + 8.0))
            .left(px(69.0))
            .w(px(240.0))
            .p_2()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                self.button("file-rename", "Rename board…", Action::Title, false, cx)
                    .justify_start(),
            )
            .child(self.menu_item("file-new", "New board", "Ctrl+N", Action::New, cx))
            .child(self.menu_item("file-open", "Open…", "Ctrl+O", Action::Open, cx))
            .child(div().h(px(1.0)).bg(rgb(BORDER)).my_1())
            .child(self.menu_item("file-save", "Save", "Ctrl+S", Action::Save, cx))
            .child(self.menu_item(
                "file-save-as",
                "Export board…",
                "Ctrl+Shift+S",
                Action::SaveAs,
                cx,
            ))
            .child(self.menu_item("file-export", "Export SVG…", "Ctrl+E", Action::Export, cx))
    }
    fn menu_item(
        &self,
        id: &str,
        title: &str,
        shortcut: &str,
        action: Action,
        cx: &Context<Self>,
    ) -> Stateful<Div> {
        self.button(id, title, action, false, cx)
            .justify_between()
            .child(label(shortcut.to_owned()))
    }
    fn help(&self, cx: &Context<Self>) -> Div {
        let mut help = panel()
            .p_6()
            .w(px(540.0))
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .flex()
                    .justify_between()
                    .items_center()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .child(icon_view(Icon::Logo, FG, 28.0))
                            .child(
                                div()
                                    .text_size(px(20.0))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .child("A little room for big ideas"),
                            ),
                    )
                    .child(self.icon_button(
                        "close-help",
                        Icon::Close,
                        Action::Help,
                        false,
                        "Close",
                        cx,
                    )),
            )
            .child(label("Everything lives on your device."));
        let mut shortcuts = div().flex().flex_col().gap(px(6.0));
        for (action, key) in [
            ("Select / Hand", "V / H"),
            ("Sticky / Text", "N / T"),
            ("Rectangle / Ellipse / Diamond", "R / O / D"),
            ("Connector / Pen / Eraser / Frame", "L / P or B / E / F"),
            ("Pan", "Space + drag / middle drag"),
            ("Zoom", "Wheel / Ctrl + trackpad"),
            ("Reset / Fit board", "1 / 2"),
            ("Select several objects", "Shift + click / drag a box"),
            ("Resize freely / Crop an image", "Shift / Alt + drag a corner"),
            ("Edit text", "Double-click / Enter"),
            ("Copy / Paste / Duplicate", "Ctrl+C / Ctrl+V / Ctrl+D"),
            ("Delete", "X / Delete"),
            ("Undo / Redo", "Ctrl+Z / Ctrl+Shift+Z"),
            ("New / Open / Save", "Ctrl+N / Ctrl+O / Ctrl+S"),
            ("Save as / Export SVG", "Ctrl+Shift+S / Ctrl+E"),
            ("Grid / Minimap", "G / M"),
            ("Declutter / Show controls", "Y"),
        ] {
            shortcuts = shortcuts.child(
                div()
                    .flex()
                    .justify_between()
                    .text_size(px(13.0))
                    .child(action)
                    .child(label(key)),
            );
        }
        help = help
            .child(shortcuts)
            .child(label(
                "Right-click an object for its toolbar. Hold Alt to bypass snapping.",
            ))
            .child(label("On macOS, use ⌘ in place of Ctrl."));
        modal().child(help)
    }
    fn edit_modal(&self, editing: &Editing, cx: &Context<Self>) -> Div {
        let title = match editing.target {
            EditTarget::Title => "Rename board",
            EditTarget::Object(_) => "Edit text",
        };
        modal().child(
            panel()
                .w(px(520.0))
                .p_6()
                .flex()
                .flex_col()
                .gap_4()
                .child(
                    div()
                        .text_size(px(20.0))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .child(title),
                )
                .child(
                    div()
                        .h(px(240.0))
                        .w_full()
                        .p_3()
                        .rounded(px(4.0))
                        .bg(rgb(BASE))
                        .border_1()
                        .border_color(rgb(0x3f3f46))
                        .child(editing.input.clone()),
                )
                .child(label("Ctrl+Enter to apply · Esc to cancel"))
                .child(
                    div()
                        .flex()
                        .justify_end()
                        .gap_2()
                        .child(self.button("cancel-edit", "Cancel", Action::CancelEdit, false, cx))
                        .child(self.button("apply-edit", "Apply", Action::ApplyEdit, true, cx)),
                ),
        )
    }
}

fn filename(title: &str) -> String {
    let s: String = title
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, ' ' | '_' | '-') {
                c
            } else {
                '_'
            }
        })
        .take(80)
        .collect();
    if s.trim().is_empty() {
        "Untitled board".into()
    } else {
        s
    }
}
fn world_rect(bounds: Bounds<Pixels>) -> Rect {
    Rect::from_min_size(
        pos2(bounds.origin.x.into(), bounds.origin.y.into()),
        vec2(bounds.size.width.into(), bounds.size.height.into()),
    )
}
/// The box of auto-width text: as wide as its longest line, one row per line.
fn fitted_text_size(window: &Window, item: &Item) -> Vec2 {
    let run = gpui::TextRun {
        len: item.text.len(),
        font: gpui::font("Inter Variable"),
        color: rgb(item.color.rgb()).into(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let lines = window
        .text_system()
        .shape_text(
            item.text.clone().into(),
            px(item.font_size),
            &[run],
            None,
            None,
        )
        .unwrap_or_default();
    let widest = lines
        .iter()
        .map(|line| f32::from(line.width()))
        .fold(0.0, f32::max);
    // Room for the caret while empty, and a pixel so the last glyph never wraps.
    vec2(
        widest.max(item.font_size * 0.5) + 1.0,
        lines.len().max(1) as f32 * (item.font_size * 1.35),
    )
}
fn icon_view(icon: Icon, color: u32, size: f32) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |b, _, window, cx| paint::icon(window, cx, world_rect(b), icon, color),
    )
    .size(px(size))
}
fn label(text: impl Into<SharedString>) -> Div {
    div()
        .text_size(px(13.0))
        .text_color(rgb(MUTED))
        .child(text.into())
}

thread_local! {
    /// Controls painted this frame, topmost last, so tablet pens can reach them.
    static CONTROLS: RefCell<Vec<(Rect, Option<Action>)>> = const { RefCell::new(Vec::new()) };
}
/// Records where a control was painted: a button with its action, or a panel
/// that keeps pens off the board beneath it.
fn control(action: Option<Action>) -> impl IntoElement {
    canvas(
        move |bounds, _, _| {
            CONTROLS.with_borrow_mut(|controls| controls.push((world_rect(bounds), action)))
        },
        |_, _, _, _| {},
    )
    .absolute()
    .size_full()
}
fn surface() -> Div {
    div()
        .occlude()
        .bg(rgb(SURFACE))
        .border_1()
        .border_color(rgb(BORDER))
        .rounded(px(8.0))
}
fn panel() -> Div {
    surface().child(control(None))
}
fn modal() -> Div {
    let mut bg = rgb(BASE);
    bg.a = 0.85;
    div()
        .absolute()
        .inset_0()
        .flex()
        .items_center()
        .justify_center()
        .bg(bg)
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(control(None))
}

struct Tooltip {
    title: String,
}
impl Render for Tooltip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        surface()
            .px_3()
            .py_2()
            .text_size(px(13.0))
            .text_color(rgb(FG))
            .child(self.title.clone())
    }
}

impl Render for Romo {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        CONTROLS.with_borrow_mut(Vec::clear);
        let now = Instant::now();
        if self
            .chrome_fade
            .is_some_and(|(start, _)| now.duration_since(start) >= DECLUTTER_FADE)
        {
            self.chrome_fade = None;
            if self.declutter {
                self.file_menu = false;
                self.show_boards = false;
                self.show_settings = false;
                self.show_help = false;
            }
        }
        let chrome_opacity = self.chrome_opacity(now);
        let chrome_visible = !self.declutter || self.chrome_fade.is_some();
        if self.chrome_fade.is_some() {
            window.request_animation_frame();
        }
        self.step_bench(window, cx);
        self.sync_inline_editor(window, cx);
        let eraser = self
            .eraser
            .filter(|_| {
                self.tool == Tool::Eraser
                    && !self.space
                    && !matches!(self.gesture, Some(Gesture::Pan { .. }))
            })
            .map(|pointer| (pointer, ERASER_RADIUS));
        while self
            .trail
            .front()
            .is_some_and(|(_, at)| now.duration_since(*at) >= TRAIL)
        {
            self.trail.pop_front();
        }
        let trail: Vec<(Pos2, f32)> = self
            .trail
            .iter()
            .map(|(point, at)| {
                let age = now.duration_since(*at).as_secs_f32() / TRAIL.as_secs_f32();
                (self.view.to_screen(*point, self.canvas), 1.0 - age)
            })
            .collect();
        if self.zoom_seen.0 != self.view.zoom {
            self.zoom_seen = (self.view.zoom, now);
        }
        let settled = now.duration_since(self.zoom_seen.1) >= ZOOM_SETTLE;
        // Keep painting while the trail fades and until text settles after a zoom.
        if !settled || !trail.is_empty() {
            window.request_animation_frame();
        }
        // The handle being dragged or hovered shows which way it resizes.
        let corner = match &self.gesture {
            Some(
                Gesture::Resize { corner, .. }
                | Gesture::Crop { corner, .. }
                | Gesture::Scale { corner, .. },
            ) => Some(*corner),
            Some(_) => None,
            None => self.hovered.filter(|corner| {
                self.handles()
                    .is_some_and(|(_, corners)| corners.contains(corner))
            }),
        };
        let entity = cx.entity();
        let paint_entity = entity.clone();
        let board_canvas = div()
            .id("board-canvas")
            .absolute()
            .inset_0()
            .overflow_hidden()
            .cursor(if self.tool == Tool::Hand || self.space {
                gpui::CursorStyle::OpenHand
            } else if let Some(corner) = corner {
                match corner {
                    Corner::TopLeft | Corner::BottomRight => {
                        gpui::CursorStyle::ResizeUpLeftDownRight
                    }
                    Corner::TopRight | Corner::BottomLeft => {
                        gpui::CursorStyle::ResizeUpRightDownLeft
                    }
                }
            } else if self.tool == Tool::Select {
                gpui::CursorStyle::Arrow
            } else {
                gpui::CursorStyle::Crosshair
            })
            .on_mouse_down(MouseButton::Left, cx.listener(Self::down))
            .on_mouse_down(MouseButton::Middle, cx.listener(Self::down))
            .on_mouse_down(MouseButton::Right, cx.listener(Self::context_menu))
            .on_scroll_wheel(cx.listener(Self::scroll))
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                // Panels cover the board: hide the eraser outline while over them.
                if !hovered && this.eraser.take().is_some() {
                    cx.notify();
                }
            }))
            .child(
                canvas(
                    move |bounds, _, cx| {
                        entity.update(cx, |this, cx| {
                            let previous = this.canvas;
                            this.canvas = world_rect(bounds);
                            if this.fit_pending {
                                this.view.fit(this.board.bounds(), this.canvas);
                                this.fit_pending = false;
                                this.session_dirty = true;
                            }
                            if previous != this.canvas {
                                cx.notify();
                            }
                        });
                    },
                    move |_, _, window, cx| {
                        // Borrow the board and stroke cache for this paint instead of
                        // cloning them; they go back once the frame is painted.
                        let (board, mut cache, view, canvas, selected, guides, overlay) =
                            paint_entity.update(cx, |this, _| {
                                let editing =
                                    this.editing.as_ref().and_then(|edit| match edit.target {
                                        EditTarget::Object(id) => Some(id),
                                        _ => None,
                                    });
                                let selected: HashSet<u64> = this
                                    .selected
                                    .iter()
                                    .copied()
                                    .filter(|id| Some(*id) != editing)
                                    .collect();
                                let marquee = match this.gesture {
                                    Some(Gesture::Marquee { start, current, .. }) => {
                                        Some(Rect::from_two_pos(start, current))
                                    }
                                    _ => None,
                                };
                                let creating = match this.gesture {
                                    Some(Gesture::Create { id, .. }) => Some(id),
                                    _ => None,
                                };
                                let cropping = match this.gesture {
                                    Some(Gesture::Crop { id, .. }) => Some(id),
                                    _ => None,
                                };
                                let handles = this.handles();
                                (
                                    std::mem::take(&mut this.board),
                                    std::mem::take(&mut this.paint_cache),
                                    this.view,
                                    this.canvas,
                                    selected,
                                    this.guides.clone(),
                                    (
                                        handles,
                                        cropping,
                                        this.show_grid,
                                        marquee,
                                        editing,
                                        creating,
                                        !this.declutter,
                                    ),
                                )
                            });
                        let (handles, cropping, grid, marquee, editing, creating, empty_hint) =
                            overlay;
                        let started = Instant::now();
                        paint::board(
                            window,
                            &board,
                            view,
                            canvas,
                            paint::Overlay {
                                selected: &selected,
                                handles,
                                cropping,
                                grid,
                                marquee,
                                editing,
                                creating,
                                guides: &guides,
                                empty_hint,
                                eraser,
                                trail: &trail,
                                settled,
                            },
                            &mut cache,
                        );
                        let elapsed = started.elapsed();
                        paint_entity.update(cx, |this, _| {
                            this.board = board;
                            this.paint_cache = cache;
                            if let Some(bench) = &mut this.bench {
                                bench.board_paint = elapsed;
                            }
                        });
                    },
                )
                .size_full(),
            );
        let mut body = div()
            .relative()
            .flex_1()
            .w_full()
            .overflow_hidden()
            .child(board_canvas);
        if let Some(editor) = self.inline_editor() {
            body = body.child(editor);
        }
        if chrome_visible {
            let mut chrome = div()
                .absolute()
                .inset_0()
                .opacity(chrome_opacity)
                .child(self.top_bar(cx))
                .child(self.toolbar(window, cx))
                .child(
                    div()
                        .absolute()
                        .left_6()
                        .bottom_6()
                        .flex()
                        .items_center()
                        .gap_4()
                        .child(
                            panel()
                                .p_1()
                                .flex()
                                .gap_1()
                                .child(self.icon_button(
                                    "help",
                                    Icon::Help,
                                    Action::Help,
                                    self.show_help,
                                    "Keyboard shortcuts",
                                    cx,
                                ))
                                .child(self.icon_button(
                                    "settings",
                                    Icon::Settings,
                                    Action::Settings,
                                    self.show_settings,
                                    "Settings",
                                    cx,
                                ))
                                .child(self.icon_button(
                                    "grid",
                                    Icon::Grid,
                                    Action::Grid,
                                    self.show_grid,
                                    "Grid · G",
                                    cx,
                                )),
                        )
                        .child(label(format!("{} objects", self.board.items.len()))),
                )
                .child(
                    panel()
                        .absolute()
                        .right_6()
                        .bottom_6()
                        .p_1()
                        .flex()
                        .items_center()
                        .gap_1()
                        .child(self.icon_button(
                            "zoom-out",
                            Icon::Minus,
                            Action::Zoom(1.0 / 1.2),
                            false,
                            "Zoom out · −",
                            cx,
                        ))
                        .child(self.button(
                            "zoom-value",
                            &format!("{:.0}%", self.view.zoom * 100.0),
                            Action::ResetZoom,
                            false,
                            cx,
                        ))
                        .child(self.icon_button(
                            "zoom-in",
                            Icon::Plus,
                            Action::Zoom(1.2),
                            false,
                            "Zoom in · +",
                            cx,
                        ))
                        .child(div().w(px(1.0)).h_6().mx_1().bg(rgb(BORDER)))
                        .child(self.icon_button(
                            "fit",
                            Icon::Fit,
                            Action::Fit,
                            false,
                            "Fit board · 2",
                            cx,
                        ))
                        .child(self.icon_button(
                            "map",
                            Icon::Map,
                            Action::Map,
                            self.show_map,
                            "Minimap · M",
                            cx,
                        )),
                );
            if self.show_map {
                chrome = chrome.child(self.minimap(cx));
            }
            if let Some((message, error, _)) = &self.toast {
                chrome = chrome.child(
                    panel()
                        .absolute()
                        .top_6()
                        .right_6()
                        .max_w(px(320.0))
                        .px_4()
                        .py_3()
                        .text_size(px(13.0))
                        .text_color(rgb(if *error { 0xdc2626 } else { FG }))
                        .child(message.clone()),
                );
            }
            body = body.child(chrome);
        }
        // The object toolbar opens on request, so it stays available in board-only mode.
        if !self.selected.is_empty()
            && self.show_properties
            && self.gesture.is_none()
            && self.editing.is_none()
        {
            body = body.child(self.properties(cx));
        }
        let mut root = div()
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(BASE))
            .text_color(rgb(FG))
            .font_family("Inter Variable")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::key))
            .on_key_up(cx.listener(Self::key_up))
            .on_mouse_move(cx.listener(Self::motion))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::up))
            .on_mouse_up(MouseButton::Middle, cx.listener(Self::up))
            .on_mouse_up_out(MouseButton::Middle, cx.listener(Self::up))
            .child(body);
        if chrome_visible && self.file_menu {
            root = root.child(self.file_dropdown(cx).opacity(chrome_opacity));
        }
        if chrome_visible && self.show_boards {
            root = root.child(self.board_picker(cx).opacity(chrome_opacity));
        }
        if chrome_visible && self.show_settings {
            root = root.child(self.settings(cx).opacity(chrome_opacity));
        }
        if chrome_visible && self.show_help {
            root = root.child(self.help(cx).opacity(chrome_opacity));
        }
        if let Some(edit) = &self.editing
            && matches!(edit.target, EditTarget::Title)
        {
            root = root.child(self.edit_modal(edit, cx));
        }
        root
    }
}
