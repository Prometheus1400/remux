use crate::render::{
    bar::{BarItem, BarRenderState, BarRenderer, BarSpec, BarStyle, WindowTab},
    diff::render_surface_diff,
    overlay::{
        FuzzySelectorOverlay, FuzzySelectorStyle, SelectorItem, SelectorOverlay, SelectorStyle,
        render_fuzzy_selector_overlay, render_selector_overlay,
    },
    surface::Surface,
    widget::{OverlayWidget, render_overlay_widget},
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
        left: vec![BarItem::Text("left".into())],
        center: vec![BarItem::Text("middle".into())],
        right: vec![BarItem::Text("right".into())],
        style: BarStyle::default(),
    });

    let surface = renderer.render(30, &BarRenderState::default());
    let rendered: String = (0..30).map(|x| surface.byte_at(x, 0).unwrap_or(b' ') as char).collect();

    assert!(rendered.starts_with(" left "));
    assert_eq!(rendered.find(" middle "), Some(11));
    assert!(rendered.trim_end().ends_with(" right"));
}

#[test]
fn disabled_bar_renders_nothing() {
    let renderer = BarRenderer::from_spec(BarSpec {
        enabled: false,
        left: vec![BarItem::ActiveSession],
        center: Vec::new(),
        right: Vec::new(),
        style: BarStyle::default(),
    });

    assert!(
        renderer
            .render(20, &BarRenderState::default())
            .cells()
            .iter()
            .all(|cell: &crate::cell::RemuxCell| cell.content_bytes() == b" ")
    );
}

#[test]
fn session_switcher_overlay_renders_selected_session() {
    let overlay = SelectorOverlay {
        title: "Sessions".into(),
        footer: String::new(),
        items: vec![
            SelectorItem {
                id: "alpha".into(),
                label: "alpha".into(),
            },
            SelectorItem {
                id: "beta".into(),
                label: "beta".into(),
            },
            SelectorItem {
                id: "gamma".into(),
                label: "gamma".into(),
            },
        ],
        selected: 1,
        style: SelectorStyle::default(),
    };

    let surface = render_selector_overlay(40, 12, &overlay);
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

    let overlay = render_selector_overlay(
        12,
        4,
        &SelectorOverlay {
            title: "Sessions".into(),
            footer: String::new(),
            items: vec![SelectorItem {
                id: "alpha".into(),
                label: "alpha".into(),
            }],
            selected: 0,
            style: SelectorStyle::default(),
        },
    );

    base.overlay_transparent_at(&overlay, 0, 0);

    assert_eq!(base.byte_at(0, 0), Some(b'a'));
    assert_eq!(base.byte_at(11, 3), Some(b'z'));
}

#[test]
fn bar_truncates_text_that_overflows_surface_width() {
    let renderer = BarRenderer::from_spec(BarSpec {
        enabled: true,
        left: vec![BarItem::Text("abcdef".into())],
        center: Vec::new(),
        right: vec![BarItem::Text("uvwxyz".into())],
        style: BarStyle::default(),
    });

    let surface = renderer.render(5, &BarRenderState::default());
    let rendered: String = (0..5).map(|x| surface.byte_at(x, 0).unwrap_or(b' ') as char).collect();

    assert_eq!(rendered, " a...");
}

#[test]
fn bar_prioritizes_left_section_when_space_is_tight() {
    let renderer = BarRenderer::from_spec(BarSpec {
        enabled: true,
        left: vec![BarItem::Text("left".into())],
        center: vec![BarItem::Text("center-center".into())],
        right: vec![BarItem::Text("right-right".into())],
        style: BarStyle::default(),
    });

    let surface = renderer.render(14, &BarRenderState::default());
    let rendered: String = (0..14).map(|x| surface.byte_at(x, 0).unwrap_or(b' ') as char).collect();

    assert_eq!(rendered, " center-cen...");
}

#[test]
fn bar_keeps_center_absolutely_centered_when_space_is_tight() {
    let renderer = BarRenderer::from_spec(BarSpec {
        enabled: true,
        left: vec![BarItem::Text("left".into())],
        center: vec![BarItem::Text("12:34:56".into())],
        right: vec![BarItem::Text("right".into())],
        style: BarStyle::default(),
    });

    let surface = renderer.render(18, &BarRenderState::default());
    let rendered: String = (0..18).map(|x| surface.byte_at(x, 0).unwrap_or(b' ') as char).collect();

    assert_eq!(rendered.find(" 12:34:56 "), Some(4));
    assert_eq!(&rendered[0..4], " ...");
    assert_eq!(&rendered[14..18], "... ");
}

#[test]
fn bar_center_is_absolutely_centered_even_with_asymmetric_sides() {
    let renderer = BarRenderer::from_spec(BarSpec {
        enabled: true,
        left: vec![BarItem::Text("very-wide-left".into())],
        center: vec![BarItem::Text("mid".into())],
        right: vec![BarItem::Text("r".into())],
        style: BarStyle::default(),
    });

    let surface = renderer.render(31, &BarRenderState::default());
    let rendered: String = (0..31).map(|x| surface.byte_at(x, 0).unwrap_or(b' ') as char).collect();

    assert_eq!(rendered.find(" mid "), Some(13));
    assert!(rendered.starts_with(" very-wide"));
    assert!(rendered.trim_end().ends_with(" r"));
}

#[test]
fn bar_clips_side_sections_before_moving_absolute_center() {
    let renderer = BarRenderer::from_spec(BarSpec {
        enabled: true,
        left: vec![BarItem::Text("left-left-left".into())],
        center: vec![BarItem::Text("mid".into())],
        right: vec![BarItem::Text("right-right-right".into())],
        style: BarStyle::default(),
    });

    let surface = renderer.render(21, &BarRenderState::default());
    let rendered: String = (0..21).map(|x| surface.byte_at(x, 0).unwrap_or(b' ') as char).collect();

    assert_eq!(rendered.find(" mid "), Some(8));
    assert_eq!(&rendered[0..8], " left...");
    assert_eq!(&rendered[13..21], "...ight ");
}

#[test]
fn window_list_component_renders_window_tabs() {
    let renderer = BarRenderer::from_spec(BarSpec {
        enabled: true,
        left: vec![BarItem::ActiveSession],
        center: vec![BarItem::WindowList],
        right: Vec::new(),
        style: BarStyle::default(),
    });

    let surface = renderer.render(
        40,
        &BarRenderState {
            active_session_name: Some("dev".into()),
            windows: vec![
                WindowTab {
                    index: 1,
                    name: "shell".into(),
                    is_active: true,
                },
                WindowTab {
                    index: 2,
                    name: "logs".into(),
                    is_active: false,
                },
            ],
        },
    );
    let rendered: String = (0..40).map(|x| surface.byte_at(x, 0).unwrap_or(b' ') as char).collect();

    assert!(rendered.contains("1 shell"));
    assert!(rendered.contains("2 logs"));
}

#[test]
fn session_switcher_overlay_handles_empty_session_list() {
    let overlay = SelectorOverlay {
        title: String::new(),
        footer: String::new(),
        items: Vec::new(),
        selected: 0,
        style: SelectorStyle::default(),
    };

    let surface = render_selector_overlay(20, 6, &overlay);
    let rendered: String = surface
        .cells()
        .iter()
        .map(|cell: &crate::cell::RemuxCell| cell.content_bytes().first().copied().unwrap_or(b' ') as char)
        .collect();

    assert!(rendered.contains("Sessions"));
    assert!(rendered.contains("No items"));
    assert!(!rendered.contains("> "));
}

#[test]
fn session_switcher_overlay_truncates_long_session_names() {
    let overlay = SelectorOverlay {
        title: "Sessions".into(),
        footer: String::new(),
        items: vec![SelectorItem {
            id: "this-session-name-is-far-too-long".into(),
            label: "this-session-name-is-far-too-long".into(),
        }],
        selected: 0,
        style: SelectorStyle::default(),
    };

    let surface = render_selector_overlay(24, 10, &overlay);
    let rendered: String = surface
        .cells()
        .iter()
        .map(|cell: &crate::cell::RemuxCell| cell.content_bytes().first().copied().unwrap_or(b' ') as char)
        .collect();

    assert!(rendered.contains("this-s"));
    assert!(rendered.contains("..."));
}

#[test]
fn fuzzy_selector_overlay_renders_query_and_placeholder() {
    let overlay = FuzzySelectorOverlay {
        title: "Search".into(),
        footer: String::new(),
        placeholder: "type a name".into(),
        query: String::new(),
        items: vec![SelectorItem {
            id: "alpha".into(),
            label: "alpha".into(),
        }],
        selected: 0,
        style: FuzzySelectorStyle::default(),
    };

    let surface = render_fuzzy_selector_overlay(40, 12, &overlay);
    let rendered: String = surface
        .cells()
        .iter()
        .map(|cell: &crate::cell::RemuxCell| cell.content_bytes().first().copied().unwrap_or(b' ') as char)
        .collect();

    assert!(rendered.contains("Search"));
    assert!(rendered.contains("type a name"));
    assert!(rendered.contains("> alpha"));
}

#[test]
fn render_widget_overlay_dispatches_fuzzy_selector_variant() {
    let overlay = OverlayWidget::FuzzySelector(FuzzySelectorOverlay {
        title: "Search".into(),
        footer: String::new(),
        placeholder: "type".into(),
        query: "be".into(),
        items: vec![SelectorItem {
            id: "beta".into(),
            label: "beta".into(),
        }],
        selected: 0,
        style: FuzzySelectorStyle::default(),
    });

    let surface = render_overlay_widget(40, 12, &overlay);
    let rendered: String = surface
        .cells()
        .iter()
        .map(|cell: &crate::cell::RemuxCell| cell.content_bytes().first().copied().unwrap_or(b' ') as char)
        .collect();

    assert!(rendered.contains("/ be"));
    assert!(rendered.contains("> beta"));
}
