use serde::{Deserialize, Serialize};
use std::ops::{Add, AddAssign, Div, Mul, Neg, Sub, SubAssign};

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Pos2 {
    pub x: f32,
    pub y: f32,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}
pub const fn pos2(x: f32, y: f32) -> Pos2 {
    Pos2 { x, y }
}
pub const fn vec2(x: f32, y: f32) -> Vec2 {
    Vec2 { x, y }
}
impl Pos2 {
    pub const ZERO: Self = pos2(0.0, 0.0);
    pub fn to_vec2(self) -> Vec2 {
        vec2(self.x, self.y)
    }
    pub fn distance(self, other: Self) -> f32 {
        (self - other).length()
    }
    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite()
    }
}
impl Vec2 {
    pub const ZERO: Self = vec2(0.0, 0.0);
    pub fn splat(v: f32) -> Self {
        vec2(v, v)
    }
    pub fn to_pos2(self) -> Pos2 {
        pos2(self.x, self.y)
    }
    pub fn length_sq(self) -> f32 {
        self.dot(self)
    }
    pub fn length(self) -> f32 {
        self.length_sq().sqrt()
    }
    pub fn dot(self, other: Self) -> f32 {
        self.x * other.x + self.y * other.y
    }
    pub fn normalized(self) -> Self {
        self / self.length().max(0.00001)
    }
    pub fn max(self, other: Self) -> Self {
        vec2(self.x.max(other.x), self.y.max(other.y))
    }
    pub fn min_elem(self) -> f32 {
        self.x.min(self.y)
    }
    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite()
    }
}
impl Add<Vec2> for Pos2 {
    type Output = Self;
    fn add(self, v: Vec2) -> Self {
        pos2(self.x + v.x, self.y + v.y)
    }
}
impl Sub<Vec2> for Pos2 {
    type Output = Self;
    fn sub(self, v: Vec2) -> Self {
        pos2(self.x - v.x, self.y - v.y)
    }
}
impl Sub for Pos2 {
    type Output = Vec2;
    fn sub(self, p: Self) -> Vec2 {
        vec2(self.x - p.x, self.y - p.y)
    }
}
impl AddAssign<Vec2> for Pos2 {
    fn add_assign(&mut self, v: Vec2) {
        *self = *self + v;
    }
}
impl Add for Vec2 {
    type Output = Self;
    fn add(self, v: Self) -> Self {
        vec2(self.x + v.x, self.y + v.y)
    }
}
impl Sub for Vec2 {
    type Output = Self;
    fn sub(self, v: Self) -> Self {
        vec2(self.x - v.x, self.y - v.y)
    }
}
impl Mul<f32> for Vec2 {
    type Output = Self;
    fn mul(self, v: f32) -> Self {
        vec2(self.x * v, self.y * v)
    }
}
impl Mul for Vec2 {
    type Output = Self;
    fn mul(self, v: Self) -> Self {
        vec2(self.x * v.x, self.y * v.y)
    }
}
impl Div<f32> for Vec2 {
    type Output = Self;
    fn div(self, v: f32) -> Self {
        vec2(self.x / v, self.y / v)
    }
}
impl Div for Vec2 {
    type Output = Self;
    fn div(self, v: Self) -> Self {
        vec2(self.x / v.x, self.y / v.y)
    }
}
impl Neg for Vec2 {
    type Output = Self;
    fn neg(self) -> Self {
        vec2(-self.x, -self.y)
    }
}
impl AddAssign for Vec2 {
    fn add_assign(&mut self, v: Self) {
        *self = *self + v;
    }
}
impl SubAssign for Vec2 {
    fn sub_assign(&mut self, v: Self) {
        *self = *self - v;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub min: Pos2,
    pub max: Pos2,
}
impl Rect {
    pub const NOTHING: Self = Self {
        min: pos2(f32::INFINITY, f32::INFINITY),
        max: pos2(f32::NEG_INFINITY, f32::NEG_INFINITY),
    };
    pub fn from_min_max(min: Pos2, max: Pos2) -> Self {
        Self { min, max }
    }
    pub fn from_min_size(min: Pos2, size: Vec2) -> Self {
        Self {
            min,
            max: min + size,
        }
    }
    pub fn from_center_size(center: Pos2, size: Vec2) -> Self {
        Self {
            min: center - size * 0.5,
            max: center + size * 0.5,
        }
    }
    pub fn from_two_pos(a: Pos2, b: Pos2) -> Self {
        Self {
            min: pos2(a.x.min(b.x), a.y.min(b.y)),
            max: pos2(a.x.max(b.x), a.y.max(b.y)),
        }
    }
    pub fn size(self) -> Vec2 {
        self.max - self.min
    }
    pub fn width(self) -> f32 {
        self.size().x
    }
    pub fn height(self) -> f32 {
        self.size().y
    }
    pub fn center(self) -> Pos2 {
        self.min + self.size() * 0.5
    }
    pub fn expand(self, v: f32) -> Self {
        Self {
            min: self.min - Vec2::splat(v),
            max: self.max + Vec2::splat(v),
        }
    }
    pub fn shrink(self, v: f32) -> Self {
        self.expand(-v)
    }
    pub fn extend_with(&mut self, p: Pos2) {
        self.min = pos2(self.min.x.min(p.x), self.min.y.min(p.y));
        self.max = pos2(self.max.x.max(p.x), self.max.y.max(p.y));
    }
    pub fn union(self, other: Self) -> Self {
        let mut r = self;
        r.extend_with(other.min);
        r.extend_with(other.max);
        r
    }
    pub fn contains(self, p: Pos2) -> bool {
        p.x >= self.min.x && p.x <= self.max.x && p.y >= self.min.y && p.y <= self.max.y
    }
    pub fn contains_rect(self, r: Self) -> bool {
        self.contains(r.min) && self.contains(r.max)
    }
    pub fn intersects(self, r: Self) -> bool {
        self.min.x <= r.max.x
            && self.max.x >= r.min.x
            && self.min.y <= r.max.y
            && self.max.y >= r.min.y
    }
    /// The overlap of two rectangles, which is empty when they don't meet.
    pub fn intersect(self, r: Self) -> Self {
        Self {
            min: pos2(self.min.x.max(r.min.x), self.min.y.max(r.min.y)),
            max: pos2(self.max.x.min(r.max.x), self.max.y.min(r.max.y)),
        }
    }
    pub fn right_top(self) -> Pos2 {
        pos2(self.max.x, self.min.y)
    }
    pub fn left_bottom(self) -> Pos2 {
        pos2(self.min.x, self.max.y)
    }
    pub fn center_top(self) -> Pos2 {
        pos2(self.center().x, self.min.y)
    }
    pub fn center_bottom(self) -> Pos2 {
        pos2(self.center().x, self.max.y)
    }
    pub fn right_center(self) -> Pos2 {
        pos2(self.max.x, self.center().y)
    }
    pub fn left_center(self) -> Pos2 {
        pos2(self.min.x, self.center().y)
    }
}

/// A corner of a rectangle, where its resize handles sit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}
impl Corner {
    pub const ALL: [Self; 4] = [
        Self::TopLeft,
        Self::TopRight,
        Self::BottomLeft,
        Self::BottomRight,
    ];
    /// This corner of `r`.
    pub fn of(self, r: Rect) -> Pos2 {
        match self {
            Self::TopLeft => r.min,
            Self::TopRight => r.right_top(),
            Self::BottomLeft => r.left_bottom(),
            Self::BottomRight => r.max,
        }
    }
    pub fn opposite(self) -> Self {
        match self {
            Self::TopLeft => Self::BottomRight,
            Self::TopRight => Self::BottomLeft,
            Self::BottomLeft => Self::TopRight,
            Self::BottomRight => Self::TopLeft,
        }
    }
    /// The direction from the opposite corner to this one, -1 or 1 on each axis.
    pub fn sign(self) -> Vec2 {
        match self {
            Self::TopLeft => vec2(-1.0, -1.0),
            Self::TopRight => vec2(1.0, -1.0),
            Self::BottomLeft => vec2(-1.0, 1.0),
            Self::BottomRight => vec2(1.0, 1.0),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Color32([u8; 4]);
impl Color32 {
    pub const fn from_rgb(r: u8, g: u8, b: u8) -> Self {
        Self([r, g, b, 255])
    }
    pub fn r(self) -> u8 {
        self.0[0]
    }
    pub fn g(self) -> u8 {
        self.0[1]
    }
    pub fn b(self) -> u8 {
        self.0[2]
    }
    pub fn rgb(self) -> u32 {
        (self.r() as u32) << 16 | (self.g() as u32) << 8 | self.b() as u32
    }
}
