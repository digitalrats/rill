//! Control events — hardware input events decoded from sensors.
//!
//! These types represent raw control input from physical interfaces
//! (MIDI controllers, OSC surfaces, buttons, knobs, faders) and
//! MIDI transport/clock events. They are decoded by sensors and
//! dispatched to servos for mapping to graph parameters.

// =============================================================================
// MidiNoteKind
// =============================================================================

/// What aspect of a MIDI note event to extract for mapping.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum MidiNoteKind {
    /// Extracts frequency: `midi_to_freq(note)`. Note Off produces no value.
    Frequency,
    /// Extracts amplitude: `velocity / 127` (On) or `0.0` (Off).
    #[default]
    Amplitude,
    /// Extracts gate: `1.0` (On) or `0.0` (Off).
    Gate,
}

// =============================================================================
// MidiTransportKind
// =============================================================================

/// MIDI transport state.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MidiTransportKind {
    /// Transport started.
    Start,
    /// Transport stopped.
    Stop,
    /// Transport resumed from current position.
    Continue,
}

// =============================================================================
// EventPattern
// =============================================================================

/// A pattern for matching controller events.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum EventPattern {
    /// Matches any button event regardless of ID.
    AnyButton,
    /// Matches a button event with a specific hardware ID.
    ButtonId(u32),
    /// Matches any knob event regardless of ID.
    AnyKnob,
    /// Matches a knob event with a specific hardware ID.
    KnobId(u32),
    /// Matches any fader event regardless of ID.
    AnyFader,
    /// Matches a fader event with a specific hardware ID.
    FaderId(u32),
    /// Matches any MIDI event (control change, note, clock, or transport).
    AnyMidi,
    /// Matches a MIDI control change event by controller number and optional channel.
    MidiControl {
        /// Optional MIDI channel filter; `None` matches any channel.
        channel: Option<u8>,
        /// MIDI controller number (CC index).
        controller: u8,
    },
    /// Matches a MIDI note-on or note-off event and extracts a mapped value.
    MidiNote {
        /// Optional MIDI channel filter; `None` matches any channel.
        channel: Option<u8>,
        /// Optional note number filter; `None` matches any note.
        note: Option<u8>,
        /// Which aspect of the note event to use as the mapping value.
        #[cfg_attr(feature = "serde", serde(default))]
        kind: MidiNoteKind,
    },
    /// Matches a MIDI clock tick event.
    MidiClock,
    /// Matches a MIDI transport event (start, stop, or continue).
    MidiTransport {
        /// Optional transport kind filter; `None` matches any transport event.
        kind: Option<MidiTransportKind>,
    },
    /// Matches an OSC message by exact address string.
    OscAddress(String),
    /// Matches an OSC message whose address contains the given substring.
    OscPattern(String),
}

// =============================================================================
// ControlEvent
// =============================================================================

/// Hardware control event from a physical interface (knob, button, fader, etc.).
///
/// Produced by sensors (MIDI, OSC, CV/Gate) and dispatched to servos
/// for mapping to graph parameters.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq)]
pub enum ControlEvent {
    /// A physical button press or release.
    Button {
        /// Hardware control identifier.
        id: u32,
        /// `true` if the button is currently held down.
        pressed: bool,
    },
    /// A physical knob (rotary encoder or potentiometer) event.
    Knob {
        /// Hardware control identifier.
        id: u32,
        /// Raw value in hardware-native units.
        value: f32,
        /// Value mapped to the [0.0, 1.0] range.
        normalized: f32,
    },
    /// A physical fader (linear slider) event.
    Fader {
        /// Hardware control identifier.
        id: u32,
        /// Raw value in hardware-native units.
        value: f32,
        /// Value mapped to the [0.0, 1.0] range.
        normalized: f32,
    },
    /// A MIDI control change message.
    MidiControl {
        /// MIDI channel (0-indexed).
        channel: u8,
        /// MIDI controller number.
        controller: u8,
        /// Raw 7-bit MIDI value.
        value: u8,
        /// Value normalized to [0.0, 1.0].
        normalized: f32,
    },
    /// A MIDI note-on or note-off message.
    MidiNote {
        /// MIDI channel (0-indexed).
        channel: u8,
        /// MIDI note number.
        note: u8,
        /// MIDI velocity value (0-127).
        velocity: u8,
        /// `true` for note-on, `false` for note-off.
        on: bool,
    },
    /// An OSC message event.
    Osc {
        /// OSC address path.
        address: String,
        /// OSC argument list as float values.
        args: Vec<f32>,
    },
    /// A MIDI clock tick event.
    MidiClock,
    /// A MIDI transport state change.
    MidiTransport {
        /// The type of transport event (start, stop, or continue).
        kind: MidiTransportKind,
    },
}

impl EventPattern {
    /// Checks whether this pattern matches a given control event.
    pub fn matches(&self, event: &ControlEvent) -> bool {
        match (self, event) {
            (EventPattern::AnyButton, ControlEvent::Button { .. }) => true,
            (EventPattern::ButtonId(id), ControlEvent::Button { id: eid, .. }) => *id == *eid,
            (EventPattern::AnyKnob, ControlEvent::Knob { .. }) => true,
            (EventPattern::KnobId(id), ControlEvent::Knob { id: eid, .. }) => *id == *eid,
            (EventPattern::AnyFader, ControlEvent::Fader { .. }) => true,
            (EventPattern::FaderId(id), ControlEvent::Fader { id: eid, .. }) => *id == *eid,
            (
                EventPattern::MidiControl {
                    channel,
                    controller,
                },
                ControlEvent::MidiControl {
                    channel: ech,
                    controller: ectr,
                    ..
                },
            ) => (channel.is_none() || channel.unwrap() == *ech) && *controller == *ectr,
            (
                EventPattern::MidiNote { channel, note, .. },
                ControlEvent::MidiNote {
                    channel: ech,
                    note: en,
                    ..
                },
            ) => {
                (channel.is_none() || channel.unwrap() == *ech)
                    && (note.is_none() || note.unwrap() == *en)
            }
            (EventPattern::AnyMidi, ControlEvent::MidiControl { .. })
            | (EventPattern::AnyMidi, ControlEvent::MidiNote { .. })
            | (EventPattern::AnyMidi, ControlEvent::MidiClock)
            | (EventPattern::AnyMidi, ControlEvent::MidiTransport { .. }) => true,
            (EventPattern::MidiClock, ControlEvent::MidiClock) => true,
            (
                EventPattern::MidiTransport { kind },
                ControlEvent::MidiTransport { kind: ek, .. },
            ) => kind.is_none_or(|k| k == *ek),
            (EventPattern::OscAddress(addr), ControlEvent::Osc { address, .. }) => addr == address,
            (EventPattern::OscPattern(pat), ControlEvent::Osc { address, .. }) => {
                address.contains(pat)
            }
            _ => false,
        }
    }
}

impl ControlEvent {
    /// Returns the normalized value (0.0–1.0) of this event, if it carries one.
    pub fn normalized_value(&self) -> Option<f32> {
        match self {
            ControlEvent::Knob { normalized, .. } => Some(*normalized),
            ControlEvent::Fader { normalized, .. } => Some(*normalized),
            ControlEvent::MidiControl { normalized, .. } => Some(*normalized),
            ControlEvent::Button { pressed, .. } => Some(if *pressed { 1.0 } else { 0.0 }),
            _ => None,
        }
    }

    /// Returns the hardware control ID attached to this event, if any.
    pub fn id(&self) -> Option<u32> {
        match self {
            ControlEvent::Button { id, .. } => Some(*id),
            ControlEvent::Knob { id, .. } => Some(*id),
            ControlEvent::Fader { id, .. } => Some(*id),
            _ => None,
        }
    }
}
