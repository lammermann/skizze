// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.
use super::{preview_buffer::{Cell, Char, PreviewBuffer}, tools::*, Options};
use clipboard::{ClipboardContext, ClipboardProvider};
use cursive::{
    event::{Event, EventResult, MouseButton::*, MouseEvent::*},
    theme::ColorStyle,
    view::{scroll::Scroller, View},
    views::ScrollView,
    Printer, Vec2,
};
use lazy_static::lazy_static;
use skizze::Document;
use parking_lot::{Mutex, RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::{
    cmp::{max, min},
    error::Error,
    fs::{self, File, OpenOptions},
    io::{self, ErrorKind, Write},
    path::{Path, PathBuf},
    rc::Rc,
};

pub(crate) const CONSUMED: Option<EventResult> = Some(EventResult::Consumed(None));

macro_rules! intercept_scrollbar {
    ($ctx:expr, $event:expr) => {{
        lazy_static! {
            static ref LAST_LPRESS: Mutex<Option<Vec2>> = Mutex::new(None);
        }

        if let Event::Mouse {
            offset,
            position: pos,
            event,
        } = $event
        {
            match event {
                Press(Left) if $ctx.on_scrollbar(*offset, *pos) => {
                    *LAST_LPRESS.lock() = Some(*pos);
                    return None;
                }

                Press(Left) => {
                    *LAST_LPRESS.lock() = Some(*pos);
                }

                Hold(Left)
                    if LAST_LPRESS
                        .lock()
                        .map(|pos| $ctx.on_scrollbar(*offset, pos))
                        .unwrap_or(false) =>
                {
                    return None;
                }

                Release(Left)
                    if LAST_LPRESS
                        .lock()
                        .take()
                        .map(|pos| $ctx.on_scrollbar(*offset, pos))
                        .unwrap_or(false) =>
                {
                    return None;
                }

                _ => {}
            }
        }
    }};
}

macro_rules! intercept_pan {
    ($ctx:expr, $event:expr) => {{
        lazy_static! {
            static ref OLD: Mutex<Option<Vec2>> = Mutex::new(None);
        }

        if let Event::Mouse {
            position: pos,
            event,
            ..
        } = $event
        {
            match event {
                Press(Right) => {
                    *OLD.lock() = Some(*pos);
                    return CONSUMED;
                }

                Hold(Right) if OLD.lock().is_none() => {
                    *OLD.lock() = Some(*pos);
                    return CONSUMED;
                }

                Hold(Right) => {
                    let old = OLD.lock().replace(*pos).unwrap();

                    let offset = ($ctx.0.content_viewport())
                        .top_left()
                        .map_x(|x| drag(x, pos.x, old.x))
                        .map_y(|y| drag(y, pos.y, old.y));

                    $ctx.0.set_offset(offset);

                    let p = $ctx.0.content_viewport();
                    let i = $ctx.0.inner_size();

                    let mut editor = $ctx.0.get_inner_mut().write();
                    if pos.x < old.x && within((old.x - pos.x + 1) * 4, p.right(), i.x) {
                        editor.canvas.x += old.x - pos.x;
                    }
                    if pos.y < old.y && within((old.y - pos.y + 1) * 2, p.bottom(), i.y) {
                        editor.canvas.y += old.y - pos.y;
                    }

                    return CONSUMED;
                }

                Release(Right) => {
                    *OLD.lock() = None;
                    return CONSUMED;
                }

                _ => {}
            }
        }
    }};
}

fn drag(x: usize, new: usize, old: usize) -> usize {
    if new > old {
        x.saturating_sub(new - old)
    } else {
        x + (old - new)
    }
}

pub(crate) struct EditorCtx<'a>(&'a mut ScrollView<EditorView>);

impl<'a> EditorCtx<'a> {
    /// Returns a new `EditorCtx`.
    pub(super) fn new(view: &'a mut ScrollView<EditorView>) -> Self {
        Self(view)
    }

    /// Handles an event using the active tool.
    pub(super) fn on_event(&mut self, event: &Event) -> Option<EventResult> {
        intercept_scrollbar!(self, event);
        intercept_pan!(self, event);

        let mut tool = self.0.get_inner_mut().write().active_tool.take().unwrap();
        let res = tool.on_event(self, event);
        self.0.get_inner_mut().write().active_tool = Some(tool);

        res
    }

    /// Returns `true` if `pos` is located on a scrollbar.
    fn on_scrollbar(&self, offset: Vec2, pos: Vec2) -> bool {
        let core = self.0.get_scroller();
        let max = core.last_size() + offset;
        let min = max - core.scrollbar_size();

        (min.x..=max.x).contains(&pos.x) || (min.y..=max.y).contains(&pos.y)
    }

    /// If `event` is a mouse event, relativize its position to the canvas plane.
    pub(crate) fn relativize(&self, event: &Event) -> Event {
        let mut event = event.clone();
        if let Event::Mouse {
            offset, position, ..
        } = &mut event
        {
            let tl = self.0.content_viewport().top_left();
            *position = position.saturating_sub(*offset) + tl;
        }
        event
    }

    /// Scroll to `pos`, moving at least `step_x` & `step_y` respectively if the x or y
    /// scroll offset needs to be modified.
    pub(crate) fn scroll_to(&mut self, pos: Vec2, step_x: usize, step_y: usize) {
        let port = self.0.content_viewport();
        let mut offset = port.top_left();

        if pos.x >= port.right() {
            offset.x += max(step_x, pos.x - port.right());
        } else if pos.x <= port.left() {
            offset.x -= max(min(step_x, offset.x), port.left() - pos.x);
        }
        if pos.y >= port.bottom() {
            offset.y += max(step_y, pos.y - port.bottom());
        } else if pos.y <= port.top() {
            offset.y -= max(min(step_y, offset.y), port.top() - pos.y);
        }
        self.0.set_offset(offset);

        // BUG: scrolling lags behind changes to the canvas bounds by 1 render tick. in
        // order to truly fix the issue, we need to implement scrolling as a function of
        // the editor itself.
        let mut editor = self.0.get_inner_mut().write();

        if pos.x + 1 >= editor.canvas.x {
            editor.canvas.x += max(step_x, (pos.x + 1) - editor.canvas.x);
        }
        if pos.y + 1 >= editor.canvas.y {
            editor.canvas.y += max(step_y, (pos.y + 1) - editor.canvas.y);
        }
    }

    /// Scroll to the edit buffer's current cursor, if one exists.
    pub(crate) fn scroll_to_cursor(&mut self) {
        let pos = self.0.get_inner_mut().read().buffer.cursor;

        if let Some(pos) = pos {
            self.scroll_to(pos, 1, 1);
        }
    }

    /// Modify the edit buffer using `render`, flushing any changes and saving a snapshot
    /// of the buffer's prior state in the editor's undo history.
    pub(crate) fn clobber<R: FnOnce(&mut PreviewBuffer)>(&mut self, render: R) {
        let mut editor = self.0.get_inner_mut().write();

        editor.with_snapshot(|ed| {
            render(&mut ed.buffer);
            ed.buffer.flush_edits();
            ed.buffer.drop_cursor();
        });
    }

    /// Modify the edit buffer using `render`, without flushing any changes.
    pub(crate) fn preview<R: FnOnce(&mut PreviewBuffer)>(&mut self, render: R) {
        let mut editor = self.0.get_inner_mut().write();
        editor.buffer.discard_edits();
        render(&mut editor.buffer);
    }
}

#[derive(Clone)]
pub(crate) struct EditorView {
    inner: Rc<RwLock<Editor>>,
}

impl View for EditorView {
    fn draw(&self, p: &Printer<'_, '_>) {
        let mut normal = print_styled(ColorStyle::primary());
        let mut change = print_styled(ColorStyle::highlight_inactive());
        let mut cursor = print_styled(ColorStyle::highlight());

        for c in self.read().buffer.iter_within(p.content_offset, p.size) {
            match c {
                Char::Clean(Cell { pos, c }) => normal(p, pos, c),
                Char::Dirty(Cell { pos, c }) => change(p, pos, c),
                Char::Cursor(Cell { pos, c }) => cursor(p, pos, c),
            }
        }
    }

    fn required_size(&mut self, size: Vec2) -> Vec2 {
        let mut editor = self.write();

        let buf_bounds = editor.buffer.bounds();

        editor.canvas = Vec2 {
            x: max(buf_bounds.x, editor.canvas.x),
            y: max(buf_bounds.y, editor.canvas.y),
        };

        Vec2 {
            x: max(size.x, editor.canvas.x),
            y: max(size.y, editor.canvas.y),
        }
    }
}

impl EditorView {
    pub(crate) fn new(inner: Editor) -> Self {
        Self {
            inner: Rc::new(RwLock::new(inner)),
        }
    }

    pub(crate) fn read(&self) -> RwLockReadGuard<Editor> {
        self.inner.read()
    }

    pub(crate) fn write(&self) -> RwLockWriteGuard<Editor> {
        self.inner.write()
    }
}

pub(crate) struct Editor {
    opts: Options,
    buffer: PreviewBuffer,
    document: Document,
    active_tool: Option<Box<dyn Tool>>,
    canvas: Vec2,
    rendered: String,
}

fn print_styled(style: ColorStyle) -> impl FnMut(&Printer<'_, '_>, Vec2, char) {
    let mut buf = vec![0; 4];
    move |p, pos, c| {
        p.with_color(style, |p| p.print(pos, c.encode_utf8(&mut buf)));
    }
}

impl Editor {
    /// Open an editor instance with the provided options.
    pub(crate) fn open(mut opts: Options) -> io::Result<Self> {
        let file = opts.file.take();

        let mut tool = BoxTool::default();
        tool.load_opts(&opts);

        let mut editor = Self {
            opts,
            buffer: PreviewBuffer::default(),
            document: Document::default(),
            active_tool: Some(Box::new(tool)),
            canvas: Vec2::new(0, 0),
            rendered: String::default(),
        };

        if let Some(path) = file {
            editor.open_file(path)?;
        }

        Ok(editor)
    }

    /// Mutate the loaded options with `apply`.
    pub(crate) fn mut_opts<F: FnOnce(&mut Options)>(&mut self, apply: F) {
        apply(&mut self.opts);
        if let Some(tool) = self.active_tool.as_mut() {
            tool.load_opts(&self.opts);
        }
    }

    /// Returns `true` if the buffer has been modified since the last save.
    pub(crate) fn is_dirty(&self) -> bool {
        self.document.is_dirty()
    }

    /// Set the active tool.
    pub(crate) fn set_tool<T: Tool + 'static>(&mut self, mut tool: T) {
        self.buffer.discard_edits();
        self.buffer.drop_cursor();
        tool.load_opts(&self.opts);
        self.active_tool = Some(Box::new(tool));
    }

    /// Returns the active tool as a human readable string.
    pub(crate) fn active_tool(&self) -> String {
        format!("({})", self.active_tool.as_ref().unwrap())
    }

    /// Returns the current save path.
    pub(crate) fn path(&self) -> Option<&PathBuf> {
        self.opts.file.as_ref()
    }

    /// Clear all buffer state and begin a blank diagram.
    pub(crate) fn clear(&mut self) {
        self.opts.file = None;
        self.buffer.clear();
        self.document = Document::default();
        self.canvas = Vec2::new(0, 0);
    }

    /// Open the file at `path`, discarding any unsaved changes to the current file, if
    /// there are any.
    ///
    /// No modifications have been performed if this returns `Err(_)`.
    pub(crate) fn open_file<P: AsRef<Path>>(&mut self, path: P) -> io::Result<()> {
        let buffer = OpenOptions::new()
            .read(true)
            .open(path.as_ref())
            .and_then(PreviewBuffer::read_from);

        let buffer = match buffer {
            Err(e) if e.kind() == ErrorKind::NotFound => None,
            r => Some(r?),
        };

        self.clear();
        self.opts.file = Some(path.as_ref().into());
        if let Some(buf) = buffer {
            self.document = Document::from_buffer(buf.committed_buffer().clone());
            self.buffer = buf;
        }

        Ok(())
    }

    /// Save the current buffer contents to disk.
    ///
    /// Returns `Ok(true)` if the buffer was saved, and `Ok(false)` if there is no path
    /// configured for saving.
    ///
    /// If the configured save path does not exist, this will recursively create it.
    pub(crate) fn save(&mut self) -> io::Result<bool> {
        if let Some(path) = self.path() {
            path.parent().map(fs::create_dir_all).transpose()?;

            let file = OpenOptions::new()
                .read(false)
                .write(true)
                .create(true)
                .truncate(true)
                .open(path)?;

            self.render_to_file(file)?;
            self.document.mark_saved();
        }

        Ok(self.path().is_some())
    }

    /// Save the current buffer contents to the file at `path`, and setting that as the
    /// new path for future calls to `save`.
    pub(crate) fn save_as<P: AsRef<Path>>(&mut self, path: P) -> io::Result<()> {
        self.opts.file = Some(path.as_ref().into());
        self.save()?;

        Ok(())
    }

    /// Render to `file`, performing whitespace cleanup if enabled.
    fn render_to_file(&mut self, mut file: File) -> io::Result<()> {
        self.canvas = Vec2::new(0, 0);

        self.document
            .replace_buffer(self.buffer.committed_buffer().clone());
        self.document
            .normalize_for_save(self.opts.strip_margin_ws, self.opts.keep_trailing_ws);
        self.buffer
            .replace_committed_buffer(self.document.buffer().clone());

        self.rendered = self.document.buffer().render("");

        file.write_all(self.rendered.as_bytes())?;
        file.flush()?;
        file.sync_all()?;

        Ok(())
    }

    /// Render to the clipboard, prefixing all lines with `prefix`.
    ///
    /// Trims all margins in the output without changing the buffer's state.
    pub(crate) fn render_to_clipboard(&self, prefix: &str) -> Result<(), Box<dyn Error>> {
        let mut ctx = ClipboardContext::new()?;

        ctx.set_contents(self.document.render_trimmed(prefix))
    }

    /// Trim all whitespace from margins.
    pub(crate) fn trim_margins(&mut self) {
        self.with_snapshot(|ed| {
            ed.canvas = Vec2::new(0, 0);
            ed.buffer.strip_margin_whitespace();
        });
    }

    /// Take a snapshot of the buffer, discard any pending edits, and run `apply`. If
    /// the buffer was modified, mark it as dirty. Otherwise, remove the snapshot.
    ///
    /// Use this function to execute any buffer modification that should be saved in the
    /// undo history.
    fn with_snapshot<F: FnOnce(&mut Self)>(&mut self, apply: F) {
        self.buffer.discard_edits();
        apply(self);
        self.document
            .replace_buffer(self.buffer.committed_buffer().clone());
    }

    /// Undo the last buffer modification.
    ///
    /// Returns `false` if there was nothing to undo.
    pub(crate) fn undo(&mut self) -> bool {
        let undone = self.document.undo();
        if undone {
            self.buffer
                .replace_committed_buffer(self.document.buffer().clone());
        }
        undone
    }

    /// Redo the last undone buffer modification.
    ///
    /// Returns `false` if there was nothing to redo.
    pub(crate) fn redo(&mut self) -> bool {
        let redone = self.document.redo();
        if redone {
            self.buffer
                .replace_committed_buffer(self.document.buffer().clone());
        }
        redone
    }
}

/// Returns `true` if `a` is within `w` of `b` (inclusive).
fn within(w: usize, a: usize, b: usize) -> bool {
    diff(a, b) <= w
}

/// Returns the absolute difference between `a` and `b`.
fn diff(a: usize, b: usize) -> usize {
    (a as isize - b as isize).abs() as usize
}

#[cfg(test)]
mod tests {
    use super::*;
    use cursive::event::{Key, MouseEvent};
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn point(x: usize, y: usize) -> Vec2 {
        Vec2::new(x, y)
    }

    fn buffer(input: &str) -> PreviewBuffer {
        PreviewBuffer::read_from(input.as_bytes()).unwrap()
    }

    fn text(buf: &PreviewBuffer) -> String {
        buf.committed_buffer().render("")
    }

    fn editor() -> Editor {
        Editor::open(Options {
            path_mode: PathMode::Snap90,
            keep_trailing_ws: false,
            strip_margin_ws: false,
            file: None,
        })
        .unwrap()
    }

    fn tool_view() -> ScrollView<EditorView> {
        let view = EditorView {
            inner: Rc::new(RwLock::new(editor())),
        };
        let mut scroll = ScrollView::new(view).scroll_x(true).scroll_y(true);
        scroll.layout(Vec2::new(20, 8));
        scroll
    }

    fn left_mouse(event: MouseEvent, position: Vec2) -> Event {
        Event::Mouse {
            offset: Vec2::new(0, 0),
            position,
            event,
        }
    }

    struct TempFile(PathBuf);

    impl TempFile {
        fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let path = std::env::temp_dir().join(format!(
                "skizze-test-{}-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
            ));
            Self(path)
        }
    }

    impl Drop for TempFile {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }

    #[test]
    fn reading_and_rendering_preserve_lines_and_prefixes() {
        let buf = buffer("  a\n\n b  \n");
        assert_eq!(text(&buf), "  a\n\n b  \n");
        assert_eq!(buf.committed_buffer().render("# "), "#   a\n# \n#  b  \n");
        assert_eq!(buf.getv(point(2, 0)), Some('a'));
        assert_eq!(buf.getv(point(0, 1)), None);
    }

    #[test]
    fn whitespace_cleanup_has_distinct_trailing_and_margin_modes() {
        let original = skizze::Buffer::from_text("   \n  a  \n    b \n \n");
        let mut trailing = original.clone();
        trailing.strip_trailing_whitespace();
        assert_eq!(trailing.render(""), "\n  a\n    b\n\n");
        let mut margins = original;
        margins.strip_margin_whitespace();
        assert_eq!(margins.render(""), "a\n  b\n");
    }

    #[test]
    fn routed_arrow_tool_avoids_a_component_and_draws_an_arrowhead() {
        let mut view = tool_view();
        let obstacle = buffer("  +-----+\n  | API |\n  +-----+\n");
        view.get_inner_mut().write().buffer = obstacle.clone();
        let mut tool = ArrowTool::default();
        tool.load_opts(&Options {
            path_mode: PathMode::Routed,
            keep_trailing_ws: false,
            strip_margin_ws: false,
            file: None,
        });

        {
            let mut ctx = EditorCtx::new(&mut view);
            tool.on_event(&mut ctx, &left_mouse(Press(Left), point(0, 1)));
            tool.on_event(&mut ctx, &left_mouse(Hold(Left), point(10, 1)));
            tool.on_event(&mut ctx, &left_mouse(Release(Left), point(10, 1)));
        }

        let diagram = &view.get_inner().read().buffer;
        for y in 0..3 {
            for x in 2..9 {
                assert_eq!(diagram.getv(point(x, y)), obstacle.getv(point(x, y)));
            }
        }
        assert!(matches!(diagram.getv(point(10, 1)), Some('^' | '>' | 'v' | '<')));
    }

    #[test]
    fn arrow_tool_handles_left_edge_and_adjacent_collision_tips() {
        let mut edge_view = tool_view();
        let mut edge_tool = ArrowTool::default();
        edge_tool.load_opts(&Options {
            path_mode: PathMode::Snap45,
            keep_trailing_ws: false,
            strip_margin_ws: false,
            file: None,
        });
        {
            let mut ctx = EditorCtx::new(&mut edge_view);
            edge_tool.on_event(&mut ctx, &left_mouse(Press(Left), point(2, 1)));
            edge_tool.on_event(&mut ctx, &left_mouse(Hold(Left), point(0, 3)));
            edge_tool.on_event(&mut ctx, &left_mouse(Release(Left), point(0, 3)));
        }
        assert_eq!(edge_view.get_inner().read().buffer.getv(point(0, 3)), Some('v'));

        let mut collision_view = tool_view();
        collision_view.get_inner_mut().write().buffer = buffer("  x\n");
        let mut collision_tool = ArrowTool::default();
        {
            let mut ctx = EditorCtx::new(&mut collision_view);
            collision_tool.on_event(&mut ctx, &left_mouse(Press(Left), point(0, 1)));
            collision_tool.on_event(&mut ctx, &left_mouse(Hold(Left), point(2, 1)));
            collision_tool.on_event(&mut ctx, &left_mouse(Release(Left), point(2, 1)));
        }
        assert_eq!(collision_view.get_inner().read().buffer.getv(point(2, 1)), Some('^'));
    }

    #[test]
    fn text_tool_supports_multiline_editing_cancel_and_commit() {
        let mut view = tool_view();
        let mut tool = TextTool::default();

        {
            let mut ctx = EditorCtx::new(&mut view);
            tool.on_event(&mut ctx, &left_mouse(Press(Left), point(2, 1)));
            tool.on_event(&mut ctx, &Event::Char('h'));
            tool.on_event(&mut ctx, &Event::Char('i'));
            tool.on_event(&mut ctx, &Event::Key(Key::Enter));
            tool.on_event(&mut ctx, &Event::Char('!'));
            tool.on_event(&mut ctx, &Event::Key(Key::Up));
            tool.on_event(&mut ctx, &Event::Key(Key::Backspace));
            tool.on_event(&mut ctx, &Event::Key(Key::Esc));
        }
        assert_eq!(text(&view.get_inner().read().buffer), "");

        {
            let mut ctx = EditorCtx::new(&mut view);
            tool.on_event(&mut ctx, &left_mouse(Press(Left), point(2, 1)));
            tool.on_event(&mut ctx, &Event::Char('o'));
            tool.on_event(&mut ctx, &Event::Char('k'));
            tool.on_event(&mut ctx, &left_mouse(Press(Left), point(2, 1)));
        }
        assert_eq!(text(&view.get_inner().read().buffer), "\n  ok\n");
    }

    #[test]
    fn text_tool_lets_a_user_insert_text_at_the_cursor() {
        let mut view = tool_view();
        let mut tool = TextTool::default();

        {
            let mut ctx = EditorCtx::new(&mut view);
            tool.on_event(&mut ctx, &left_mouse(Press(Left), point(2, 1)));
            for c in "cat".chars() {
                tool.on_event(&mut ctx, &Event::Char(c));
            }
            // Move between "c" and "a", then insert a character there.
            tool.on_event(&mut ctx, &Event::Key(Key::Left));
            tool.on_event(&mut ctx, &Event::Key(Key::Left));
            tool.on_event(&mut ctx, &Event::Char('r'));
            // Moving right at the end stays at the end instead of creating a gap.
            tool.on_event(&mut ctx, &Event::Key(Key::Right));
            tool.on_event(&mut ctx, &Event::Key(Key::Right));
            tool.on_event(&mut ctx, &Event::Key(Key::Right));
            tool.on_event(&mut ctx, &Event::Char('!'));
            tool.on_event(&mut ctx, &left_mouse(Press(Left), point(2, 1)));
        }

        assert_eq!(text(&view.get_inner().read().buffer), "\n  crat!\n");
    }

    #[test]
    fn text_tool_keeps_the_cursor_on_a_shorter_line() {
        let mut view = tool_view();
        let mut tool = TextTool::default();

        {
            let mut ctx = EditorCtx::new(&mut view);
            tool.on_event(&mut ctx, &left_mouse(Press(Left), point(0, 0)));
            tool.on_event(&mut ctx, &Event::Char('a'));
            tool.on_event(&mut ctx, &Event::Key(Key::Enter));
            for c in "wide".chars() {
                tool.on_event(&mut ctx, &Event::Char(c));
            }
            // Going up from column four lands at the end of the one-character line.
            tool.on_event(&mut ctx, &Event::Key(Key::Up));
            tool.on_event(&mut ctx, &Event::Char('X'));
            // Going back down keeps the current column when that line has room.
            tool.on_event(&mut ctx, &Event::Key(Key::Down));
            tool.on_event(&mut ctx, &Event::Char('Y'));
            tool.on_event(&mut ctx, &left_mouse(Press(Left), point(0, 0)));
        }

        assert_eq!(text(&view.get_inner().read().buffer), "aX\nwiYde\n");
    }

    #[test]
    fn text_tool_backspace_at_the_start_of_a_line_joins_it_to_the_previous_line() {
        let mut view = tool_view();
        let mut tool = TextTool::default();

        {
            let mut ctx = EditorCtx::new(&mut view);
            tool.on_event(&mut ctx, &left_mouse(Press(Left), point(0, 0)));
            for c in "hello".chars() {
                tool.on_event(&mut ctx, &Event::Char(c));
            }
            // Split before the word, then use Backspace at the start of line two.
            for _ in 0..5 {
                tool.on_event(&mut ctx, &Event::Key(Key::Left));
            }
            tool.on_event(&mut ctx, &Event::Key(Key::Enter));
            tool.on_event(&mut ctx, &Event::Key(Key::Backspace));
            tool.on_event(&mut ctx, &left_mouse(Press(Left), point(0, 0)));
        }

        assert_eq!(text(&view.get_inner().read().buffer), "hello\n");
    }

    #[test]
    fn text_tool_deletes_before_the_cursor_with_backspace_and_delete() {
        let mut view = tool_view();
        let mut tool = TextTool::default();

        {
            let mut ctx = EditorCtx::new(&mut view);
            tool.on_event(&mut ctx, &left_mouse(Press(Left), point(0, 0)));
            for c in "abcd".chars() {
                tool.on_event(&mut ctx, &Event::Char(c));
            }
            tool.on_event(&mut ctx, &Event::Key(Key::Left));
            tool.on_event(&mut ctx, &Event::Key(Key::Left));
            tool.on_event(&mut ctx, &Event::Key(Key::Backspace));
            tool.on_event(&mut ctx, &Event::Key(Key::Del));
            tool.on_event(&mut ctx, &left_mouse(Press(Left), point(0, 0)));
        }

        assert_eq!(text(&view.get_inner().read().buffer), "cd\n");
    }

    #[test]
    fn right_mouse_drag_pans_the_viewport_in_both_directions() {
        let mut view = tool_view();
        view.get_inner_mut().write().canvas = point(100, 100);
        view.layout(Vec2::new(20, 8));

        {
            let mut ctx = EditorCtx::new(&mut view);
            ctx.on_event(&left_mouse(Press(Right), point(10, 5)));
            ctx.on_event(&left_mouse(Hold(Right), point(6, 2)));
        }
        assert_eq!(view.content_viewport().top_left(), point(4, 3));

        {
            let mut ctx = EditorCtx::new(&mut view);
            ctx.on_event(&left_mouse(Hold(Right), point(9, 4)));
            ctx.on_event(&left_mouse(Release(Right), point(9, 4)));
        }
        assert_eq!(view.content_viewport().top_left(), point(1, 1));
    }

    #[test]
    fn text_tool_scrolls_the_viewport_when_typing_beyond_its_edge() {
        let mut view = tool_view();
        let mut tool = TextTool::default();

        {
            let mut ctx = EditorCtx::new(&mut view);
            tool.on_event(&mut ctx, &left_mouse(Press(Left), point(30, 15)));
            tool.on_event(&mut ctx, &Event::Char('x'));
        }
        // The next render makes the newly expanded canvas available to the
        // scroll view; subsequent input must then bring the cursor on-screen.
        view.layout(Vec2::new(20, 8));
        {
            let mut ctx = EditorCtx::new(&mut view);
            tool.on_event(&mut ctx, &Event::Char('y'));
        }

        assert!(view.content_viewport().top_left().x > 0);
        assert!(view.content_viewport().top_left().y > 0);
        let editor = view.get_inner().read();
        assert!(editor.canvas.x >= 33);
        assert!(editor.canvas.y >= 16);
    }

    #[test]
    fn move_tool_selects_then_moves_content_on_mouse_release() {
        let mut view = tool_view();
        view.get_inner_mut().write().buffer = buffer("ab\n");
        let mut tool = MoveTool::default();

        {
            let mut ctx = EditorCtx::new(&mut view);
            // Select the label, then grab it and drag it down and right.
            tool.on_event(&mut ctx, &left_mouse(Press(Left), point(0, 0)));
            tool.on_event(&mut ctx, &left_mouse(Hold(Left), point(1, 0)));
            tool.on_event(&mut ctx, &left_mouse(Release(Left), point(1, 0)));
            tool.on_event(&mut ctx, &left_mouse(Press(Left), point(0, 0)));
            tool.on_event(&mut ctx, &left_mouse(Hold(Left), point(2, 1)));
            tool.on_event(&mut ctx, &left_mouse(Release(Left), point(2, 1)));
        }

        assert_eq!(text(&view.get_inner().read().buffer), "  \n  ab\n");
    }

    #[test]
    fn undo_redo_and_clear_track_saved_state() {
        let mut ed = editor();
        assert!(!ed.is_dirty());
        assert!(!ed.undo());
        ed.with_snapshot(|ed| {
            ed.buffer.set(true, 0, 0, 'a');
            ed.buffer.flush_edits();
        });
        assert!(ed.is_dirty());
        assert_eq!(text(&ed.buffer), "a\n");
        assert!(ed.undo());
        assert!(!ed.is_dirty());
        assert_eq!(text(&ed.buffer), "");
        assert!(ed.redo());
        assert!(ed.is_dirty());
        assert_eq!(text(&ed.buffer), "a\n");
        ed.clear();
        assert!(!ed.is_dirty());
        assert!(!ed.redo());
        assert_eq!(ed.path(), None);
    }

    #[test]
    fn saving_trims_trailing_spaces_and_opening_restores_saved_content() {
        let path = TempFile::new();
        let mut ed = editor();
        assert!(!ed.save().unwrap());
        ed.with_snapshot(|ed| ed.buffer = buffer("a  \n b \n"));
        ed.save_as(&path.0).unwrap();
        assert_eq!(fs::read_to_string(&path.0).unwrap(), "a\n b\n");
        assert!(!ed.is_dirty());
        ed.with_snapshot(|ed| ed.buffer = buffer("changed\n"));
        ed.open_file(&path.0).unwrap();
        assert_eq!(text(&ed.buffer), "a\n b\n");
        assert!(!ed.is_dirty());
        assert_eq!(ed.path(), Some(&path.0));
    }

    #[test]
    fn save_options_control_whitespace_cleanup() {
        let kept = TempFile::new();
        let stripped = TempFile::new();
        let mut ed = editor();
        ed.buffer = buffer("  a  \n  b \n");
        ed.opts.keep_trailing_ws = true;
        ed.save_as(&kept.0).unwrap();
        assert_eq!(fs::read_to_string(&kept.0).unwrap(), "  a  \n  b \n");
        ed.opts.strip_margin_ws = true;
        ed.save_as(&stripped.0).unwrap();
        assert_eq!(fs::read_to_string(&stripped.0).unwrap(), "a\nb\n");
    }
}
