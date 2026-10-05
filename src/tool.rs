use crate::{Buffer, Point};

/// Routing mode for line and arrow connectors.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub enum PathMode {
    #[default]
    Snap90,
    Snap45,
    Routed,
}

impl Buffer {
    /// Stage spaces over non-whitespace cells in the inclusive rectangle.
    pub fn erase_box(&mut self, first: Point, second: Point) {
        let min_x = first.x.min(second.x);
        let max_x = first.x.max(second.x);
        let min_y = first.y.min(second.y);
        let max_y = first.y.max(second.y);
        let cells: Vec<_> = self
            .committed_cells()
            .filter(|(point, c)| {
                (min_x..=max_x).contains(&point.x)
                    && (min_y..=max_y).contains(&point.y)
                    && !c.is_whitespace()
            })
            .map(|(point, _)| point)
            .collect();
        for point in cells {
            self.set_at_forced(point, ' ');
        }
    }

    /// Draw a rectangular box between opposite corners.
    pub fn draw_box(&mut self, top_left: Point, bottom_right: Point) {
        self.draw_line(top_left, Point::new(bottom_right.x, top_left.y));
        self.draw_line(Point::new(bottom_right.x, top_left.y), bottom_right);
        self.draw_line(bottom_right, Point::new(top_left.x, bottom_right.y));
        self.draw_line(Point::new(top_left.x, bottom_right.y), top_left);
    }

    /// Draw a connector using the selected routing mode.
    pub fn draw_connector(&mut self, src: Point, dst: Point, mode: PathMode, arrow: bool) {
        if mode == PathMode::Routed {
            let last = self.draw_path(src, dst);
            if arrow {
                self.draw_arrow_tip(last, dst);
            }
            return;
        }

        let middle = match mode {
            PathMode::Snap90 => self.snap90(src, dst),
            PathMode::Snap45 => self.snap45(src, dst),
            PathMode::Routed => unreachable!(),
        };

        if middle != dst {
            self.draw_line(src, middle);
            self.draw_line(middle, dst);
            if arrow {
                self.draw_arrow_tip(middle, dst);
            }
        } else {
            self.draw_line(src, dst);
            if arrow {
                self.draw_arrow_tip(src, dst);
            }
        }
    }
}
