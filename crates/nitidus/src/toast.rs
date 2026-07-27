//! Toast notifications via ratatui-comfy-toaster, drawn as a custom
//! plurimus `DrawFn` layer above every widget. Two feeders: the outbox
//! countdown, and whatever `MessageLog` judged too loud for the
//! statusline.
//!
//! The engine is `!Sync`, so it rides inside a `Mutex` on the component
//! (the crate's own recommendation); ticking happens in the draw call,
//! which runs once per frame. Display-only: `z` and friends stay router
//! bindings.

use std::sync::Mutex;
use std::time::Duration;

use bevy::prelude::*;
use bevy_trait_query::RegisterExt;
use plurimus::{DrawFn, WidgetLayout, WidgetOrder};
use ratatui::layout::Rect;
use ratatui::widgets::WidgetRef;
use ratatui_comfy_toaster::{ToastBuilder, ToastEngine, ToastEngineBuilder, ToastType};

use crate::outbox::OutboxState;
use crate::status::{MessageLog, Severity};
use nitidus_ui_kit::{layer, layout};

const SENT_TOAST: Duration = Duration::from_secs(4);
/// Long enough to read a sentence without becoming furniture; the log
/// keeps the copy either way.
const MESSAGE_TOAST: Duration = Duration::from_secs(6);
/// The countdown toast refreshes once a second; re-showing with dedup
/// keeps it a single surface.
const COUNTDOWN_ID_MESSAGE_PREFIX: &str = "sending in ";

pub struct ToastPlugin;

impl Plugin for ToastPlugin {
    fn build(&self, app: &mut App) {
        app.register_component_as::<dyn DrawFn, ToastLayer>();
        app.init_resource::<CountdownShown>();
        app.add_systems(Startup, spawn_toasts);
        app.add_systems(Update, (refresh_send_toasts, surface_logged_messages));
    }
}

/// Last countdown second surfaced, so the toast re-shows only on
/// change.
#[derive(Resource, Default)]
struct CountdownShown(Option<u64>);

#[derive(Component)]
pub struct ToastLayer(Mutex<ToastEngine<()>>);

impl ToastLayer {
    fn show(&self, toast: ToastBuilder) {
        if let Ok(mut engine) = self.0.lock() {
            engine.show_toast(toast);
        }
    }

    fn dismiss_countdowns(&self) {
        if let Ok(mut engine) = self.0.lock() {
            while engine
                .current_message()
                .is_some_and(|message| message.starts_with(COUNTDOWN_ID_MESSAGE_PREFIX))
            {
                if !engine.dismiss() {
                    break;
                }
            }
        }
    }
}

impl DrawFn for ToastLayer {
    fn draw(&mut self, frame: &mut ratatui::Frame, area: Rect) -> Result {
        let Ok(mut engine) = self.0.lock() else {
            return Ok(());
        };
        engine.set_area(area);
        engine.tick();
        engine.render_ref(area, frame.buffer_mut());
        Ok(())
    }
}

/// Drains what `MessageLog` marked loud enough to interrupt. The policy
/// lives in `status`; this only draws the result.
fn surface_logged_messages(mut log: ResMut<MessageLog>, layers: Query<&ToastLayer>) {
    let Ok(layer) = layers.single() else {
        return;
    };
    for entry in log.take_pending() {
        let toast_type = match entry.severity {
            Severity::Error => ToastType::Error,
            Severity::Warning => ToastType::Warning,
            Severity::Info => ToastType::Info,
        };
        layer.show(
            ToastBuilder::new(entry.text.into())
                .toast_type(toast_type)
                .duration(MESSAGE_TOAST),
        );
    }
}

fn spawn_toasts(mut commands: Commands) {
    let engine = ToastEngineBuilder::new(Rect::default()).dedup(true).build();
    commands.spawn((
        ToastLayer(Mutex::new(engine)),
        WidgetOrder(layer::TOAST),
        WidgetLayout::from(layout::content_layout()),
    ));
}

/// Mirrors the outbox into toasts: a per-second countdown while an
/// entry waits, a sending toast on submission, and success feedback
/// arrives via the statusline (`SendDone` handling).
fn refresh_send_toasts(
    outbox: Res<OutboxState>,
    mut shown: ResMut<CountdownShown>,
    layers: Query<&ToastLayer>,
) {
    let Ok(layer) = layers.single() else {
        return;
    };
    let countdown = outbox.countdown_ms().map(|ms| ms.div_ceil(1000) as u64);
    if countdown == shown.0 {
        return;
    }
    shown.0 = countdown;
    match countdown {
        Some(seconds) => {
            layer.dismiss_countdowns();
            layer.show(
                ToastBuilder::new(
                    format!("{COUNTDOWN_ID_MESSAGE_PREFIX}{seconds}s — z undoes").into(),
                )
                .toast_type(ToastType::Info)
                .duration(Duration::from_millis(1_100)),
            );
        }
        None if outbox.is_sending() => {
            layer.dismiss_countdowns();
            layer.show(
                ToastBuilder::new("sending…".into())
                    .toast_type(ToastType::Info)
                    .duration(SENT_TOAST),
            );
        }
        None => layer.dismiss_countdowns(),
    }
}
