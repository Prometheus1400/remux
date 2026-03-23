use crate::{
    layout::Rect,
    render::{
        bar::{BarRenderState, BarRenderer, BarSpec},
        overlay::{
            FuzzySelectorOverlay, SelectorOverlay, render_fuzzy_selector_overlay, render_scrim, render_selector_overlay,
        },
        surface::Surface,
    },
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum DockEdge {
    Top,
    Bottom,
    Left,
    Right,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DockWidgetKind {
    Bar(BarSpec),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DockedWidgetSpec {
    pub id: String,
    pub edge: DockEdge,
    pub size: u16,
    pub kind: DockWidgetKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DockedWidgetLayout {
    pub id: String,
    pub edge: DockEdge,
    pub rect: Rect,
    pub kind: DockWidgetKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OverlayWidget {
    Selector(SelectorOverlay),
    FuzzySelector(FuzzySelectorOverlay),
}

pub fn compute_docked_layout(root: Rect, widgets: &[DockedWidgetSpec]) -> (Rect, Vec<DockedWidgetLayout>) {
    let mut content = root;
    let mut layouts = Vec::with_capacity(widgets.len());

    for widget in widgets {
        let rect = match widget.edge {
            DockEdge::Top => {
                let height = widget.size.min(content.height);
                let rect = Rect {
                    x: content.x,
                    y: content.y,
                    width: content.width,
                    height,
                };
                content.y = content.y.saturating_add(height);
                content.height = content.height.saturating_sub(height);
                rect
            }
            DockEdge::Bottom => {
                let height = widget.size.min(content.height);
                let rect = Rect {
                    x: content.x,
                    y: content.y + content.height.saturating_sub(height),
                    width: content.width,
                    height,
                };
                content.height = content.height.saturating_sub(height);
                rect
            }
            DockEdge::Left => {
                let width = widget.size.min(content.width);
                let rect = Rect {
                    x: content.x,
                    y: content.y,
                    width,
                    height: content.height,
                };
                content.x = content.x.saturating_add(width);
                content.width = content.width.saturating_sub(width);
                rect
            }
            DockEdge::Right => {
                let width = widget.size.min(content.width);
                let rect = Rect {
                    x: content.x + content.width.saturating_sub(width),
                    y: content.y,
                    width,
                    height: content.height,
                };
                content.width = content.width.saturating_sub(width);
                rect
            }
        };

        layouts.push(DockedWidgetLayout {
            id: widget.id.clone(),
            edge: widget.edge,
            rect,
            kind: widget.kind.clone(),
        });
    }

    (content, layouts)
}

pub fn render_docked_widget(
    width: u16,
    height: u16,
    widget: &DockWidgetKind,
    edge: DockEdge,
    state: &BarRenderState,
) -> Surface {
    match widget {
        DockWidgetKind::Bar(spec) => match edge {
            DockEdge::Top | DockEdge::Bottom => render_horizontal_bar(width, height, spec.clone(), state),
            DockEdge::Left | DockEdge::Right => render_vertical_bar(width, height, spec.clone(), state),
        },
    }
}

pub fn render_overlay_widget(width: u16, height: u16, overlay: &OverlayWidget) -> Surface {
    let (mut surface, popup) = match overlay {
        OverlayWidget::Selector(overlay) => (
            render_scrim(width, height, overlay.style.scrim_fg, overlay.style.scrim_bg),
            render_selector_overlay(width, height, overlay),
        ),
        OverlayWidget::FuzzySelector(overlay) => (
            render_scrim(width, height, overlay.style.scrim_fg, overlay.style.scrim_bg),
            render_fuzzy_selector_overlay(width, height, overlay),
        ),
    };
    surface.overlay_transparent_at(&popup, 0, 0);
    surface
}

fn render_horizontal_bar(width: u16, height: u16, spec: BarSpec, state: &BarRenderState) -> Surface {
    let mut surface = Surface::new(width, height);
    for y in 0..height {
        let row = BarRenderer::from_spec(spec.clone()).render(width, state);
        surface.overlay_at(&row, 0, y);
    }
    surface
}

fn render_vertical_bar(width: u16, height: u16, spec: BarSpec, state: &BarRenderState) -> Surface {
    let mut surface = Surface::new(width, height);
    if width == 0 || height == 0 {
        return surface;
    }

    let flattened = BarRenderer::from_spec(spec).render(width.saturating_mul(height), state);
    let mut src = 0u16;
    for x in 0..width {
        for y in 0..height {
            if let Some(cell) = flattened.cells().get(src as usize) {
                surface.paint_cell(x, y, cell.clone());
            }
            src = src.saturating_add(1);
        }
    }
    surface
}
