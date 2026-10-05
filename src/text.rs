use crate::Point;

/// Pure multiline text editing state used by text-entry tools.
#[derive(Clone, Default, Eq, PartialEq)]
pub struct TextEdit {
    lines: Vec<Vec<char>>,
    cursor: Point,
}

impl TextEdit {
    pub fn begin(&mut self) { self.lines = vec![vec![]]; self.cursor = Point::default(); }
    pub fn lines(&self) -> &[Vec<char>] { &self.lines }
    pub fn cursor(&self) -> Point { self.cursor }
    pub fn insert(&mut self, c: char) { self.lines[self.cursor.y].insert(self.cursor.x, c); self.cursor.x += 1; }
    pub fn left(&mut self) { self.cursor.x = self.cursor.x.saturating_sub(1); }
    pub fn right(&mut self) { self.cursor.x = self.cursor.x.min(self.lines[self.cursor.y].len()).saturating_add(1).min(self.lines[self.cursor.y].len()); }
    pub fn up(&mut self) { self.cursor.y = self.cursor.y.saturating_sub(1); self.cursor.x = self.cursor.x.min(self.lines[self.cursor.y].len()); }
    pub fn down(&mut self) { self.cursor.y = (self.cursor.y + 1).min(self.lines.len() - 1); self.cursor.x = self.cursor.x.min(self.lines[self.cursor.y].len()); }
    pub fn newline(&mut self) { let next = self.lines[self.cursor.y].split_off(self.cursor.x); self.cursor.y += 1; self.cursor.x = 0; self.lines.insert(self.cursor.y, next); }
    pub fn backspace(&mut self) { if self.cursor.x > 0 { self.lines[self.cursor.y].remove(self.cursor.x - 1); self.cursor.x -= 1; } else if self.cursor.y > 0 { let next = self.lines.remove(self.cursor.y); self.cursor.y -= 1; self.cursor.x = self.lines[self.cursor.y].len(); self.lines[self.cursor.y].extend(next); } }
    pub fn reset(&mut self) { self.lines.clear(); self.cursor = Point::default(); }
}
