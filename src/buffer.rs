use pathfinding::directed::astar::astar;
use std::{cmp::max, ops::Add};
use num_traits::Zero;

/// A position in a diagram buffer.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct Point {
    pub x: usize,
    pub y: usize,
}

impl Point {
    pub const fn new(x: usize, y: usize) -> Self {
        Self { x, y }
    }
}

/// An editable ASCII diagram buffer.
///
/// Changes made by drawing operations are staged until [`Buffer::commit`] is called.
#[derive(Clone, Default, Eq, PartialEq)]
pub struct Buffer {
    chars: Vec<Vec<char>>,
    edits: Vec<(Point, char)>,
}

impl Buffer {
    /// Create a buffer from its textual diagram representation.
    ///
    /// Like the application's existing file loader, a final newline terminates
    /// the final line; it does not create an extra blank line.
    pub fn from_text(text: &str) -> Self {
        Self {
            chars: text.lines().map(|line| line.chars().collect()).collect(),
            edits: Vec::new(),
        }
    }

    /// Remove whitespace from the end of every line.
    pub fn strip_trailing_whitespace(&mut self) {
        for line in &mut self.chars {
            let length = line
                .iter()
                .rposition(|character| !character.is_whitespace())
                .map_or(0, |index| index + 1);
            line.truncate(length);
        }
    }

    /// Remove empty top and bottom lines, common left whitespace, and trailing
    /// whitespace from the buffer.
    pub fn strip_margin_whitespace(&mut self) {
        let is_blank = |line: &Vec<char>| line.iter().all(|character| character.is_whitespace());

        let first = self.chars.iter().position(|line| !is_blank(line));
        let last = self.chars.iter().rposition(|line| !is_blank(line));
        match (first, last) {
            (Some(first), Some(last)) => {
                self.chars = self.chars[first..=last].to_vec();
            }
            _ => {
                self.chars.clear();
                return;
            }
        }

        let left_margin = self
            .chars
            .iter()
            .filter(|line| !is_blank(line))
            .filter_map(|line| line.iter().position(|character| !character.is_whitespace()))
            .min()
            .unwrap_or(0);

        for line in &mut self.chars {
            if !line.is_empty() {
                line.drain(..left_margin.min(line.len() - 1));
            }
        }
        self.strip_trailing_whitespace();
    }

    /// Remove all committed content and pending edits.
    pub fn clear(&mut self) {
        self.chars.clear();
        self.edits.clear();
    }

    /// Iterate over all committed cells in row-major order.
    pub fn committed_cells(&self) -> impl Iterator<Item = (Point, char)> + '_ {
        self.chars.iter().enumerate().flat_map(|(y, line)| {
            line.iter()
                .copied()
                .enumerate()
                .map(move |(x, character)| (Point::new(x, y), character))
        })
    }

    /// Return the committed character at `point`.
    ///
    /// Pending edits are deliberately not considered; this matches the
    /// application's distinction between the saved buffer and a preview.
    pub fn get(&self, point: Point) -> Option<char> {
        self.chars
            .get(point.y)
            .and_then(|line| line.get(point.x))
            .copied()
    }

    /// Return the dimensions needed to display committed content and pending
    /// edits.
    pub fn bounds(&self) -> Point {
        let committed_width = self.chars.iter().map(Vec::len).max().unwrap_or(0);
        let pending_width = self
            .edits
            .iter()
            .map(|(point, _)| point.x + 1)
            .max()
            .unwrap_or(0);
        let pending_height = self
            .edits
            .iter()
            .map(|(point, _)| point.y + 1)
            .max()
            .unwrap_or(0);

        Point::new(max(committed_width, pending_width), max(self.chars.len(), pending_height))
    }

    /// Stage an edit using diagram stroke-overlap precedence.
    pub fn set_at(&mut self, point: Point, character: char) {
        self.set(point, character);
    }

    /// Stage an edit which replaces any existing character at `point`.
    pub fn set_at_forced(&mut self, point: Point, character: char) {
        self.edits.push((point, character));
    }

    /// Return whether a committed, non-whitespace character exists at `point`.
    pub fn visible(&self, point: Point) -> bool {
        self.get(point)
            .is_some_and(|character| !character.is_whitespace())
    }

    /// Iterate over pending edits in insertion order.
    pub fn pending_cells(&self) -> impl Iterator<Item = (Point, char)> + '_ {
        self.edits.iter().copied()
    }

    /// Drop all pending edits without changing committed content.
    pub fn discard_edits(&mut self) {
        self.edits.clear();
    }

    /// Snap the intermediate corner of a connector to a 45° path.
    pub fn snap45(&self, src: Point, dst: Point) -> Point {
        let delta = src.x.abs_diff(dst.x).min(src.y.abs_diff(dst.y));
        match slope(src, dst) {
            (-1, -1) => Point::new(dst.x + delta, dst.y + delta),
            (1, -1) => Point::new(dst.x - delta, dst.y + delta),
            (-1, 1) => Point::new(dst.x + delta, dst.y - delta),
            (1, 1) => Point::new(dst.x - delta, dst.y - delta),
            _ => src,
        }
    }

    /// Snap the intermediate corner of a connector to a 90° path.
    pub fn snap90(&self, src: Point, dst: Point) -> Point {
        if self.get(dst) == Some('-') { Point::new(dst.x, src.y) } else { Point::new(src.x, dst.y) }
    }

    /// Draw an arrowhead at `dst` for a connector drawn from `src`.
    pub fn draw_arrow_tip(&mut self, src: Point, dst: Point) {
        let north = dst.y > 0 && self.visible(Point::new(dst.x, dst.y - 1));
        let east = self.visible(Point::new(dst.x + 1, dst.y));
        let south = self.visible(Point::new(dst.x, dst.y + 1));
        let west = dst.x > 0 && self.visible(Point::new(dst.x - 1, dst.y));
        let tip = match slope(src, dst) {
            (0, -1) if north || (west && east) => '^', (0, -1) if west => '<', (0, -1) if east => '>', (0, -1) => '^',
            (1, 0) if east || (north && south) => '>', (1, 0) if north => '^', (1, 0) if south => 'v', (1, 0) => '>',
            (0, 1) if south || (east && west) => 'v', (0, 1) if east => '>', (0, 1) if west => '<', (0, 1) => 'v',
            (-1, 0) if west || (south && north) => '<', (-1, 0) if south => 'v', (-1, 0) if north => '^', (-1, 0) => '<',
            (1, 1) if east => '>', (1, 1) => 'v', (1, -1) if east => '>', (1, -1) => '^',
            (-1, 1) if dst.x == 0 => 'v', (-1, 1) if west => '<', (-1, 1) => 'v',
            (-1, -1) if dst.x == 0 => '^', (-1, -1) if west => '<', (-1, -1) => '^', _ => '+',
        };
        self.set_at_forced(dst, tip);
    }

    /// Draw a lowest-cost path around visible content and return its penultimate point.
    pub fn draw_path(&mut self, src: Point, dst: Point) -> Point {
        let points = astar(&src, |point| self.neighbors(*point), |point| cost(*point, dst), |point| *point == dst)
            .expect("an unbounded diagram always has a route").0;
        let penultimate = if points.len() > 1 {
            points[points.len() - 2]
        } else {
            src
        };
        let mut previous = src;
        for (index, point) in points.into_iter().enumerate() {
            self.set_at(point, if index == 0 { '+' } else { stroke(previous, point) });
            previous = point;
        }
        self.set_at(dst, '+');
        penultimate
    }

    fn neighbors(&self, point: Point) -> Vec<(Point, Cost)> {
        let mut result = Vec::with_capacity(8);
        for dy in -1isize..=1 { for dx in -1isize..=1 {
            if dx == 0 && dy == 0 { continue; }
            let x = point.x.checked_add_signed(dx); let y = point.y.checked_add_signed(dy);
            if let (Some(x), Some(y)) = (x, y) {
                let next = Point::new(x, y);
                let step = if dx == 0 || dy == 0 { 1.0 } else { std::f64::consts::SQRT_2 };
                result.push((next, Cost(step + if self.visible(next) { 64.0 } else { 0.0 })));
            }
        }}
        result
    }

    /// Draw a line between two points.
    pub fn draw_line(&mut self, src: Point, dst: Point) {
        let dx = dst.x as isize - src.x as isize;
        let dy = dst.y as isize - src.y as isize;
        let steps = max(dx.unsigned_abs(), dy.unsigned_abs());

        if steps == 0 {
            self.set(src, '+');
            return;
        }

        for i in 0..=steps {
            let x = src.x as isize + dx * i as isize / steps as isize;
            let y = src.y as isize + dy * i as isize / steps as isize;
            let point = Point::new(x as usize, y as usize);
            let c = if i == 0 || i == steps {
                '+'
            } else if dx == 0 {
                '|'
            } else if dy == 0 {
                '-'
            } else if (dx > 0) == (dy > 0) {
                '\\'
            } else {
                '/'
            };
            self.set(point, c);
        }
    }

    /// Apply all staged edits.
    pub fn commit(&mut self) {
        for (Point { x, y }, c) in self.edits.drain(..) {
            if self.chars.len() <= y {
                self.chars.resize_with(y + 1, Vec::new);
            }
            if self.chars[y].len() <= x {
                self.chars[y].resize(x + 1, ' ');
            }
            self.chars[y][x] = c;
        }
    }

    /// Render the buffer, placing `prefix` before every line.
    pub fn render(&self, prefix: &str) -> String {
        self.chars
            .iter()
            .flat_map(|line| prefix.chars().chain(line.iter().copied()).chain(std::iter::once('\n')))
            .collect()
    }

    fn set(&mut self, point: Point, character: char) {
        let precedence = |character| match character {
            '+' => 5,
            '-' => 4,
            '|' => 3,
            '/' => 2,
            '\\' => 1,
            _ => 0,
        };
        let overrides = |old| old == character || precedence(old) > precedence(character);

        let committed_overrides = self.get(point).is_some_and(overrides);
        let pending_overrides = self
            .edits
            .iter()
            .filter(|(position, _)| *position == point)
            .any(|(_, old)| overrides(*old));

        if !committed_overrides && !pending_overrides {
            self.edits.push((point, character));
        }
    }
}

#[derive(Copy, Clone, PartialEq, PartialOrd)]
struct Cost(f64);
impl Eq for Cost {}
impl Ord for Cost { fn cmp(&self, other: &Self) -> std::cmp::Ordering { self.partial_cmp(other).unwrap() } }
impl Add for Cost { type Output = Self; fn add(self, other: Self) -> Self { Self(self.0 + other.0) } }
impl Zero for Cost { fn zero() -> Self { Self(0.0) } fn is_zero(&self) -> bool { self.0 == 0.0 } }

fn cost(point: Point, destination: Point) -> Cost {
    let dx = point.x.abs_diff(destination.x) as f64;
    let dy = point.y.abs_diff(destination.y) as f64;
    Cost((dx - dy).abs() + std::f64::consts::SQRT_2 * dx.min(dy))
}

fn stroke(src: Point, dst: Point) -> char {
    match slope(src, dst) { (0, _) => '|', (_, 0) => '-', (x, y) if x == y => '\\', _ => '/' }
}

fn slope(src: Point, dst: Point) -> (isize, isize) {
    (
        (dst.x as isize - src.x as isize).signum(),
        (dst.y as isize - src.y as isize).signum(),
    )
}
