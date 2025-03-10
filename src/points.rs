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

#[derive(Clone, Debug)]
pub struct Line {
    /// Start point of line
    pub start: Point,
    /// Non-negative line length
    pub length: PointInt,
    pub direction: LineDir,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineDir {
    /// X axis
    Horizontal,
    /// Y axis
    Vertical,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Corner {
    TopRight,
    TopLeft,
    BottomLeft,
    BottomRight,
}

#[derive(Clone, Debug)]
pub enum ByTwoPoints {
    Rectangle(Rectangle),
    Line(Line),
    Point(Point),
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

    /// Make non-degenerate geometric figure from two points.
    pub fn into_figure(self, other: Self) -> ByTwoPoints {
        match self.quater(&other) {
            Quater::BottomLeft | Quater::TopLeft | Quater::TopRight | Quater::BottomRight => {
                ByTwoPoints::Rectangle(
                    Rectangle::from_two_points(self, other)
                        .expect("rectangle from correct quater points"),
                )
            }
            Quater::AxisX | Quater::AxisY => ByTwoPoints::Line(
                Line::from_two_points(self, other).expect("line from correct quater points"),
            ),
            Quater::Centre => ByTwoPoints::Point(self),
        }
    }

    /// Returns the non-negative "distance" between two points.
    /// This function implements a valid metric but is not necessarily Euclidean.
    /// The distance is zero if and only if the two points are equal.
    pub fn metric(&self, other: &Self) -> PointInt {
        // In this project, the Manhattan metric is sufficient
        // for measuring distances
        self.x.abs_diff(other.x) + self.y.abs_diff(other.y)
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
        match a.quater(&b) {
            Quater::BottomRight => {
                let width = b.x - a.x;
                let height = b.y - a.y;
                Some(Self {
                    start: a,
                    width,
                    height,
                })
            }

            Quater::TopRight => Some(Self {
                start: Point::new(a.x, b.y),
                width: b.x - a.x,
                height: a.y - b.y,
            }),

            Quater::TopLeft | Quater::BottomLeft => Self::from_two_points(b, a),

            _ => None,
        }
    }

    pub fn is_degenerate(&self) -> bool {
        self.height == 0 || self.width == 0
    }

    pub fn degenerate(self) -> Result<Line, Self> {
        match (self.height, self.width) {
            (0, length) => Ok(Line::new(self.start, length, LineDir::Horizontal)),
            (length, 0) => Ok(Line::new(self.start, length, LineDir::Vertical)),
            _ => Err(self),
        }
    }

    pub fn corner(&self, corner: Corner) -> Point {
        let mut start = self.start.clone();
        match corner {
            Corner::TopLeft => start,
            Corner::TopRight => {
                start.x += self.width;
                start
            }
            Corner::BottomLeft => {
                start.y += self.height;
                start
            }
            Corner::BottomRight => {
                start.x += self.width;
                start.y += self.height;
                start
            }
        }
    }
}

impl Line {
    pub fn new(start: Point, length: PointInt, direction: LineDir) -> Self {
        Self {
            start,
            length,
            direction,
        }
    }

    /// Returns line from two points. Line can be degenerate (if `a == b`).
    /// Returns [`None`] if two points are not in same axis.
    pub fn from_two_points(a: Point, b: Point) -> Option<Self> {
        let (direction, length) = if a.x == b.x {
            (LineDir::Vertical, b.y.abs_diff(a.y))
        } else if a.y == b.y {
            (LineDir::Horizontal, b.x.abs_diff(a.x))
        } else {
            return None;
        };

        match direction {
            LineDir::Vertical => Some(Line::new(if a.y <= b.y { a } else { b }, length, direction)),
            LineDir::Horizontal => {
                Some(Line::new(if a.x <= b.x { a } else { b }, length, direction))
            }
        }
    }

    pub fn is_degenerate(&self) -> bool {
        self.length == 0
    }

    pub fn degenerate(self) -> Result<Point, Self> {
        if self.length == 0 {
            Ok(self.start)
        } else {
            Err(self)
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
