//! OSC server with system handlers and user surface dispatch.
//!
//! **System paths** (`/sys/*`) — fixed protocol for host-level operations:
//!
//! | Path | Args | Action |
//! |---|---|---|
//! | `/sys/param/set` | `node param value` | Direct parameter set (via queue) |
//! | `/sys/graph/stop` | — | Log stop request |
//! | `/sys/status` | — | Print runtime state |
//!
//! **User paths** — registered from `PatchbayDocument::osc_surface`.
//! Each entry maps an OSC address to an `EventPattern`; the value is
//! extracted from the first OSC argument (treated as normalized 0..1).

use std::net::SocketAddr;
use std::sync::Arc;

use rill_core::queues::{SetParameter, SignalOrigin};
use rill_core::traits::{ActorRef, NodeId, ParamValue, ParameterId, PortId};
use rill_patchbay::control::{ControlEvent, EventPattern, OscSurface, PatchbayControl};

use crate::osc::osc::{OscMessage, OscType};
use crate::osc::server::OscServer;

/// Handle to a running OSC server + dispatch task.
pub struct OscHandle {
    /// The tokio task that runs the OSC recv loop.
    pub task: tokio::task::JoinHandle<()>,
}

impl OscHandle {
    /// Bind an OSC server, register system + surface handlers, spawn recv loop.
    pub async fn start(
        bind: &str,
        queue: ActorRef<SetParameter>,
        control: Arc<std::sync::Mutex<PatchbayControl>>,
        surface: OscSurface,
    ) -> Result<Self, String> {
        let mut server = OscServer::bind(bind)
            .await
            .map_err(|e| format!("OSC bind failed: {e}"))?;

        // ── System handlers ────────────────────────────────────────────

        // /sys/param/set <node> <param> <value>
        let q = queue.clone();
        server.handle("/sys/param/set", move |msg: OscMessage, _: SocketAddr| {
            if msg.args.len() < 3 {
                return;
            }
            let node = match &msg.args[0] {
                OscType::Int(i) => NodeId(*i as u32),
                _ => return,
            };
            let param = match &msg.args[1] {
                OscType::String(s) => s.clone(),
                _ => return,
            };
            let value: f32 = match &msg.args[2] {
                OscType::Float(f) => *f,
                OscType::Int(i) => *i as f32,
                _ => return,
            };
            if let Ok(pid) = ParameterId::new(&param) {
                q.send(SetParameter::new(
                    PortId::param(node, 0),
                    pid,
                    ParamValue::Float(value),
                    SignalOrigin::External("osc".into()),
                ));
            }
        });

        // /sys/graph/stop
        server.handle("/sys/graph/stop", |_: OscMessage, _: SocketAddr| {
            log::info!("OSC: /sys/graph/stop");
        });

        // /sys/status
        server.handle("/sys/status", move |_: OscMessage, _: SocketAddr| {
            log::info!("status: alive (ActorRef holds strong ref)");
        });

        // ── User surface handlers ──────────────────────────────────────

        for entry in surface {
            let path = entry.osc_path;
            let q = queue.clone();
            let ctrl = control.clone();
            let pattern = entry.event_pattern;

            server.handle(path.clone(), move |msg: OscMessage, _: SocketAddr| {
                let value = match msg.args.first() {
                    Some(OscType::Float(f)) => *f,
                    Some(OscType::Int(i)) => *i as f32,
                    _ => 0.0,
                };

                let event = pattern_to_event(&pattern, value);

                if let Ok(mut guard) = ctrl.lock() {
                    guard.handle_event(event);
                } else {
                    log::warn!("OSC surface: control lock failed");
                    if let Some(normalized) = event.normalized_value() {
                        if let Ok(pid) = ParameterId::new(&path) {
                            q.send(SetParameter::new(
                                PortId::param(NodeId(0), 0),
                                pid,
                                ParamValue::Float(normalized),
                                SignalOrigin::External("osc".into()),
                            ));
                        }
                    }
                }
            });
        }

        let task = tokio::spawn(async move {
            log::info!("OSC server started");
            if let Err(e) = server.run().await {
                log::error!("OSC server error: {e}");
            }
        });

        Ok(Self { task })
    }
}

/// Build a `ControlEvent` from an `EventPattern` and a normalized value.
fn pattern_to_event(pattern: &EventPattern, value: f32) -> ControlEvent {
    match pattern {
        EventPattern::KnobId(id) => ControlEvent::Knob {
            id: *id,
            value,
            normalized: value,
        },
        EventPattern::ButtonId(id) => ControlEvent::Button {
            id: *id,
            pressed: value > 0.5,
        },
        EventPattern::FaderId(id) => ControlEvent::Fader {
            id: *id,
            value,
            normalized: value,
        },
        EventPattern::MidiControl {
            channel,
            controller,
        } => ControlEvent::MidiControl {
            channel: channel.unwrap_or(0),
            controller: *controller,
            value: (value * 127.0) as u8,
            normalized: value,
        },
        _ => ControlEvent::Osc {
            address: String::new(),
            args: vec![value],
        },
    }
}
