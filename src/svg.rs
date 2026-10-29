use crate::geometry::{Color32, Rect};
use crate::model::{Board, Kind};
use std::fmt::Write;

fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn color(value: Color32) -> String {
    format!("#{:02x}{:02x}{:02x}", value.r(), value.g(), value.b())
}

pub fn export(board: &Board) -> String {
    let bounds = board
        .bounds()
        .unwrap_or(Rect::from_min_size(
            crate::geometry::Pos2::ZERO,
            crate::geometry::vec2(1000.0, 700.0),
        ))
        .expand(48.0);
    let mut out = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"{} {} {} {}\" width=\"{}\" height=\"{}\">\n<title>{}</title>\n<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"#09090b\"/>\n",
        bounds.min.x,
        bounds.min.y,
        bounds.width(),
        bounds.height(),
        bounds.width(),
        bounds.height(),
        escape(&board.title),
        bounds.min.x,
        bounds.min.y,
        bounds.width(),
        bounds.height()
    );
    out.push_str("<defs><marker id=\"arrow\" viewBox=\"0 0 10 10\" refX=\"9\" refY=\"5\" markerWidth=\"7\" markerHeight=\"7\" orient=\"auto-start-reverse\"><path d=\"M 1 1 L 9 5 L 1 9\" fill=\"none\" stroke=\"context-stroke\" stroke-width=\"1.5\"/></marker></defs>\n");
    for frames in [true, false] {
        for item in board
            .items
            .iter()
            .filter(|item| (item.kind == Kind::Frame) == frames)
        {
            let r = item.rect();
            let fill = color(item.color);
            let text_color = if item.kind == Kind::Text {
                fill.clone()
            } else if item.color.r() as u32 + item.color.g() as u32 + item.color.b() as u32 > 420 {
                "#18181b".into()
            } else {
                "#fafafa".into()
            };
            match item.kind {
                Kind::Note | Kind::Rectangle => {
                    let _ = writeln!(
                        out,
                        "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"4\" fill=\"{fill}\" stroke=\"#3f3f46\" stroke-width=\"1\"/>",
                        r.min.x,
                        r.min.y,
                        r.width(),
                        r.height()
                    );
                }
                Kind::Ellipse => {
                    let _ = writeln!(
                        out,
                        "<ellipse cx=\"{}\" cy=\"{}\" rx=\"{}\" ry=\"{}\" fill=\"{fill}\" stroke=\"#3f3f46\"/>",
                        r.center().x,
                        r.center().y,
                        r.width() / 2.0,
                        r.height() / 2.0
                    );
                }
                Kind::Diamond => {
                    let _ = writeln!(
                        out,
                        "<polygon points=\"{},{} {},{} {},{} {},{}\" fill=\"{fill}\" stroke=\"#3f3f46\"/>",
                        r.center().x,
                        r.min.y,
                        r.max.x,
                        r.center().y,
                        r.center().x,
                        r.max.y,
                        r.min.x,
                        r.center().y
                    );
                }
                Kind::Frame => {
                    let _ = writeln!(
                        out,
                        "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"8\" fill=\"none\" stroke=\"{fill}\" stroke-width=\"1\"/>",
                        r.min.x,
                        r.min.y,
                        r.width(),
                        r.height()
                    );
                }
                Kind::Pen => {
                    if let Some(start) = item.points.first() {
                        let mut d = format!("M {} {}", item.pos.x + start.x, item.pos.y + start.y);
                        for curve in crate::stroke::curves(&item.points) {
                            let _ = write!(
                                d,
                                " Q {} {} {} {}",
                                item.pos.x + curve.control.x,
                                item.pos.y + curve.control.y,
                                item.pos.x + curve.end.x,
                                item.pos.y + curve.end.y
                            );
                        }
                        let _ = writeln!(
                            out,
                            "<path d=\"{d}\" fill=\"none\" stroke=\"{fill}\" stroke-width=\"{}\" stroke-linecap=\"round\" stroke-linejoin=\"round\"/>",
                            item.line_width()
                        );
                        if item.points.iter().all(|p| p.distance(*start) < 0.01) {
                            let _ = writeln!(
                                out,
                                "<circle cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"{fill}\"/>",
                                item.pos.x + start.x,
                                item.pos.y + start.y,
                                item.line_width() * 0.5
                            );
                        }
                    }
                }
                Kind::Arrow => {
                    let (a, b) = board.arrow_ends(item);
                    let _ = writeln!(
                        out,
                        "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"{fill}\" stroke-width=\"{}\" marker-end=\"url(#arrow)\"/>",
                        a.x, a.y, b.x, b.y, item.stroke_width
                    );
                }
                Kind::Image => {
                    if let Some(asset) = &item.image {
                        // The viewBox shows only the cropped part, stretched like on the board.
                        let shown = item.crop.unwrap_or(Rect::from_min_size(
                            crate::geometry::Pos2::ZERO,
                            crate::geometry::vec2(asset.width as f32, asset.height as f32),
                        ));
                        let _ = writeln!(
                            out,
                            "<svg x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" viewBox=\"{} {} {} {}\" preserveAspectRatio=\"none\"><image width=\"{}\" height=\"{}\" href=\"data:image/png;base64,{}\"/></svg>",
                            item.pos.x,
                            item.pos.y,
                            item.size.x,
                            item.size.y,
                            shown.min.x,
                            shown.min.y,
                            shown.width(),
                            shown.height(),
                            asset.width,
                            asset.height,
                            asset.png
                        );
                    }
                }
                Kind::Text => {}
            }
            if !item.text.is_empty() {
                let left = matches!(item.kind, Kind::Text | Kind::Frame);
                let x = if left { r.min.x } else { r.center().x };
                let lines = wrap_text(&item.text, (r.width() - 32.0).max(20.0), item.font_size);
                let y = if item.kind == Kind::Frame {
                    r.min.y - 12.0
                } else if left {
                    r.min.y + item.font_size
                } else {
                    r.center().y - (lines.len() as f32 - 1.0) * item.font_size * 1.35 / 2.0
                        + item.font_size * 0.35
                };
                let _ = write!(
                    out,
                    "<text font-family=\"Inter, sans-serif\" font-size=\"{}\" fill=\"{}\" text-anchor=\"{}\">",
                    item.font_size,
                    if item.kind == Kind::Frame {
                        fill.as_str()
                    } else {
                        text_color.as_str()
                    },
                    if left { "start" } else { "middle" }
                );
                for (i, line) in lines.iter().enumerate() {
                    let _ = write!(
                        out,
                        "<tspan x=\"{x}\" y=\"{}\">{}</tspan>",
                        y + i as f32 * item.font_size * 1.35,
                        escape(line)
                    );
                }
                out.push_str("</text>\n");
            }
        }
    }
    out.push_str("</svg>\n");
    out
}

fn wrap_text(text: &str, width: f32, font: f32) -> Vec<String> {
    let capacity = (width / (font * 0.53)).max(1.0) as usize;
    let mut lines = Vec::new();
    for paragraph in text.split('\n') {
        if paragraph.is_empty() {
            lines.push(String::new());
            continue;
        }
        let mut line = String::new();
        for word in paragraph.split_whitespace() {
            if !line.is_empty() && line.chars().count() + word.chars().count() + 1 > capacity {
                lines.push(std::mem::take(&mut line));
            }
            if !line.is_empty() {
                line.push(' ');
            }
            line.push_str(word);
        }
        lines.push(line);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn svg_escapes_user_text_and_contains_the_whole_board() {
        let mut board = Board::welcome();
        board.title = "<Ideas & plans>".into();
        board.items[0].text = "<script>alert('x')</script> & a thought".into();
        let svg = export(&board);
        assert!(svg.contains("&lt;Ideas &amp; plans&gt;"));
        assert!(!svg.contains("<script>"));
        assert!(svg.contains("&lt;script&gt;"));
        assert!(svg.contains("marker-end=\"url(#arrow)\""));
        assert!(svg.ends_with("</svg>\n"));
    }

    #[test]
    fn cropped_images_export_only_the_part_that_shows() {
        use crate::geometry::{pos2, vec2};
        let mut board = Board::default();
        let id = board.add(Kind::Image, pos2(10.0, 20.0), crate::model::PALETTE[7]);
        let image = board.get_mut(id).unwrap();
        image.image = Some(std::sync::Arc::new(crate::images::ImageAsset::sample(
            200, 100,
        )));
        image.size = vec2(200.0, 100.0);
        *image = image.cropped(Rect::from_min_max(pos2(60.0, 20.0), pos2(160.0, 70.0)));
        let svg = export(&board);
        assert!(svg.contains(
            "<svg x=\"60\" y=\"20\" width=\"100\" height=\"50\" viewBox=\"50 0 100 50\" preserveAspectRatio=\"none\"><image width=\"200\" height=\"100\""
        ));
    }
}
