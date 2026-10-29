//! Rendering benchmark, enabled only by the explicit --bench flag.
use crate::{
    app::Romo,
    geometry::{Pos2, Vec2, pos2, vec2},
    model::{Board, Kind, PALETTE, View},
};
use gpui::{App, WindowHandle};

const WORDS: [&str; 12] = [
    "idea", "user", "launch", "research", "draft", "sketch", "flow", "test", "metric", "goal",
    "note", "plan",
];

/// A busy board: a wall of sticky notes in frames, with sketches, shapes and connectors.
pub fn board() -> Board {
    let mut board = Board {
        title: "Rendering benchmark".into(),
        ..Board::default()
    };
    let mut seed = 0x2545_f491_u32;
    let mut random = move || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed as f32 / u32::MAX as f32
    };
    for (index, (x, y)) in [
        (-2960.0, -2000.0),
        (40.0, -2000.0),
        (-2960.0, 40.0),
        (40.0, 40.0),
    ]
    .into_iter()
    .enumerate()
    {
        let id = board.add(Kind::Frame, pos2(x, y), PALETTE[4]);
        let frame = board.get_mut(id).unwrap();
        frame.size = vec2(2920.0, 1960.0);
        frame.text = format!("0{}  /  WORKSTREAM", index + 1);
    }
    let mut notes = Vec::new();
    for row in 0..16 {
        for column in 0..24 {
            let pos = pos2(column as f32 * 240.0 - 2900.0, row as f32 * 240.0 - 1940.0);
            let id = board.add(Kind::Note, pos, PALETTE[(row + column) % 4]);
            let words = 4 + (random() * 10.0) as usize;
            board.get_mut(id).unwrap().text = (0..words)
                .map(|i| WORDS[(i * 7 + row * 3 + column) % WORDS.len()])
                .collect::<Vec<_>>()
                .join(" ");
            notes.push(id);
        }
    }
    for index in 0..80 {
        let kind = [Kind::Ellipse, Kind::Diamond, Kind::Rectangle][index % 3];
        let pos = pos2(random() * 5600.0 - 2900.0, random() * 3700.0 - 1940.0);
        let id = board.add(kind, pos, PALETTE[1 + index % 3]);
        board.get_mut(id).unwrap().text = format!("Shape {index}");
    }
    for index in 0..300 {
        let origin = pos2(random() * 5600.0 - 2800.0, random() * 3800.0 - 1900.0);
        let id = board.add(Kind::Pen, origin, PALETTE[7]);
        let phase = index as f32;
        board.get_mut(id).unwrap().points = (0..120)
            .map(|step| {
                let t = step as f32 * 0.08;
                pos2(t * 30.0, (t * 2.0 + phase).sin() * 25.0 + random() * 4.0)
            })
            .collect();
    }
    for index in 0..120 {
        let from = notes[(index * 7) % notes.len()];
        let to = notes[(index * 7 + 25) % notes.len()];
        let id = board.add(Kind::Arrow, Pos2::ZERO, PALETTE[5]);
        let arrow = board.get_mut(id).unwrap();
        arrow.points = vec![Pos2::ZERO, pos2(200.0, 0.0)];
        arrow.start_anchor = Some(from);
        arrow.end_anchor = Some(to);
    }
    board
}

/// The benchmark's view for each frame, or `None` once it is finished.
pub fn view(frame: usize) -> Option<(&'static str, View)> {
    let circle = |frame: usize, zoom: f32| {
        let angle = frame as f32 / 120.0 * std::f32::consts::TAU;
        View {
            pan: vec2(angle.cos(), angle.sin()) * 300.0,
            zoom,
        }
    };
    Some(match frame {
        0..30 => ("warm-up", circle(0, 0.15)),
        30..270 => {
            let wave = (1.0 - ((frame - 30) as f32 / 120.0 * std::f32::consts::TAU).cos()) * 0.5;
            let zoom = 0.15 * (1.0f32 / 0.15).powf(wave);
            (
                "zoom 15-100%",
                View {
                    pan: Vec2::ZERO,
                    zoom,
                },
            )
        }
        270..390 => ("pan at 15%", circle(frame, 0.15)),
        390..510 => ("pan at 35%", circle(frame, 0.35)),
        _ => return None,
    })
}

/// Print mean and 95th percentile frame interval and board paint time per phase.
pub fn report(samples: &[(&'static str, f32, f32)]) {
    let mut phases: Vec<&str> = Vec::new();
    for (phase, _, _) in samples {
        if !phases.contains(phase) {
            phases.push(phase);
        }
    }
    for phase in phases.into_iter().filter(|phase| *phase != "warm-up") {
        let mut frames: Vec<f32> = samples
            .iter()
            .filter(|s| s.0 == phase)
            .map(|s| s.1)
            .collect();
        let mut paints: Vec<f32> = samples
            .iter()
            .filter(|s| s.0 == phase)
            .map(|s| s.2)
            .collect();
        let stats = |values: &mut Vec<f32>| {
            values.sort_by(f32::total_cmp);
            let mean = values.iter().sum::<f32>() / values.len().max(1) as f32;
            let p95 = values
                .get((values.len() as f32 * 0.95) as usize)
                .copied()
                .unwrap_or(0.0);
            (mean, p95)
        };
        let (frame_mean, frame_p95) = stats(&mut frames);
        let (paint_mean, paint_p95) = stats(&mut paints);
        println!(
            "{phase:>13}: {:5.1} fps · frame {frame_mean:5.1} ms (p95 {frame_p95:5.1}) · board paint {paint_mean:5.1} ms (p95 {paint_p95:5.1})",
            1000.0 / frame_mean
        );
    }
}

pub fn run(handle: WindowHandle<Romo>, cx: &mut App) {
    let _ = handle.update(cx, |app, window, cx| app.start_bench(board(), window, cx));
}
