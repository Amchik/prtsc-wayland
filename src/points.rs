use std::cmp::Ordering;

pub type PointInt = u32;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Point {
    pub x: PointInt,
    pub y: PointInt,
}

#[derive(Clone, Debug)]
pub struct Rectangle {
    /// Top left point of rectangle
    pub start: Point,
    /// Width of rectangle, always non-negative
    pub width: PointInt,
    /// Height of rectangle, always non-negative
    pub height: PointInt,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Quater {
    TopRight,
    TopLeft,
    BottomLeft,
    BottomRight,

    AxisX,
    AxisY,
    Centre,
}

impl Point {
    pub fn new(x: PointInt, y: PointInt) -> Self {
        Point { x, y }
    }

    pub fn quater(&self, point: &Self) -> Quater {
        match (self.x.cmp(&point.x), self.y.cmp(&point.y)) {
            (Ordering::Equal, Ordering::Equal) => Quater::Centre,
            (Ordering::Less, Ordering::Greater) => Quater::TopRight,
            (Ordering::Less, Ordering::Less) => Quater::BottomRight,
            (Ordering::Greater, Ordering::Less) => Quater::BottomLeft,
            (Ordering::Greater, Ordering::Greater) => Quater::TopLeft,
            (Ordering::Equal, _) => Quater::AxisY,
            (_, Ordering::Equal) => Quater::AxisX,
        }
    }

    pub fn is_same_quater(&self, a: &Self, b: &Self) -> bool {
        self.quater(a) == self.quater(b)
    }
}

impl Rectangle {
    pub fn new(start: Point, width: PointInt, height: PointInt) -> Self {
        Self {
            start,
            width,
            height,
        }
    }

    /// Make rectangle by two points. Returns [`None`] if points are same or located in one axis
    /// (so rectangle never degenerate).
    pub fn from_two_points(a: Point, b: Point) -> Option<Self> {
        let start = Point::new(a.x.min(b.x), a.y.min(b.y));
        let end = Point::new(a.x.max(b.x), a.y.max(b.y));

        let width = end.x - start.x;
        let height = end.y - start.y;

        if width == 0 || height == 0 {
            None
        } else {
            Some(Self {
                start,
                width,
                height,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Point, Quater};

    #[test]
    fn quater_tests() {
        // a, b, expected:
        let expected = &[
            (Point::new(5, 5), Point::new(6, 6), Quater::BottomRight),
            (Point::new(5, 5), Point::new(4, 6), Quater::BottomLeft),
            (Point::new(5, 5), Point::new(4, 4), Quater::TopLeft),
            (Point::new(5, 5), Point::new(6, 4), Quater::TopRight),
            (Point::new(5, 5), Point::new(5, 6), Quater::AxisY),
            (Point::new(5, 5), Point::new(6, 5), Quater::AxisX),
            (Point::new(5, 5), Point::new(5, 5), Quater::Centre),
        ];

        for (a, b, expected) in expected {
            let actual = a.quater(b);

            assert_eq!(*expected, actual, "Failed for a = {a:?}, b = {b:?}");
        }
    }
}
