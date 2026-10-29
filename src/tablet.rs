//! Graphics tablets on Wayland. GPUI doesn't implement the tablet protocol, and
//! some compositors (Hyprland) don't turn pens into pointers for such apps. Read
//! the pen on GPUI's own connection and hand it to Romo as an ordinary mouse:
//! the tip is the left button, the lower and upper barrel buttons are middle
//! and right. Pressure and tilt are ignored.
use crate::app::Romo;
use gpui::{
    App, Modifiers, MouseButton, MouseDownEvent, MouseExitEvent, MouseMoveEvent, MouseUpEvent,
    Pixels, PlatformInput, Point, WindowHandle, point, px,
};
use raw_window_handle::{HasDisplayHandle, RawDisplayHandle};
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};
use wayland_client::{
    Connection, Dispatch, Proxy, QueueHandle, WEnum,
    backend::ObjectId,
    delegate_noop, event_created_child,
    globals::{GlobalListContents, registry_queue_init},
    protocol::{wl_registry, wl_seat},
};
use wayland_protocols::wp::tablet::zv2::client::{
    zwp_tablet_manager_v2::ZwpTabletManagerV2,
    zwp_tablet_pad_group_v2::{self, ZwpTabletPadGroupV2},
    zwp_tablet_pad_ring_v2::ZwpTabletPadRingV2,
    zwp_tablet_pad_strip_v2::ZwpTabletPadStripV2,
    zwp_tablet_pad_v2::{self, ZwpTabletPadV2},
    zwp_tablet_seat_v2::{self, ZwpTabletSeatV2},
    zwp_tablet_tool_v2::{self, ZwpTabletToolV2},
    zwp_tablet_v2::ZwpTabletV2,
};

/// Linux input codes of the stylus barrel buttons.
const BTN_STYLUS: u32 = 0x14b;
const BTN_STYLUS2: u32 = 0x14c;

/// Listen for tablet pens and hand them to the app in `window` as a mouse.
pub fn watch(window: WindowHandle<Romo>, cx: &mut App) {
    let display = window
        .update(cx, |_, window, _| {
            match window.display_handle().map(|handle| handle.as_raw()) {
                Ok(RawDisplayHandle::Wayland(handle)) => Some(handle.display),
                _ => None,
            }
        })
        .ok()
        .flatten();
    // X11 already delivers tablets as pointer input.
    let Some(display) = display else {
        return;
    };
    // SAFETY: GPUI's display lives as long as the app. This connection only
    // adds objects with their own event queue to it.
    let connection = Connection::from_backend(unsafe {
        wayland_backend::sys::client::Backend::from_foreign_display(display.as_ptr().cast())
    });
    let Ok((globals, mut queue)) = registry_queue_init::<Tablets>(&connection) else {
        return;
    };
    let handle = queue.handle();
    let (Ok(seat), Ok(manager)) = (
        globals.bind::<wl_seat::WlSeat, _, _>(&handle, 1..=1, ()),
        globals.bind::<ZwpTabletManagerV2, _, _>(&handle, 1..=1, ()),
    ) else {
        return;
    };
    let tablet_seat = manager.get_tablet_seat(&seat, &handle, ());
    if connection.flush().is_err() {
        return;
    }
    let mut tablets = Tablets::default();
    cx.spawn(async move |cx| {
        let _alive = (seat, manager, tablet_seat);
        let mut replay = Replay::default();
        loop {
            // GPUI's event loop reads the socket; dispatch what it queued for pens.
            if queue.dispatch_pending(&mut tablets).is_err() {
                return;
            }
            let pens: Vec<Pen> = tablets
                .tools
                .values_mut()
                .flat_map(|tool| tool.ready.drain(..))
                .collect();
            if !pens.is_empty() {
                let delivered = cx.update(|cx| {
                    window.update(cx, |app, window, cx| {
                        let (modifiers, now) = (window.modifiers(), Instant::now());
                        for pen in pens {
                            app.pen(replay.input(pen, modifiers, now), window, cx);
                        }
                    })
                });
                if !matches!(delivered, Ok(Ok(()))) {
                    return;
                }
            }
            // Check often while a pen hovers over the tablet, rarely otherwise.
            let near = tablets.tools.values().any(|tool| tool.near);
            gpui::Timer::after(Duration::from_millis(if near { 4 } else { 50 })).await;
        }
    })
    .detach();
}

/// Pen input in the order it happened, as mouse actions.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Pen {
    Move(Point<Pixels>),
    Press(MouseButton),
    Release(MouseButton),
    Leave,
}

/// One tool's events, released at each protocol frame.
#[derive(Default)]
struct Tool {
    position: Point<Pixels>,
    moved: bool,
    tip: bool,
    near: bool,
    pending: Vec<Pen>,
    ready: Vec<Pen>,
}

impl Tool {
    fn event(&mut self, event: zwp_tablet_tool_v2::Event) {
        use zwp_tablet_tool_v2::Event;
        match event {
            Event::ProximityIn { .. } => self.near = true,
            Event::ProximityOut => {
                if std::mem::take(&mut self.tip) {
                    self.pending.push(Pen::Release(MouseButton::Left));
                }
                self.pending.push(Pen::Leave);
                self.near = false;
            }
            Event::Motion { x, y } => {
                self.position = point(px(x as f32), px(y as f32));
                self.moved = true;
            }
            Event::Down { .. } => {
                self.tip = true;
                self.pending.push(Pen::Press(MouseButton::Left));
            }
            Event::Up => {
                if std::mem::take(&mut self.tip) {
                    self.pending.push(Pen::Release(MouseButton::Left));
                }
            }
            Event::Button { button, state, .. } => {
                let button = match button {
                    BTN_STYLUS => MouseButton::Middle,
                    BTN_STYLUS2 => MouseButton::Right,
                    _ => return,
                };
                let pressed = matches!(
                    state,
                    WEnum::Value(zwp_tablet_tool_v2::ButtonState::Pressed)
                );
                self.pending.push(if pressed {
                    Pen::Press(button)
                } else {
                    Pen::Release(button)
                });
            }
            Event::Frame { .. } => {
                // Move first, so presses land where the pen is.
                if std::mem::take(&mut self.moved) {
                    self.ready.push(Pen::Move(self.position));
                }
                self.ready.append(&mut self.pending);
            }
            _ => {}
        }
    }
}

/// Turns pen actions into GPUI input, counting clicks like a mouse.
#[derive(Default)]
struct Replay {
    position: Point<Pixels>,
    pressed: Option<MouseButton>,
    last_press: Option<(Instant, Point<Pixels>, usize)>,
}

impl Replay {
    fn input(&mut self, pen: Pen, modifiers: Modifiers, now: Instant) -> PlatformInput {
        match pen {
            Pen::Move(position) => {
                self.position = position;
                PlatformInput::MouseMove(MouseMoveEvent {
                    position,
                    pressed_button: self.pressed,
                    modifiers,
                })
            }
            Pen::Press(button) => {
                let near = |at: Point<Pixels>| {
                    let (dx, dy) = (
                        f32::from(at.x - self.position.x),
                        f32::from(at.y - self.position.y),
                    );
                    dx * dx + dy * dy < 36.0
                };
                let click_count = match self.last_press {
                    Some((at, position, count))
                        if now.duration_since(at) < Duration::from_millis(400)
                            && near(position) =>
                    {
                        count + 1
                    }
                    _ => 1,
                };
                self.last_press = Some((now, self.position, click_count));
                self.pressed = Some(button);
                PlatformInput::MouseDown(MouseDownEvent {
                    button,
                    position: self.position,
                    modifiers,
                    click_count,
                    first_mouse: false,
                })
            }
            Pen::Release(button) => {
                self.pressed = None;
                PlatformInput::MouseUp(MouseUpEvent {
                    button,
                    position: self.position,
                    modifiers,
                    click_count: self.last_press.map_or(1, |(_, _, count)| count),
                })
            }
            Pen::Leave => PlatformInput::MouseExited(MouseExitEvent {
                position: self.position,
                pressed_button: self.pressed,
                modifiers,
            }),
        }
    }
}

#[derive(Default)]
struct Tablets {
    tools: HashMap<ObjectId, Tool>,
}

impl Dispatch<ZwpTabletToolV2, ()> for Tablets {
    fn event(
        tablets: &mut Self,
        tool: &ZwpTabletToolV2,
        event: zwp_tablet_tool_v2::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if matches!(event, zwp_tablet_tool_v2::Event::Removed) {
            tablets.tools.remove(&tool.id());
            tool.destroy();
        } else {
            tablets.tools.entry(tool.id()).or_default().event(event);
        }
    }
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for Tablets {
    fn event(
        _: &mut Self,
        _: &wl_registry::WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwpTabletSeatV2, ()> for Tablets {
    fn event(
        _: &mut Self,
        _: &ZwpTabletSeatV2,
        _: zwp_tablet_seat_v2::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
    event_created_child!(Tablets, ZwpTabletSeatV2, [
        zwp_tablet_seat_v2::EVT_TABLET_ADDED_OPCODE => (ZwpTabletV2, ()),
        zwp_tablet_seat_v2::EVT_TOOL_ADDED_OPCODE => (ZwpTabletToolV2, ()),
        zwp_tablet_seat_v2::EVT_PAD_ADDED_OPCODE => (ZwpTabletPadV2, ()),
    ]);
}

// Pads announce buttons, rings and strips, which Romo doesn't use.
impl Dispatch<ZwpTabletPadV2, ()> for Tablets {
    fn event(
        _: &mut Self,
        _: &ZwpTabletPadV2,
        _: zwp_tablet_pad_v2::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
    event_created_child!(Tablets, ZwpTabletPadV2, [
        zwp_tablet_pad_v2::EVT_GROUP_OPCODE => (ZwpTabletPadGroupV2, ()),
    ]);
}

impl Dispatch<ZwpTabletPadGroupV2, ()> for Tablets {
    fn event(
        _: &mut Self,
        _: &ZwpTabletPadGroupV2,
        _: zwp_tablet_pad_group_v2::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
    event_created_child!(Tablets, ZwpTabletPadGroupV2, [
        zwp_tablet_pad_group_v2::EVT_RING_OPCODE => (ZwpTabletPadRingV2, ()),
        zwp_tablet_pad_group_v2::EVT_STRIP_OPCODE => (ZwpTabletPadStripV2, ()),
    ]);
}

delegate_noop!(Tablets: ignore wl_seat::WlSeat);
delegate_noop!(Tablets: ZwpTabletManagerV2);
delegate_noop!(Tablets: ignore ZwpTabletV2);
delegate_noop!(Tablets: ignore ZwpTabletPadRingV2);
delegate_noop!(Tablets: ignore ZwpTabletPadStripV2);

#[cfg(test)]
mod tests {
    use super::*;
    use zwp_tablet_tool_v2::{ButtonState, Event};

    #[test]
    fn pen_frames_replay_as_mouse_moves_presses_and_releases() {
        let mut tool = Tool::default();
        tool.event(Event::Down { serial: 1 });
        tool.event(Event::Motion { x: 10.0, y: 20.0 });
        assert!(tool.ready.is_empty(), "events wait for the frame");
        tool.event(Event::Frame { time: 0 });
        let at = point(px(10.0), px(20.0));
        assert_eq!(tool.ready, [Pen::Move(at), Pen::Press(MouseButton::Left)]);
        tool.ready.clear();
        tool.event(Event::Button {
            serial: 2,
            button: BTN_STYLUS2,
            state: WEnum::Value(ButtonState::Pressed),
        });
        tool.event(Event::ProximityOut);
        tool.event(Event::Frame { time: 1 });
        assert_eq!(
            tool.ready,
            [
                Pen::Press(MouseButton::Right),
                Pen::Release(MouseButton::Left),
                Pen::Leave
            ]
        );

        let mut replay = Replay::default();
        let now = Instant::now();
        let modifiers = Modifiers::default();
        replay.input(Pen::Move(at), modifiers, now);
        for (tap, clicks) in [(0, 1), (150, 2)] {
            let when = now + Duration::from_millis(tap);
            match replay.input(Pen::Press(MouseButton::Left), modifiers, when) {
                PlatformInput::MouseDown(event) => {
                    assert_eq!((event.position, event.click_count), (at, clicks))
                }
                _ => panic!("a pen tip press is a mouse press"),
            }
            replay.input(Pen::Release(MouseButton::Left), modifiers, when);
        }
    }
}
