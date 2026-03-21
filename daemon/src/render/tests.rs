use crate::render::{
    diff::render_surface_diff,
    overlay::{render_session_switcher_overlay, SessionSwitcherOverlay},
    status_line::{StatusLineRenderer, StatusLineTemplate},
    surface::Surface,
};

#[test]
fn surface_can_paint_rectangular_regions_in_absolute_coordinates() {
    let mut surface = Surface::new(4, 2);

    surface.paint_byte(1, 1, b'x');

    assert_eq!(surface.byte_at(1, 1), Some(b'x'));
}

#[test]
fn later_layers_override_earlier_layers() {
    let mut base = Surface::new(2, 1);
    let mut overlay = Surface::new(2, 1);

    base.paint_byte(0, 0, b'a');
    overlay.paint_byte(0, 0, b'b');
    base.overlay_at(&overlay, 0, 0);

    assert_eq!(base.byte_at(0, 0), Some(b'b'));
}

#[test]
fn surface_tracks_cursor_position_and_visibility() {
    let mut surface = Surface::new(2, 2);

    surface.set_cursor(Some((1, 1)), true);

    assert_eq!(surface.cursor(), Some((1, 1)));
    assert!(surface.cursor_visible());
}

#[test]
fn final_surface_diff_emits_cursor_moves_and_cell_content() {
    let prev = Surface::new(2, 1);
    let mut curr = Surface::new(2, 1);

    curr.paint_byte(1, 0, b'z');

    let output = render_surface_diff(&prev, &curr, false);

    assert!(output.windows(4).any(|window| window == b"\x1b[1;"));
    assert!(output.contains(&b'z'));
}

#[test]
fn final_surface_diff_clears_removed_overlay_content() {
    let mut prev = Surface::new(3, 1);
    let curr = Surface::new(3, 1);

    prev.paint_byte(2, 0, b'x');

    let output = render_surface_diff(&prev, &curr, false);

    assert!(output.ends_with(b"\x1b[0m"));
    assert!(
        output.windows(3).any(|window| window == b"[1X"),
        "output was {:?}",
        output
    );
}

#[test]
fn status_line_renderer_places_sections_across_the_row() {
    let renderer = StatusLineRenderer::from_template(StatusLineTemplate {
        enabled: true,
        a: vec!["left".into()],
        b: vec!["middle".into()],
        c: vec!["right".into()],
    });

    let surface = renderer.render(30, Some("demo-session"));
    let rendered: String = (0..30).map(|x| surface.byte_at(x, 0).unwrap_or(b' ') as char).collect();

    assert!(rendered.starts_with("left"));
    assert!(rendered.contains("middle"));
    assert!(rendered.trim_end().ends_with("right"));
}

#[test]
fn disabled_status_line_renders_nothing() {
    let renderer = StatusLineRenderer::from_template(StatusLineTemplate {
        enabled: false,
        a: vec!["active-session".into()],
        b: Vec::new(),
        c: Vec::new(),
    });

    assert!(renderer
        .render(20, Some("demo-session"))
        .cells()
        .iter()
        .all(|cell: &crate::cell::RemuxCell| cell.content_bytes() == b" "));
}

#[test]
fn session_switcher_overlay_renders_selected_session() {
    let overlay = SessionSwitcherOverlay {
        sessions: vec!["alpha".into(), "beta".into(), "gamma".into()],
        selected: 1,
    };

    let surface = render_session_switcher_overlay(40, 12, &overlay);
    let rendered: String = surface
        .cells()
        .iter()
        .map(|cell: &crate::cell::RemuxCell| cell.content_bytes().first().copied().unwrap_or(b' ') as char)
        .collect();

    assert!(rendered.contains("Sessions"));
    assert!(rendered.contains("> beta"));
}
