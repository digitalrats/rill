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

use crate::engine::{ControlEvent, MidiTransportKind, Module};
use crate::midi_clock::MidiClockTracker;
use crate::sensor::Sensor;

/// MIDI sensor — polls a [`MidiBackend`] on a dedicated OS thread,
/// parses raw bytes into [`ControlEvent`]s, and dispatches via [`ActorRef`].
///
/// Optionally integrates a [`MidiClockTracker`] for MIDI clock sync:
/// when present, each raw status byte is fed to the tracker before
/// normal parsing, enabling BPM derivation from clock pulses.
pub struct MidiHub {
    id: String,
    thread: Option<JoinHandle<()>>,
    running: Arc<AtomicBool>,
    events: Option<ActorRef<ControlEvent>>,
    backend: Option<Box<dyn MidiBackend>>,
    tracker: Option<MidiClockTracker>,
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
            tracker: None,
        }
    }

    /// Create a MIDI sensor with an integrated [`MidiClockTracker`].
    ///
    /// The tracker receives raw status bytes from every incoming message.
    /// Use [`MidiHub::shared_clock`] to obtain the `Arc<SystemClock>` for
    /// wiring into the signal graph.
    pub fn with_clock_tracker(
        id: impl Into<String>,
        backend: Box<dyn MidiBackend>,
        tracker: MidiClockTracker,
    ) -> Self {
        Self {
            id: id.into(),
            thread: None,
            running: Arc::new(AtomicBool::new(true)),
            events: None,
            backend: Some(backend),
            tracker: Some(tracker),
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

    /// Return a clone of the shared `SystemClock` if a clock tracker is active.
    pub fn shared_clock(&self) -> Option<Arc<rill_core::time::SystemClock>> {
        self.tracker.as_ref().map(|t| t.shared_clock())
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
        let mut tracker = self.tracker.take();
        let r = self.running.clone();

        self.thread = Some(thread::spawn(move || {
            let mut backend = backend;
            while r.load(Ordering::Acquire) {
                match backend.poll() {
                    Ok(msgs) => {
                        for msg in msgs {
                            if let Some(ref mut t) = tracker {
                                t.process_status(msg.status());
                            }
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
/// The polling loop runs in a dedicated OS thread. Raw MIDI bytes are
/// decoded into [`ControlEvent`]s and sent to the **servo** via
/// `CommandEnum::Control`. The servo applies mappings and sends
/// `SetParameter` to the graph — the sensor never maps or writes
/// parameters directly.
///
/// # Arguments
/// * `id` — unique sensor identifier
/// * `backend` — MIDI I/O backend (e.g. [`MidirBackend`], [`AlsaSeqBackend`])
/// * `system` — actor system for spawning the control actor
/// * `servo_ref` — target servo's actor reference for delivering events
pub fn spawn_midi_sensor(
    id: &str,
    backend: Box<dyn MidiBackend>,
    system: &ActorSystem,
    servo_ref: ActorRef<CommandEnum>,
) -> ActorRef<CommandEnum> {
    let enabled = Arc::new(AtomicBool::new(true));
    let sr = servo_ref.clone();
    let mid = id.to_string();

    // Control actor — receives messages from RackActor fan-out (SetEnabled, etc.).
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

    // Polling thread — decodes MIDI, sends raw events to the servo
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
                            sr.send(CommandEnum::Control(event));
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
