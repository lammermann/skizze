//! Cursive-specific preview state layered over the pure library buffer.

use cursive::{Rect, Vec2, XY};
use skizze::{Buffer as LibraryBuffer, PathMode, Point as LibraryPoint};
use std::{
    cmp::max,
    io::{self, BufReader, Read},
};

const CURSOR: char = '_';

/// A pending Cursive-coordinate cell edit.
#[derive(Copy, Clone, PartialEq, Eq)]
pub(crate) struct Cell {
    pub(crate) pos: Vec2,
    pub(crate) c: char,
}

impl Cell {
    pub(crate) fn pos(&self) -> Vec2 {
        self.pos
    }

    pub(crate) fn c(&self) -> char {
        self.c
    }

    pub(crate) fn is_whitespace(&self) -> bool {
        self.c.is_whitespace()
    }

    pub(crate) fn translate(mut self, by: XY<isize>) -> Self {
        self.pos = self.pos.saturating_add(by);
        self
    }
}

/// A cell prepared for terminal rendering.
#[derive(Copy, Clone, PartialEq, Eq)]
pub(crate) enum Char {
    Clean(Cell),
    Dirty(Cell),
    Cursor(Cell),
}

/// Preview edits and cursor state used by the terminal UI.
#[derive(Clone, Default, PartialEq, Eq)]
pub(crate) struct PreviewBuffer {
    chars: LibraryBuffer,
    pub(crate) edits: Vec<Cell>,
    pub(crate) cursor: Option<Vec2>,
}

fn point(position: Vec2) -> LibraryPoint {
    LibraryPoint::new(position.x, position.y)
}

fn vec2(position: LibraryPoint) -> Vec2 {
    Vec2::new(position.x, position.y)
}

fn precedence(c: char) -> usize {
    match c {
        '+' => 5,
        '-' => 4,
        '|' => 3,
        '/' => 2,
        '\\' => 1,
        _ => 0,
    }
}

impl PreviewBuffer {
    /// Set the cursor position for preview rendering.
    pub(crate) fn set_cursor(&mut self, pos: Vec2) {
        self.cursor = Some(pos);
    }

    /// Disable cursor preview rendering.
    pub(crate) fn drop_cursor(&mut self) {
        self.cursor = None;
    }

    /// Discard pending preview edits without changing committed content.
    pub(crate) fn discard_edits(&mut self) {
        self.edits.clear();
    }

    /// Add a pending edit, respecting connector overlap precedence unless forced.
    pub(crate) fn setv(&mut self, force: bool, pos: Vec2, c: char) {
        if force {
            self.edits.push(Cell { pos, c });
            return;
        }

        let max_precedence = precedence(c);
        let overrides = |existing| existing == c || precedence(existing) > max_precedence;
        let overridden = self.chars.get(LibraryPoint::new(pos.x, pos.y)).map(overrides).unwrap_or(false)
            || self.edits.iter().filter(|cell| cell.pos == pos).any(|cell| overrides(cell.c));
        if !overridden {
            self.edits.push(Cell { pos, c });
        }
    }

    /// Add a pending edit at the given coordinates.
    pub(crate) fn set(&mut self, force: bool, x: usize, y: usize, c: char) {
        self.setv(force, Vec2::new(x, y), c);
    }

    /// Commit pending preview edits to the library buffer.
    pub(crate) fn flush_edits(&mut self) {
        for Cell { pos, c } in self.edits.drain(..) {
            self.chars.set_at_forced(LibraryPoint::new(pos.x, pos.y), c);
        }
        self.chars.commit();
    }

    /// Return the committed library buffer, excluding preview edits and cursor state.
    pub(crate) fn committed_buffer(&self) -> &LibraryBuffer {
        &self.chars
    }

    /// Replace committed content and discard transient preview state.
    pub(crate) fn replace_committed_buffer(&mut self, buffer: LibraryBuffer) {
        self.chars = buffer;
        self.edits.clear();
        self.cursor = None;
    }

    /// Construct preview state from caller-supplied text bytes.
    pub(crate) fn read_from<R: Read>(r: R) -> io::Result<Self> {
        let mut text = String::new();
        BufReader::new(r).read_to_string(&mut text)?;
        Ok(Self {
            chars: LibraryBuffer::from_text(&text),
            edits: vec![],
            cursor: None,
        })
    }

    /// Get a committed cell, ignoring pending preview edits.
    pub(crate) fn getv(&self, pos: Vec2) -> Option<char> {
        self.chars.get(point(pos))
    }

    fn preview_library<F: FnOnce(&mut LibraryBuffer)>(&mut self, render: F) {
        let mut buffer = self.chars.clone();
        render(&mut buffer);
        self.edits.extend(buffer.pending_cells().map(|(position, c)| Cell {
            pos: vec2(position),
            c,
        }));
    }

    pub(crate) fn draw_line(&mut self, src: Vec2, dst: Vec2) {
        self.preview_library(|buffer| buffer.draw_line(point(src), point(dst)));
    }

    pub(crate) fn draw_arrow_tip(&mut self, src: Vec2, dst: Vec2) {
        self.preview_library(|buffer| buffer.draw_arrow_tip(point(src), point(dst)));
    }

    /// Erase non-whitespace committed cells in an inclusive rectangle.
    pub(crate) fn erase_box(&mut self, first: Vec2, second: Vec2) {
        self.preview_library(|buffer| buffer.erase_box(point(first), point(second)));
    }

    /// Draw a rectangular box using pure library geometry.
    pub(crate) fn draw_box(&mut self, top_left: Vec2, bottom_right: Vec2) {
        self.preview_library(|buffer| buffer.draw_box(point(top_left), point(bottom_right)));
    }

    /// Draw a line or arrow connector using pure library routing behavior.
    pub(crate) fn draw_connector(&mut self, src: Vec2, dst: Vec2, mode: PathMode, arrow: bool) {
        self.preview_library(|buffer| buffer.draw_connector(point(src), point(dst), mode, arrow));
    }

    pub(crate) fn snap45(&self, src: Vec2, dst: Vec2) -> Vec2 {
        vec2(self.chars.snap45(point(src), point(dst)))
    }

    pub(crate) fn snap90(&self, src: Vec2, dst: Vec2) -> Vec2 {
        vec2(self.chars.snap90(point(src), point(dst)))
    }

    pub(crate) fn draw_path(&mut self, src: Vec2, dst: Vec2) -> Vec2 {
        let mut path = self.chars.clone();
        let last = path.draw_path(point(src), point(dst));
        self.edits.extend(path.pending_cells().map(|(position, c)| Cell {
            pos: vec2(position),
            c,
        }));
        vec2(last)
    }

    /// Render visible committed cells, pending edits, and cursor state for a viewport.
    pub(crate) fn iter_within<'a>(
        &'a self,
        offset: Vec2,
        size: Vec2,
    ) -> impl Iterator<Item = Char> + 'a {
        let area = Rect::from_corners(offset, offset + size);
        self.chars
            .committed_cells()
            .filter(move |(point, _)| area.contains(Vec2::new(point.x, point.y)))
            .map(|(point, c)| Cell { pos: Vec2::new(point.x, point.y), c })
            .map(Char::Clean)
            .chain(self.edits.iter().copied().filter(move |cell| area.contains(cell.pos)).map(Char::Dirty))
            .chain(self.cursor.map(|pos| Cell { pos, c: CURSOR }).map(Char::Cursor))
    }

    pub(crate) fn strip_margin_whitespace(&mut self) {
        self.chars.strip_margin_whitespace();
    }

    /// Clears committed content, pending preview edits, and the cursor.
    pub(crate) fn clear(&mut self) {
        self.chars.clear();
        self.edits.clear();
        self.cursor = None;
    }

    /// Returns the viewport size required to display all preview state.
    pub(crate) fn bounds(&self) -> Vec2 {
        let library_bounds = self.chars.bounds();
        let mut bounds = Vec2::new(library_bounds.x, library_bounds.y);
        bounds.x = max(bounds.x, self.edits.iter().map(|cell| cell.pos.x + 1).max().unwrap_or(0));
        bounds.y = max(bounds.y, self.edits.iter().map(|cell| cell.pos.y + 1).max().unwrap_or(0));
        if let Some(cursor) = self.cursor {
            bounds.x = max(bounds.x, cursor.x + 1);
            bounds.y = max(bounds.y, cursor.y + 1);
        }
        bounds
    }
}
