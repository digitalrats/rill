//! OSC sensor — receives OSC messages over UDP,
//! parses them into [`ControlEvent`]s, and sends them via [`ActorRef`].
//!
//! Two implementations:
//! - [`OscSensor`] — standalone sensor with its own `ActorRef<ControlEvent>` (legacy)
//! - [`spawn_osc_sensor`] — integrates with the actor model: control through
//!   `ActorRef<CommandEnum>`, polling in a dedicated OS thread.

use std::net::{SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use rill_core::queues::{CommandEnum, SensorCommand};
use rill_core_actor::{ActorRef, ActorSystem};
use rill_osc::osc::{self, OscMessage, OscPacket};

use crate::engine::{ControlEvent, Module};
use crate::sensor::Sensor;

/// OSC sensor — polls a UDP socket on a dedicated OS thread,
/// decodes incoming packets into [`ControlEvent::Osc`] events,
/// and dispatches via [`ActorRef`].
pub struct OscSensor {
    id: String,
    thread: Option<JoinHandle<()>>,
    running: Arc<AtomicBool>,
    events: Option<ActorRef<ControlEvent>>,
    bind_addr: SocketAddr,
}

impl OscSensor {
    /// Create a new OSC sensor bound to the given UDP address.
    pub fn new(id: impl Into<String>, bind_addr: SocketAddr) -> Self {
        Self {
            id: id.into(),
            thread: None,
            running: Arc::new(AtomicBool::new(true)),
            events: None,
            bind_addr,
        }
    }

    /// Convenience: create, attach, and start in one call.
    pub fn start(
        id: impl Into<String>,
        bind_addr: SocketAddr,
        events: ActorRef<ControlEvent>,
    ) -> Self {
        let mut sensor = Self::new(id, bind_addr);
        sensor.attach(events);
        sensor.start();
        sensor
    }
}

impl Module for OscSensor {
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

impl Sensor for OscSensor {
    fn attach(&mut self, events: ActorRef<ControlEvent>) {
        self.events = Some(events);
    }

    fn start(&mut self) {
        let events = self
            .events
            .take()
            .expect("OscSensor: attach() must be called before start()");
        let bind_addr = self.bind_addr;
        let r = self.running.clone();
        let mid = self.id.clone();

        self.thread = Some(thread::spawn(move || {
            let socket = match UdpSocket::bind(bind_addr) {
                Ok(s) => s,
                Err(e) => {
                    log::error!("OscSensor '{mid}': failed to bind {bind_addr}: {e}");
                    return;
                }
            };
            let _ = socket.set_nonblocking(true);

            let mut buf = [0u8; 65536];

            while r.load(Ordering::Acquire) {
                match socket.recv_from(&mut buf) {
                    Ok((n, _src)) => match osc::decode(&buf[..n]) {
                        Ok(packet) => {
                            for event in osc_packet_to_events(&packet) {
                                events.send(event);
                            }
                        }
                        Err(e) => {
                            log::warn!("OscSensor '{mid}': decode error: {e}");
                        }
                    },
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(1));
                    }
                    Err(e) => {
                        log::warn!("OscSensor '{mid}': recv error: {e}");
                        thread::sleep(Duration::from_millis(10));
                    }
                }
            }
        }));
    }
}

impl Drop for OscSensor {
    fn drop(&mut self) {
        self.stop();
    }
}

// =============================================================================
// OscSensor — actor-model OSC sensor
// =============================================================================

/// Spawns an OSC sensor that integrates with the actor model.
///
/// The polling loop runs in a dedicated OS thread. Raw OSC messages are
/// decoded into [`ControlEvent`]s and sent to the **servo** via
/// `CommandEnum::Control`. The servo applies mappings and sends
/// `SetParameter` to the graph — the sensor never maps or writes
/// parameters directly.
///
/// # Arguments
/// * `id` — unique sensor identifier
/// * `bind_addr` — UDP socket address to bind
/// * `system` — actor system for spawning the control actor
/// * `servo_ref` — target servo's actor reference for delivering events
pub fn spawn_osc_sensor(
    id: &str,
    bind_addr: SocketAddr,
    system: &ActorSystem,
    servo_ref: ActorRef<CommandEnum>,
) -> ActorRef<CommandEnum> {
    let enabled = Arc::new(AtomicBool::new(true));
    let sr = servo_ref.clone();
    let oid = id.to_string();

    // Control actor — receives messages from RackActor fan-out (SetEnabled, etc.).
    let actor_ref = system.spawn_detached(
        &format!("osc_{id}"),
        {
            let e2 = enabled.clone();
            move || {
                Box::new(move |msg: CommandEnum| {
                    if let CommandEnum::Sensor(SensorCommand::SetEnabled { enabled: en, .. }) = msg {
                        e2.store(en, Ordering::Release);
                    }
                })
            }
        },
        10,
    );

    // Polling thread — decodes OSC, sends raw events to the servo
    thread::spawn(move || {
        let socket = match UdpSocket::bind(bind_addr) {
            Ok(s) => s,
            Err(e) => {
                log::error!("osc sensor '{oid}' failed to bind {bind_addr}: {e}");
                return;
            }
        };
        let _ = socket.set_nonblocking(true);
        let mut buf = [0u8; 65536];

        loop {
            thread::sleep(Duration::from_millis(5));

            if !enabled.load(Ordering::Acquire) {
                continue;
            }
            match socket.recv_from(&mut buf) {
                Ok((n, _src)) => match osc::decode(&buf[..n]) {
                    Ok(packet) => {
                        for event in osc_packet_to_events(&packet) {
                            sr.send(CommandEnum::Control(event));
                        }
                    }
                    Err(e) => {
                        log::warn!("osc sensor '{oid}' decode error: {e}");
                    }
                },
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(e) => {
                    log::warn!("osc sensor '{oid}' recv error: {e}");
                    thread::sleep(Duration::from_millis(50));
                }
            }
        }
    });

    actor_ref
}

// =============================================================================
// Parsing — OSC message → ControlEvent
// =============================================================================

/// Convert an OSC packet into a `Vec` of [`ControlEvent`]s,
/// recursively unwrapping bundles.
fn osc_packet_to_events(packet: &OscPacket) -> Vec<ControlEvent> {
    let mut events = Vec::new();
    match packet {
        OscPacket::Message(msg) => {
            events.push(osc_message_to_event(msg));
        }
        OscPacket::Bundle(bundle) => {
            for inner in &bundle.packets {
                events.extend(osc_packet_to_events(inner));
            }
        }
    }
    events
}

/// Convert a single OSC message into a [`ControlEvent::Osc`].
fn osc_message_to_event(msg: &OscMessage) -> ControlEvent {
    let args: Vec<f32> = msg
        .args
        .iter()
        .filter_map(|a| match a {
            rill_osc::osc::OscType::Float(f) => Some(*f),
            rill_osc::osc::OscType::Int(i) => Some(*i as f32),
            _ => None,
        })
        .collect();

    ControlEvent::Osc {
        address: msg.addr.clone(),
        args,
    }
}

/// Parse an OSC message into a [`ControlEvent`].
///
/// Converts the message address and numeric arguments into
/// a [`ControlEvent::Osc`] variant. Non-numeric arguments
/// (strings, blobs) are silently dropped.
pub fn parse_osc(msg: &OscMessage) -> ControlEvent {
    osc_message_to_event(msg)
}
