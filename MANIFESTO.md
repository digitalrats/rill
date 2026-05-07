# The Rill Manifesto

## Drifting along the stream of signals

We are building **Rill** — an infrastructure for distributed intelligence, where the periphery (Graph) meets the mind (Patchbay), and the protocol between them is a nervous system connecting the fast world of sensors and actuators with the slow world of thinking and memory.

Rill was not born as an architecture. It grew from a simple desire: to build a software analog of the Bastl Instruments Thyme+ pedal. But the deeper I dived into the code, the clearer I saw: behind this lie principles that work everywhere — from audio effects to industrial automation, from robotics to distributed AI.

---

## Three principles of Rill

### 1. Separation of worlds

- **Hard real-time world (Graph)** — fast, deterministic, bounded. Here live sensors (sound, CAN bus, temperature) and actuators (speakers, motors, relays). No allocations, no locks, no doubts. Pure data flow.

- **Control world (Patchbay)** — slow, complex, unbounded. Here live automata (LFOs, envelopes, logic), here they communicate with the user (GUI, MIDI, OSC), here they store history and make decisions. Here you can think.

- **Protocol between them** — asynchronous, fault-tolerant, scalable. Command queues (Soft RT → Hard RT) and telemetry (Hard RT → Soft RT). This is the nervous system connecting reflexes with intelligence.

### 2. Block coherence

Graph parameters do not change within a block. They are fixed at its boundary and applied uniformly to all samples.

This gives:
- Predictability (no clicks or glitches)
- Performance (SIMD-friendly)
- Simplicity of reasoning about the system

### 3. Protocol as foundation

Graph and Patchbay do not have to live in the same process — or even on the same node.

Locally — `crossbeam_channel` (fast). Globally — TCP, UDP, WebSocket, LoRa (reliable, far, cheap).

By designing the protocol, we design the future. Internal Internet-Drafts today — potential RFCs tomorrow.

---

## What we don't do

We do not chase **perfect form**. We don't write code for code's sake. We don't document for documentation's sake.

Every line of Rill answers the question: **"Does this solve a real problem in real time?"**

If not — it shouldn't exist.

---

## Why Rill?

**Rill** is a stream. Not a river (too powerful), not a flow (too technological), but a stream. It flows where there is a slope. It doesn't choose its bed — it goes around obstacles. It doesn't fight stones — it washes over them. It doesn't promise an ocean — but it gets there.

**Rill Adrift** — a drifting stream. It takes the temperature of the world, compensates for its chaos, and simply flows. Because data flows. Signals flow. Life flows.

---

## Join us

Rill is an open technology. Its code is on GitHub and SourceCraft, its documentation is in Obsidian, its spirit — in this manifesto.

Commercialization? Perhaps. Standardization? When the time comes. The main thing is **infrastructure** on which you can build anything, from an effects pedal to cloud AI.

**I just wanted to create a software analog of the Bastl Instruments Thyme+, but got a little carried away.**