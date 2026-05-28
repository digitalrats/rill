//! MIDI sensor — receives raw MIDI messages from a backend,
//! parses them into [`ControlEvent`]s, and sends them via [`ActorRef`].
//!
//! Two implementations:
//! - [`MidiHub`] — standalone sensor with its own `ActorRef<ControlEvent>` (legacy)
//! - [`spawn_midi_sensor`] — integrates with the actor model: control through
//!   `ActorRef<CommandEnum>`, polling in a dedicated OS thread (like Graph's
//!   I/O callback that calls `actor.drain()`).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use rill_core::queues::{CommandEnum, SensorCommand};
use rill_core_actor::{ActorRef, ActorSystem};
use rill_io::midi_backend::MidiBackend;
use rill_io::midi_message::MidiMessage;

use crate::engine::{ControlEvent, Mapping, MidiTransportKind, Module};
use crate::sensor::Sensor;

/// MIDI sensor — polls a [`MidiBackend`] on a dedicated OS thread,
/// parses raw bytes into [`ControlEvent`]s, and dispatches via [`ActorRef`].
pub struct MidiHub {
    id: String,
    thread: Option<JoinHandle<()>>,
    running: Arc<AtomicBool>,
    events: Option<ActorRef<ControlEvent>>,
    backend: Option<Box<dyn MidiBackend>>,
}

impl MidiHub {
    /// Create a new MIDI sensor with a backend.
    pub fn new(id: impl Into<String>, backend: Box<dyn MidiBackend>) -> Self {
        Self {
            id: id.into(),
            thread: None,
            running: Arc::new(AtomicBool::new(true)),
            events: None,
            backend: Some(backend),
        }
    }

    /// Convenience: create, attach, and start in one call.
    pub fn start(
        id: impl Into<String>,
        backend: Box<dyn MidiBackend>,
        events: ActorRef<ControlEvent>,
    ) -> Self {
        let mut hub = Self::new(id, backend);
        hub.attach(events);
        hub.start();
        hub
    }
}

impl Module for MidiHub {
    fn id(&self) -> &str {
        &self.id
    }

    fn stop(&mut self) {
        self.running.store(false, Ordering::Release);
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}

impl Sensor for MidiHub {
    fn attach(&mut self, events: ActorRef<ControlEvent>) {
        self.events = Some(events);
    }

    fn start(&mut self) {
        let events = self
            .events
            .take()
            .expect("MidiHub: attach() must be called before start()");
        let backend = self
            .backend
            .take()
            .expect("MidiHub: already started or no backend");
        let r = self.running.clone();

        self.thread = Some(thread::spawn(move || {
            let mut backend = backend;
            while r.load(Ordering::Acquire) {
                match backend.poll() {
                    Ok(msgs) => {
                        for msg in msgs {
                            if let Some(event) = parse_midi(&msg) {
                                events.send(event);
                            }
                        }
                    }
                    Err(e) => {
                        log::warn!("midi backend poll error: {e}");
                        thread::sleep(Duration::from_millis(10));
                    }
                }
                thread::sleep(Duration::from_millis(1));
            }
        }));
    }
}

impl Drop for MidiHub {
    fn drop(&mut self) {
        self.stop();
    }
}

// =============================================================================
// MidiSensor — actor-model MIDI sensor
// =============================================================================

/// Spawns a MIDI sensor that integrates with the actor model.
///
/// Control messages ([`CommandEnum::Sensor`]) are received through the returned
/// [`ActorRef<CommandEnum>`] which should be registered in the rack's module map.
/// The polling loop runs in a dedicated OS thread.
///
/// # Arguments
/// * `id` — unique sensor identifier
/// * `backend` — MIDI I/O backend (e.g. [`MidirBackend`], [`AlsaSeqBackend`])
/// * `mappings` — event-to-parameter mappings; each mapping's pattern is
///   matched against parsed MIDI events and, on match, a
///   `SetParameter` is sent to `graph_ref`
/// * `system` — actor system for spawning the control actor
/// * `graph_ref` — target graph for parameter changes
pub fn spawn_midi_sensor(
    id: &str,
    backend: Box<dyn MidiBackend>,
    mappings: Vec<Mapping>,
    system: &ActorSystem,
    graph_ref: ActorRef<CommandEnum>,
) -> ActorRef<CommandEnum> {
    let enabled = Arc::new(AtomicBool::new(true));
    let gr = graph_ref.clone();
    let mid = id.to_string();

    // Control actor — receives messages from RackActor fan-out (SetEnabled, etc.).
    // Uses spawn_detached so the handler (!Send) stays inside its own thread.
    let actor_ref = system.spawn_detached(
        &format!("midi_{id}"),
        {
            let e2 = enabled.clone();
            move || {
                Box::new(move |msg: CommandEnum| {
                    if let CommandEnum::Sensor(SensorCommand::SetEnabled { enabled: en, .. }) = msg
                    {
                        e2.store(en, Ordering::Release);
                    }
                })
            }
        },
        10,
    );

    // Polling thread — external trigger, analogous to Graph's I/O callback.
    thread::spawn(move || {
        let mut backend = backend;
        loop {
            thread::sleep(Duration::from_millis(5));

            if !enabled.load(Ordering::Acquire) {
                continue;
            }
            match backend.poll() {
                Ok(msgs) => {
                    for msg in &msgs {
                        if let Some(event) = parse_midi(msg) {
                            for mapping in &mappings {
                                if let Some(sp) = mapping.apply(&event) {
                                    gr.send(CommandEnum::SetParameter(sp));
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    log::warn!("midi sensor '{mid}' poll error: {e}");
                    thread::sleep(Duration::from_millis(50));
                }
            }
        }
    });

    actor_ref
}

/// Parse a raw [`MidiMessage`] into a [`ControlEvent`].
pub fn parse_midi(msg: &MidiMessage) -> Option<ControlEvent> {
    let status = msg.status();
    match msg.message_type() {
        // Note Off
        0x80 => Some(ControlEvent::MidiNote {
            channel: msg.channel(),
            note: msg.data1(),
            velocity: 0,
            on: false,
        }),
        // Note On
        0x90 => {
            let velocity = msg.data2();
            if velocity == 0 {
                // Velocity 0 NoteOn = NoteOff
                Some(ControlEvent::MidiNote {
                    channel: msg.channel(),
                    note: msg.data1(),
                    velocity: 0,
                    on: false,
                })
            } else {
                Some(ControlEvent::MidiNote {
                    channel: msg.channel(),
                    note: msg.data1(),
                    velocity,
                    on: true,
                })
            }
        }
        // Polyphonic Aftertouch
        0xA0 => Some(ControlEvent::MidiNote {
            channel: msg.channel(),
            note: msg.data1(),
            velocity: msg.data2(),
            on: true,
        }),
        // Control Change
        0xB0 => Some(ControlEvent::MidiControl {
            channel: msg.channel(),
            controller: msg.data1(),
            value: msg.data2(),
            normalized: msg.data2() as f32 / 127.0,
        }),
        // Program Change
        0xC0 => unsupported(msg),
        // Channel Aftertouch
        0xD0 => unsupported(msg),
        // Pitch Bend
        0xE0 => {
            let lsb = msg.data1() as i32;
            let msb = msg.data2() as i32;
            let val = (msb << 7) | lsb;
            let normalized = val as f32 / 8191.0; // 0.0–2.0 range, ~1.0 center
            Some(ControlEvent::MidiControl {
                channel: msg.channel(),
                controller: 128, // pseudo-controller for pitch bend
                value: ((normalized * 127.0) as u8).min(127),
                normalized,
            })
        }
        // System real-time / common
        0xF0 => match status {
            0xF2 => unsupported(msg), // Song Position
            0xF8 => Some(ControlEvent::MidiClock),
            0xFA => Some(ControlEvent::MidiTransport {
                kind: MidiTransportKind::Start,
            }),
            0xFB => Some(ControlEvent::MidiTransport {
                kind: MidiTransportKind::Continue,
            }),
            0xFC => Some(ControlEvent::MidiTransport {
                kind: MidiTransportKind::Stop,
            }),
            _ => unsupported(msg),
        },
        _ => unsupported(msg),
    }
}

#[allow(clippy::unnecessary_wraps)]
fn unsupported(_msg: &MidiMessage) -> Option<ControlEvent> {
    None
}
