//! MIDI actor — receives raw MIDI messages from a backend,
//! parses them into [`ControlEvent`]s, and feeds the [`Patchbay`].

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use rill_io::midi_backend::MidiBackend;
use rill_io::midi_message::MidiMessage;

use crate::engine::{ControlEvent, MidiTransportKind, Patchbay};

/// Drives a [`MidiBackend`] on a dedicated thread, converts raw
/// MIDI bytes into [`ControlEvent`]s, and dispatches them to a
/// shared [`Patchbay`].
///
/// The actor is intentionally simple — no message passing, just
/// a polling loop → parse → dispatch cycle on its own thread.
pub struct MidiActor {
    thread: Option<JoinHandle<()>>,
    running: Arc<AtomicBool>,
}

impl MidiActor {
    /// Start the MIDI actor.
    ///
    /// Spawns a dedicated thread that polls `backend` for messages,
    /// parses them, and calls `patchbay.handle_event()`.
    pub fn start(backend: Box<dyn MidiBackend>, patchbay: Arc<Mutex<Patchbay>>) -> Self {
        let running = Arc::new(AtomicBool::new(true));
        let r = running.clone();

        let thread = thread::spawn(move || {
            let mut backend = backend;
            while r.load(Ordering::Acquire) {
                match backend.poll() {
                    Ok(events) => {
                        for msg in events {
                            if let Some(event) = parse_midi(&msg) {
                                let _ = patchbay.lock().map(|mut pb| {
                                    pb.handle_event(event);
                                });
                            }
                        }
                    }
                    Err(e) => {
                        log::warn!("midi backend poll error: {e}");
                        // Brief pause on error to avoid tight spin loop.
                        thread::sleep(Duration::from_millis(10));
                    }
                }
                // Small yield to avoid busy-waiting when no events.
                thread::sleep(Duration::from_millis(1));
            }
        });

        MidiActor {
            thread: Some(thread),
            running,
        }
    }

    /// Stop the actor and join its thread.
    pub fn stop(&mut self) {
        self.running.store(false, Ordering::Release);
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for MidiActor {
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
