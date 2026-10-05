use crate::Buffer;

/// Pure committed-document state with save tracking and undo/redo history.
#[derive(Clone, Default, Eq, PartialEq)]
pub struct Document {
    buffer: Buffer,
    saved: Buffer,
    undo: Vec<Buffer>,
    redo: Vec<Buffer>,
}

impl Document {
    /// Create a document from textual content and mark it as saved.
    pub fn from_text(text: &str) -> Self {
        Self::from_buffer(Buffer::from_text(text))
    }

    /// Create a document from committed buffer content and mark it as saved.
    pub fn from_buffer(buffer: Buffer) -> Self {
        Self { saved: buffer.clone(), buffer, undo: vec![], redo: vec![] }
    }

    /// Return the committed diagram buffer.
    pub fn buffer(&self) -> &Buffer {
        &self.buffer
    }

    /// Whether the document differs from its last saved state.
    pub fn is_dirty(&self) -> bool {
        self.buffer != self.saved
    }

    /// Apply an atomic edit, recording it only when it changes the document.
    pub fn edit<F: FnOnce(&mut Buffer)>(&mut self, edit: F) {
        let before = self.buffer.clone();
        edit(&mut self.buffer);
        self.buffer.commit();
        if self.buffer != before {
            self.undo.push(before);
            self.redo.clear();
        }
    }

    /// Replace committed content as one undoable edit.
    pub fn replace_buffer(&mut self, buffer: Buffer) {
        if self.buffer != buffer {
            self.undo.push(std::mem::replace(&mut self.buffer, buffer));
            self.redo.clear();
        }
    }

    /// Normalize whitespace as required before saving, recording a change when needed.
    pub fn normalize_for_save(&mut self, strip_margin: bool, keep_trailing: bool) {
        self.edit(|buffer| {
            if strip_margin {
                buffer.strip_margin_whitespace();
            } else if !keep_trailing {
                buffer.strip_trailing_whitespace();
            }
        });
    }

    /// Render a margin-trimmed copy for clipboard output without changing the document.
    pub fn render_trimmed(&self, prefix: &str) -> String {
        let mut buffer = self.buffer.clone();
        buffer.strip_margin_whitespace();
        let mut rendered = buffer.render(prefix);
        if rendered.ends_with('\n') {
            rendered.pop();
        }
        rendered
    }

    /// Mark the current committed content as saved.
    pub fn mark_saved(&mut self) {
        self.saved = self.buffer.clone();
    }

    /// Replace content from caller-provided text, clearing document history.
    pub fn load_text(&mut self, text: &str) {
        self.buffer = Buffer::from_text(text);
        self.saved = self.buffer.clone();
        self.undo.clear();
        self.redo.clear();
    }

    /// Undo the last content change.
    pub fn undo(&mut self) -> bool {
        if let Some(previous) = self.undo.pop() {
            self.redo.push(std::mem::replace(&mut self.buffer, previous));
            true
        } else {
            false
        }
    }

    /// Redo the last undone content change.
    pub fn redo(&mut self) -> bool {
        if let Some(next) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.buffer, next));
            true
        } else {
            false
        }
    }
}
