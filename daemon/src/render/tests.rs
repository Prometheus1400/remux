use crate::render::{
    bar::{BarRenderer, BarSpec, BarStyle},
    diff::render_surface_diff,
    overlay::{SessionSwitcherOverlay, SessionSwitcherStyle, render_session_switcher_overlay},
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
    curr.set_cursor(Some((1, 0)), true);

    let output = render_surface_diff(&prev, &curr, false);

    assert!(output.windows(4).any(|window| window == b"\x1b[1;"));
    assert!(output.windows(6).any(|window| window == b"\x1b[?25h"));
    assert!(output.contains(&b'z'));
}

#[test]
fn final_surface_diff_clears_removed_overlay_content() {
    let mut prev = Surface::new(3, 1);
    let curr = Surface::new(3, 1);

    prev.paint_byte(2, 0, b'x');

    let output = render_surface_diff(&prev, &curr, false);

    assert!(
        output.windows(4).any(|window| window == b"\x1b[0m"),
        "output was {:?}",
        output
    );
    assert!(
        output.windows(3).any(|window| window == b"[1X"),
        "output was {:?}",
        output
    );
}

#[test]
fn forced_surface_diff_clears_screen_before_redraw() {
    let prev = Surface::new(80, 24);
    let curr = Surface::new(170, 51);

    let output = render_surface_diff(&prev, &curr, true);

    assert!(output.starts_with(b"\x1b[H\x1b[2J"), "output was {:?}", output);
}

#[test]
fn final_surface_diff_hides_cursor_when_surface_cursor_is_not_visible() {
    let prev = Surface::new(2, 1);
    let curr = Surface::new(2, 1);

    let output = render_surface_diff(&prev, &curr, false);

    assert!(
        output.windows(6).any(|window| window == b"\x1b[?25l"),
        "output was {:?}",
        output
    );
}

#[test]
fn bar_renderer_places_sections_across_the_row() {
    let renderer = BarRenderer::from_spec(BarSpec {
        enabled: true,
        left: vec!["left".into()],
        center: vec!["middle".into()],
        right: vec!["right".into()],
        style: BarStyle::default(),
    });

    let surface = renderer.render(30);
    let rendered: String = (0..30).map(|x| surface.byte_at(x, 0).unwrap_or(b' ') as char).collect();

    assert!(rendered.starts_with("left"));
    assert!(rendered.contains("middle"));
    assert!(rendered.trim_end().ends_with("right"));
}

#[test]
fn disabled_bar_renders_nothing() {
    let renderer = BarRenderer::from_spec(BarSpec {
        enabled: false,
        left: vec!["active-session".into()],
        center: Vec::new(),
        right: Vec::new(),
        style: BarStyle::default(),
    });

    assert!(
        renderer
            .render(20)
            .cells()
            .iter()
            .all(|cell: &crate::cell::RemuxCell| cell.content_bytes() == b" ")
    );
}

#[test]
fn session_switcher_overlay_renders_selected_session() {
    let overlay = SessionSwitcherOverlay {
        sessions: vec!["alpha".into(), "beta".into(), "gamma".into()],
        selected: 1,
    };

    let surface = render_session_switcher_overlay(40, 12, &overlay, &SessionSwitcherStyle::default());
    let rendered: String = surface
        .cells()
        .iter()
        .map(|cell: &crate::cell::RemuxCell| cell.content_bytes().first().copied().unwrap_or(b' ') as char)
        .collect();

    assert!(rendered.contains("Sessions"));
    assert!(rendered.contains("> beta"));
}

#[test]
fn overlay_out_of_bounds_cells_are_clipped() {
    let mut base = Surface::new(2, 2);
    let mut overlay = Surface::new(2, 2);

    overlay.paint_byte(0, 0, b'x');
    overlay.paint_byte(1, 1, b'y');
    base.overlay_at(&overlay, 1, 1);

    assert_eq!(base.byte_at(1, 1), Some(b'x'));
    assert_eq!(base.byte_at(0, 0), Some(b' '));
}

#[test]
fn overlay_propagates_cursor_position_and_visibility() {
    let mut base = Surface::new(4, 2);
    let mut overlay = Surface::new(2, 1);

    overlay.set_cursor(Some((1, 0)), true);
    base.overlay_at(&overlay, 2, 1);

    assert_eq!(base.cursor(), Some((3, 1)));
    assert!(base.cursor_visible());
}

#[test]
fn transparent_overlay_preserves_background_outside_painted_cells() {
    let mut base = Surface::new(12, 4);
    base.paint_byte(0, 0, b'a');
    base.paint_byte(11, 3, b'z');

    let overlay = render_session_switcher_overlay(
        12,
        4,
        &SessionSwitcherOverlay {
            sessions: vec!["alpha".into()],
            selected: 0,
        },
        &SessionSwitcherStyle::default(),
    );

    base.overlay_transparent_at(&overlay, 0, 0);

    assert_eq!(base.byte_at(0, 0), Some(b'a'));
    assert_eq!(base.byte_at(11, 3), Some(b'z'));
}

#[test]
fn bar_truncates_text_that_overflows_surface_width() {
    let renderer = BarRenderer::from_spec(BarSpec {
        enabled: true,
        left: vec!["abcdef".into()],
        center: Vec::new(),
        right: vec!["uvwxyz".into()],
        style: BarStyle::default(),
    });

    let surface = renderer.render(5);
    let rendered: String = (0..5).map(|x| surface.byte_at(x, 0).unwrap_or(b' ') as char).collect();

    assert_eq!(rendered, "...  ");
}

#[test]
fn bar_prioritizes_left_section_when_space_is_tight() {
    let renderer = BarRenderer::from_spec(BarSpec {
        enabled: true,
        left: vec!["left".into()],
        center: vec!["center-center".into()],
        right: vec!["right-right".into()],
        style: BarStyle::default(),
    });

    let surface = renderer.render(14);
    let rendered: String = (0..14).map(|x| surface.byte_at(x, 0).unwrap_or(b' ') as char).collect();

    assert!(rendered.starts_with("left"));
}

#[test]
fn bar_hides_center_section_when_it_would_float_awkwardly() {
    let renderer = BarRenderer::from_spec(BarSpec {
        enabled: true,
        left: vec!["left".into()],
        center: vec!["12:34:56".into()],
        right: vec!["right".into()],
        style: BarStyle::default(),
    });

    let surface = renderer.render(18);
    let rendered: String = (0..18).map(|x| surface.byte_at(x, 0).unwrap_or(b' ') as char).collect();

    assert!(rendered.starts_with("left"));
    assert!(rendered.trim_end().ends_with("right"));
    assert!(!rendered.contains("12:34:56"));
}

#[test]
fn session_switcher_overlay_handles_empty_session_list() {
    let overlay = SessionSwitcherOverlay::default();

    let surface = render_session_switcher_overlay(20, 6, &overlay, &SessionSwitcherStyle::default());
    let rendered: String = surface
        .cells()
        .iter()
        .map(|cell: &crate::cell::RemuxCell| cell.content_bytes().first().copied().unwrap_or(b' ') as char)
        .collect();

    assert!(rendered.contains("Sessions"));
    assert!(!rendered.contains("> "));
}

#[test]
fn session_switcher_overlay_truncates_long_session_names() {
    let overlay = SessionSwitcherOverlay {
        sessions: vec!["this-session-name-is-far-too-long".into()],
        selected: 0,
    };

    let surface = render_session_switcher_overlay(24, 10, &overlay, &SessionSwitcherStyle::default());
    let rendered: String = surface
        .cells()
        .iter()
        .map(|cell: &crate::cell::RemuxCell| cell.content_bytes().first().copied().unwrap_or(b' ') as char)
        .collect();

    assert!(rendered.contains("this-s"));
    assert!(rendered.contains("..."));
}
