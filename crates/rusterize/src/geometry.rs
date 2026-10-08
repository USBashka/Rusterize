use std::sync::Arc;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}
impl Point {
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
    pub(crate) fn finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite()
    }
}
impl From<(f32, f32)> for Point {
    fn from((x, y): (f32, f32)) -> Self {
        Self::new(x, y)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Size {
    pub width: f32,
    pub height: f32,
}
impl Size {
    pub const fn new(width: f32, height: f32) -> Self {
        Self { width, height }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}
impl Rect {
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }
    pub fn contains(self, p: Point) -> bool {
        p.x >= self.x && p.y >= self.y && p.x < self.x + self.width && p.y < self.y + self.height
    }
    pub fn inset(self, amount: f32) -> Self {
        Self::new(
            self.x + amount,
            self.y + amount,
            (self.width - 2.0 * amount).max(0.0),
            (self.height - 2.0 * amount).max(0.0),
        )
    }
    pub(crate) fn valid(self) -> bool {
        [
            self.x,
            self.y,
            self.width,
            self.height,
            self.x + self.width,
            self.y + self.height,
        ]
        .iter()
        .all(|v| v.is_finite())
            && self.width >= 0.0
            && self.height >= 0.0
    }
}

/// Affine matrix `[a b c d e f]`: x' = ax + cy + e, y' = bx + dy + f.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transform(pub [f32; 6]);
impl Default for Transform {
    fn default() -> Self {
        Self::IDENTITY
    }
}
impl Transform {
    pub const IDENTITY: Self = Self([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
    pub const fn translate(x: f32, y: f32) -> Self {
        Self([1.0, 0.0, 0.0, 1.0, x, y])
    }
    pub const fn scale(x: f32, y: f32) -> Self {
        Self([x, 0.0, 0.0, y, 0.0, 0.0])
    }
    pub fn rotate(radians: f32) -> Self {
        let (s, c) = radians.sin_cos();
        Self([c, s, -s, c, 0.0, 0.0])
    }
    /// Post-multiply: the returned transform applies `other` first, then `self`.
    pub fn concat(self, other: Self) -> Self {
        let [a, b, c, d, e, f] = self.0;
        let [g, h, i, j, k, l] = other.0;
        Self([
            a * g + c * h,
            b * g + d * h,
            a * i + c * j,
            b * i + d * j,
            a * k + c * l + e,
            b * k + d * l + f,
        ])
    }
    pub fn apply(self, p: Point) -> Point {
        let [a, b, c, d, e, f] = self.0;
        Point::new(a * p.x + c * p.y + e, b * p.x + d * p.y + f)
    }
    pub fn inverse(self) -> Option<Self> {
        let [a, b, c, d, e, f] = self.0;
        let det = a * d - b * c;
        if !det.is_finite() || det.abs() < f32::EPSILON {
            return None;
        }
        let result = Self([
            d / det,
            -b / det,
            -c / det,
            a / det,
            (c * f - d * e) / det,
            (b * e - a * f) / det,
        ]);
        result.0.iter().all(|n| n.is_finite()).then_some(result)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Segment {
    Move(Point),
    Line(Point),
    Cubic(Point, Point, Point),
    Close,
}

/// Immutable, cheaply cloned cubic path. Filled paths use the non-zero winding rule.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Path(pub(crate) Arc<[Segment]>);
impl Path {
    pub fn builder() -> PathBuilder {
        PathBuilder::default()
    }
    pub fn segments(&self) -> &[Segment] {
        &self.0
    }
    pub(crate) fn valid(&self) -> bool {
        let mut open = false;
        self.0.iter().all(|s| match *s {
            Segment::Move(p) => {
                open = true;
                p.finite()
            }
            Segment::Line(p) => open && p.finite(),
            Segment::Cubic(a, b, c) => open && a.finite() && b.finite() && c.finite(),
            Segment::Close => {
                let was_open = open;
                open = false;
                was_open
            }
        })
    }
}

#[derive(Default)]
pub struct PathBuilder {
    segments: Vec<Segment>,
}
impl PathBuilder {
    pub fn move_to(mut self, p: impl Into<Point>) -> Self {
        self.segments.push(Segment::Move(p.into()));
        self
    }
    pub fn line_to(mut self, p: impl Into<Point>) -> Self {
        self.segments.push(Segment::Line(p.into()));
        self
    }
    pub fn cubic_to(
        mut self,
        a: impl Into<Point>,
        b: impl Into<Point>,
        end: impl Into<Point>,
    ) -> Self {
        self.segments
            .push(Segment::Cubic(a.into(), b.into(), end.into()));
        self
    }
    pub fn close(mut self) -> Self {
        self.segments.push(Segment::Close);
        self
    }
    pub fn build(self) -> Path {
        Path(self.segments.into())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Shape {
    Rect(Rect),
    RoundedRect(Rect, f32),
    Ellipse(Rect),
    Path(Path),
}
impl Shape {
    pub(crate) fn valid(&self) -> bool {
        match self {
            Self::Rect(r) | Self::Ellipse(r) => r.valid(),
            Self::RoundedRect(r, v) => r.valid() && v.is_finite() && *v >= 0.0,
            Self::Path(p) => p.valid(),
        }
    }
}
impl From<Rect> for Shape {
    fn from(r: Rect) -> Self {
        Self::Rect(r)
    }
}
impl From<Path> for Shape {
    fn from(p: Path) -> Self {
        Self::Path(p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn compose_inverse_and_order() {
        let t = Transform::translate(10.0, 20.0).concat(Transform::scale(2.0, 3.0));
        let p = Point::new(4.0, 5.0);
        assert_eq!(t.apply(p), Point::new(18.0, 35.0));
        let q = t.inverse().unwrap().apply(t.apply(p));
        assert!((q.x - p.x).abs() < 1e-5 && (q.y - p.y).abs() < 1e-5);
        assert!(Transform::scale(0.0, 1.0).inverse().is_none());
    }
    #[test]
    fn half_open_hit_test() {
        let r = Rect::new(0.0, 0.0, 20.0, 30.0);
        assert!(r.contains((0.0, 0.0).into()));
        assert!(!r.contains((20.0, 10.0).into()));
    }
}
