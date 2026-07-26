//! Shared mouse plumbing: widget-local coordinates, wheel-to-motion
//! mapping, and the modal gate that keeps clicks from acting through
//! an open overlay.

use bevy::prelude::*;
use bevy_ratatui::crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use plurimus::{UiEvent, WidgetRect};

use crate::action::Motion;

/// The mouse event with coordinates translated into the widget's rect;
/// `None` when the event is not a mouse event or missed the rect.
pub fn local_event(world: &World, entity: Entity, event: UiEvent) -> Option<LocalMouse> {
    let UiEvent::Mouse(mouse) = event else {
        return None;
    };
    let rect = world.get::<WidgetRect>(entity)?.0;
    if !rect.contains(ratatui::layout::Position::new(mouse.column, mouse.row)) {
        return None;
    }
    Some(LocalMouse {
        kind: mouse.kind,
        column: mouse.column - rect.x,
        row: mouse.row - rect.y,
        raw: mouse,
    })
}

pub struct LocalMouse {
    pub kind: MouseEventKind,
    pub column: u16,
    pub row: u16,
    pub raw: MouseEvent,
}

impl LocalMouse {
    pub fn is_left_click(&self) -> bool {
        matches!(self.kind, MouseEventKind::Down(MouseButton::Left))
    }

    /// `Prev` for wheel-up, `Next` for wheel-down.
    pub fn wheel_motion(&self) -> Option<Motion> {
        match self.kind {
            MouseEventKind::ScrollUp => Some(Motion::Prev),
            MouseEventKind::ScrollDown => Some(Motion::Next),
            _ => None,
        }
    }

    pub fn is_move(&self) -> bool {
        matches!(self.kind, MouseEventKind::Moved)
    }
}

/// Base surfaces ignore the mouse while a modal owns input — the
/// picker, the file explorer, or a y/n prompt.
pub fn is_modal_open(world: &World) -> bool {
    world
        .get_resource::<crate::overlay::ActiveOverlay>()
        .is_some_and(|overlay| overlay.is_open())
        || world
            .get_resource::<crate::explorer::ExplorerState>()
            .is_some_and(|explorer| explorer.is_open())
        || world
            .get_resource::<crate::prompt::PromptState>()
            .is_some_and(|prompt| prompt.is_open())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use bevy_ratatui::crossterm::event::KeyModifiers;
    use ratatui::layout::Rect;

    use super::*;

    fn mouse(kind: MouseEventKind, column: u16, row: u16) -> UiEvent {
        UiEvent::Mouse(MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        })
    }

    fn world_with_rect(rect: Rect) -> (World, Entity) {
        let mut world = World::new();
        let entity = world.spawn(WidgetRect(rect)).id();
        (world, entity)
    }

    #[test]
    fn local_event_translates_hits_and_rejects_misses() {
        let (world, entity) = world_with_rect(Rect::new(10, 5, 20, 10));
        let hit = local_event(
            &world,
            entity,
            mouse(MouseEventKind::Down(MouseButton::Left), 12, 7),
        )
        .unwrap();
        assert_eq!((hit.column, hit.row), (2, 2));
        assert!(hit.is_left_click());

        let miss = local_event(&world, entity, mouse(MouseEventKind::Moved, 9, 7));
        assert!(miss.is_none(), "a point left of the rect must not hit");
        let below = local_event(&world, entity, mouse(MouseEventKind::Moved, 12, 15));
        assert!(below.is_none(), "the rect's bottom edge is exclusive");
    }

    #[test]
    fn wheel_maps_up_to_prev_and_down_to_next() {
        let (world, entity) = world_with_rect(Rect::new(0, 0, 10, 10));
        let up = local_event(&world, entity, mouse(MouseEventKind::ScrollUp, 1, 1)).unwrap();
        assert_eq!(up.wheel_motion(), Some(Motion::Prev));
        let down = local_event(&world, entity, mouse(MouseEventKind::ScrollDown, 1, 1)).unwrap();
        assert_eq!(down.wheel_motion(), Some(Motion::Next));
        assert!(!down.is_left_click());
    }
}
