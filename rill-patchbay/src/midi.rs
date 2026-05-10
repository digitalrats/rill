//! MIDI sensor — receives raw MIDI messages from a backend,
//! parses them into [`ControlEvent`]s, and sends them via [`ActorRef`].
//!
//! Uses a dedicated OS thread for polling. Multiple sensors can run
//! independently — events from all sources arrive via a shared mailbox.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use rill_core_actor::ActorRef;
use rill_io::midi_backend::MidiBackend;
use rill_io::midi_message::MidiMessage;

use crate::engine::{ControlEvent, MidiTransportKind};
use crate::sensor::Sensor;

/// MIDI sensor — polls a [`MidiBackend`] on a dedicated OS thread,
/// parses raw bytes into [`ControlEvent`]s, and dispatches via [`ActorRef`].
pub struct MidiHub {
    thread: Option<JoinHandle<()>>,
    running: Arc<AtomicBool>,
    events: Option<ActorRef<ControlEvent>>,
    backend: Option<Box<dyn MidiBackend>>,
}

impl MidiHub {
    /// Create a new MIDI sensor with a backend.
    pub fn new(backend: Box<dyn MidiBackend>) -> Self {
        Self {
            thread: None,
            running: Arc::new(AtomicBool::new(true)),
            events: None,
            backend: Some(backend),
        }
    }

    /// Convenience: create, attach, and start in one call.
    pub fn start(backend: Box<dyn MidiBackend>, events: ActorRef<ControlEvent>) -> Self {
        let mut hub = Self::new(backend);
        hub.attach(events);
        hub.start();
        hub
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

    fn stop(&mut self) {
        self.running.store(false, Ordering::Release);
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for MidiHub {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Parse a raw [`MidiMessage`] into a [`ControlEvent`].
fn parse_midi(msg: &MidiMessage) -> Option<ControlEvent> {
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
