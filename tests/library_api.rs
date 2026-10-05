use skizze::{Buffer, Document, PathMode, Point, TextEdit};

#[test]
fn library_buffer_draws_and_renders_without_an_application_runtime() {
    let mut buffer = Buffer::default();

    buffer.draw_line(Point::new(0, 0), Point::new(3, 0));
    buffer.commit();

    assert_eq!(buffer.render(""), "+--+\n");
}

#[test]
fn library_buffer_preserves_input_lines_and_applies_a_prefix_per_line() {
    let buffer = Buffer::from_text("  a\n\n b  \n");

    assert_eq!(buffer.render(""), "  a\n\n b  \n");
    assert_eq!(buffer.render("# "), "#   a\n# \n#  b  \n");
}

#[test]
fn library_buffer_can_trim_trailing_or_all_margin_whitespace() {
    let source = "   \n  a  \n    b \n \n";

    let mut trailing = Buffer::from_text(source);
    trailing.strip_trailing_whitespace();
    assert_eq!(trailing.render(""), "\n  a\n    b\n\n");

    let mut margins = Buffer::from_text(source);
    margins.strip_margin_whitespace();
    assert_eq!(margins.render(""), "a\n  b\n");
}

#[test]
fn library_buffer_stages_edits_until_commit_or_discard() {
    let mut buffer = Buffer::from_text("a\n");
    let position = Point::new(2, 1);

    buffer.set_at_forced(position, 'z');
    assert_eq!(buffer.get(position), None);
    assert_eq!(buffer.bounds(), Point::new(3, 2));
    assert_eq!(buffer.pending_cells().collect::<Vec<_>>(), vec![(position, 'z')]);

    buffer.discard_edits();
    assert_eq!(buffer.bounds(), Point::new(1, 1));

    buffer.set_at_forced(position, 'z');
    buffer.commit();
    assert_eq!(buffer.get(position), Some('z'));
    assert_eq!(buffer.render(""), "a\n  z\n");
}

#[test]
fn library_buffer_uses_stroke_precedence_and_visibility_of_committed_content() {
    let mut buffer = Buffer::from_text("-\n");

    buffer.set_at(Point::new(0, 0), '|');
    buffer.set_at(Point::new(1, 0), '|');
    buffer.set_at(Point::new(1, 0), '+');
    assert!(buffer.visible(Point::new(0, 0)));
    assert!(!buffer.visible(Point::new(1, 0)));
    buffer.commit();

    assert_eq!(buffer.render(""), "-+\n");
    buffer.set_at_forced(Point::new(0, 0), 'x');
    buffer.commit();
    assert_eq!(buffer.render(""), "x+\n");
}

#[test]
fn library_buffer_arrow_tips_cover_all_directions() {
    let cases = [
        (Point::new(1, 1), Point::new(1, 0), '^'),
        (Point::new(1, 1), Point::new(2, 1), '>'),
        (Point::new(1, 1), Point::new(1, 2), 'v'),
        (Point::new(1, 1), Point::new(0, 1), '<'),
        (Point::new(1, 1), Point::new(2, 0), '^'),
        (Point::new(1, 1), Point::new(0, 2), 'v'),
    ];
    for (src, dst, expected) in cases {
        let mut buffer = Buffer::default();
        buffer.draw_arrow_tip(src, dst);
        buffer.commit();
        assert_eq!(buffer.get(dst), Some(expected), "{src:?} -> {dst:?}");
    }
}

#[test]
fn library_arrow_tip_follows_adjacent_stroke_matrix() {
    let cases = [
        (Point::new(3, 5), Point::new(3, 3), vec![Point::new(2, 3)], '<'),
        (Point::new(3, 5), Point::new(3, 3), vec![Point::new(4, 3)], '>'),
        (Point::new(1, 3), Point::new(3, 3), vec![Point::new(3, 2)], '^'),
        (Point::new(3, 1), Point::new(3, 3), vec![Point::new(2, 3)], '<'),
        (Point::new(5, 3), Point::new(3, 3), vec![Point::new(3, 4)], 'v'),
        (Point::new(1, 1), Point::new(3, 3), vec![Point::new(4, 3)], '>'),
        (Point::new(5, 5), Point::new(3, 3), vec![Point::new(2, 3)], '<'),
    ];
    for (src, dst, occupied, expected) in cases {
        let mut buffer = Buffer::default();
        for point in occupied { buffer.set_at_forced(point, 'x'); }
        buffer.commit();
        buffer.draw_arrow_tip(src, dst);
        buffer.commit();
        assert_eq!(buffer.get(dst), Some(expected), "{src:?} -> {dst:?}");
    }
}

#[test]
fn library_arrow_tip_retains_direction_when_forward_cell_is_occupied() {
    let mut buffer = Buffer::from_text("   \n - \n   \n");
    buffer.draw_arrow_tip(Point::new(1, 2), Point::new(1, 1));
    buffer.commit();
    assert_eq!(buffer.get(Point::new(1, 1)), Some('^'));
}

#[test]
fn library_arrow_tip_preserves_connected_horizontal_direction() {
    let mut buffer = Buffer::from_text(" - \n");
    buffer.draw_arrow_tip(Point::new(0, 0), Point::new(1, 0));
    buffer.commit();
    assert_eq!(buffer.get(Point::new(1, 0)), Some('>'));
}

#[test]
fn library_buffer_snaps_connectors_and_draws_arrow_tips() {
    let buffer = Buffer::from_text("    -\n");
    assert_eq!(buffer.snap90(Point::new(1, 1), Point::new(4, 0)), Point::new(4, 1));
    assert_eq!(buffer.snap90(Point::new(1, 1), Point::new(3, 0)), Point::new(1, 0));
    assert_eq!(buffer.snap45(Point::new(1, 1), Point::new(4, 3)), Point::new(2, 1));

    let mut arrow = Buffer::default();
    arrow.draw_arrow_tip(Point::new(1, 1), Point::new(1, 0));
    arrow.commit();
    assert_eq!(arrow.get(Point::new(1, 0)), Some('^'));
}

#[test]
fn document_tracks_save_state_and_history() {
    let mut document = Document::from_text("a\n");
    assert!(!document.is_dirty());
    assert!(!document.undo());

    document.edit(|buffer| buffer.set_at_forced(Point::new(1, 0), 'b'));
    assert!(document.is_dirty());
    assert_eq!(document.buffer().render(""), "ab\n");

    assert!(document.undo());
    assert_eq!(document.buffer().render(""), "a\n");
    assert!(!document.is_dirty());
    assert!(document.redo());
    assert_eq!(document.buffer().render(""), "ab\n");

    document.mark_saved();
    assert!(!document.is_dirty());

    document.replace_buffer(Buffer::from_text("z\n"));
    assert_eq!(document.buffer().render(""), "z\n");
    assert!(document.undo());
    assert_eq!(document.buffer().render(""), "ab\n");
}

#[test]
fn library_buffer_respects_stroke_overlap_precedence() {
    let mut buffer = Buffer::from_text("-\n");
    buffer.set_at(Point::new(0, 0), '|');
    buffer.set_at(Point::new(1, 0), '|');
    buffer.set_at(Point::new(1, 0), '+');
    buffer.commit();
    assert_eq!(buffer.render(""), "-+\n");
    buffer.set_at_forced(Point::new(0, 0), 'x');
    buffer.commit();
    assert_eq!(buffer.render(""), "x+\n");
}

#[test]
fn text_edit_handles_insertion_navigation_and_line_merging() {
    let mut text = TextEdit::default();
    text.begin();
    text.insert('a');
    text.insert('b');
    text.left();
    text.newline();
    text.insert('x');
    assert_eq!(text.lines(), &[vec!['a'], vec!['x', 'b']]);
    assert_eq!(text.cursor(), Point::new(1, 1));

    text.backspace();
    text.backspace();
    assert_eq!(text.lines(), &[vec!['a', 'b']]);
    assert_eq!(text.cursor(), Point::new(1, 0));
}

#[test]
fn library_buffer_places_multiline_text() {
    let mut buffer = Buffer::default();
    buffer.place_text(Point::new(2, 1), &[vec!['h', 'i'], vec![' ', '!']]);
    buffer.commit();
    assert_eq!(buffer.render(""), "\n  hi\n   !\n");
}

#[test]
fn library_buffer_moves_non_whitespace_cells() {
    let mut buffer = Buffer::from_text("ab\n");
    buffer.move_box(Point::new(0, 0), Point::new(1, 0), (1, 1));
    buffer.commit();
    assert_eq!(buffer.render(""), "  \n ab\n");
}

#[test]
fn library_buffer_erases_only_visible_cells_in_a_box() {
    let mut buffer = Buffer::from_text("ab\n c\n");
    buffer.erase_box(Point::new(0, 0), Point::new(0, 1));
    buffer.commit();
    assert_eq!(buffer.render(""), " b\n c\n");
}

#[test]
fn library_buffer_preserves_reverse_diagonal_lines() {
    let cases = [
        (Point::new(2, 0), Point::new(0, 2), '/'),
        (Point::new(2, 2), Point::new(0, 0), '\\'),
    ];
    for (start, end, middle) in cases {
        let mut buffer = Buffer::default();
        buffer.draw_line(start, end);
        buffer.commit();
        assert_eq!(buffer.get(Point::new(1, 1)), Some(middle));
    }
}

#[test]
fn library_buffer_draws_boxes() {
    let mut buffer = Buffer::default();
    buffer.draw_box(Point::new(0, 0), Point::new(2, 1));
    buffer.commit();
    assert_eq!(buffer.render(""), "+-+\n+-+\n");
}

#[test]
fn library_buffer_draws_connectors_by_path_mode() {
    let mut line = Buffer::default();
    line.draw_connector(Point::new(0, 0), Point::new(2, 1), PathMode::Snap90, false);
    line.commit();
    assert_eq!(line.render(""), "+\n+-+\n");

    let mut arrow = Buffer::default();
    arrow.draw_connector(Point::new(0, 0), Point::new(2, 0), PathMode::Snap45, true);
    arrow.commit();
    assert_eq!(arrow.render(""), "+->\n");
}

#[test]
fn library_buffer_routes_around_component_content() {
    let mut buffer = Buffer::from_text("  +-----+\n  | API |\n  +-----+\n");
    let original = buffer.clone();
    let last = buffer.draw_path(Point::new(0, 1), Point::new(10, 1));
    buffer.commit();

    assert!(last.y > 1);
    for y in 0..3 {
        for x in 2..9 {
            assert_eq!(buffer.get(Point::new(x, y)), original.get(Point::new(x, y)));
        }
    }
}

#[test]
fn library_buffer_routes_around_existing_content() {
    let mut buffer = Buffer::from_text(" + \n");
    let last = buffer.draw_path(Point::new(0, 0), Point::new(2, 0));
    buffer.commit();

    assert_eq!(last, Point::new(1, 1));
    assert_eq!(buffer.get(Point::new(1, 0)), Some('+'));
    assert_eq!(buffer.get(Point::new(0, 0)), Some('+'));
    assert_eq!(buffer.get(Point::new(2, 0)), Some('+'));
}
