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
