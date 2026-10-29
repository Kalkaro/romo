use crate::geometry::Pos2;

#[derive(Clone, Copy, Debug)]
pub struct Curve {
    pub control: Pos2,
    pub end: Pos2,
}

/// Midpoint curves soften sampling corners without overshooting the original bounds.
/// The first and final sample remain exact, so there is no delayed pen tip.
pub fn curves(points: &[Pos2]) -> Vec<Curve> {
    let mut points = points.iter().copied();
    let Some(mut previous) = points.next() else {
        return Vec::new();
    };
    let mut result = Vec::new();
    for point in points {
        if point.distance(previous) < 0.0001 {
            continue;
        }
        result.push(Curve {
            control: previous,
            end: previous + (point - previous) * 0.5,
        });
        previous = point;
    }
    result.push(Curve {
        control: previous,
        end: previous,
    });
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::{Rect, pos2};

    #[test]
    fn smoothing_preserves_the_pen_tip_and_stays_within_the_sample_bounds() {
        let points = [
            pos2(0.0, 0.0),
            pos2(10.0, 20.0),
            pos2(20.0, 0.0),
            pos2(30.0, 15.0),
        ];
        let curves = curves(&points);
        assert_eq!(curves.last().unwrap().end, pos2(30.0, 15.0));
        let bounds = Rect::from_min_max(pos2(0.0, 0.0), pos2(30.0, 20.0));
        let mut start = points[0];
        for curve in curves {
            for i in 0..=20 {
                let t = i as f32 / 20.0;
                let point = start
                    + (curve.control - start) * (2.0 * (1.0 - t) * t)
                    + (curve.end - start) * (t * t);
                assert!(bounds.contains(point));
            }
            start = curve.end;
        }
    }

    #[test]
    fn duplicate_samples_and_dots_produce_finite_curves() {
        let point = pos2(5.0, 8.0);
        let curves = curves(&[point, point, point]);
        assert_eq!(curves.len(), 1);
        assert_eq!(curves[0].end, point);
        assert!(curves[0].control.is_finite());
    }
}
