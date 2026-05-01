use crossbeam_channel::{Receiver, Sender, TryRecvError};
use rill_core::math::Transcendental;
use rill_core::queues::signal::CommandEnum;
use rill_core::queues::telemetry::Telemetry;
use rill_core::time::ClockTick;
use rill_core::traits::processable::{NodeVariant, Processable};
use rill_core::traits::port::Port;
use rill_core::traits::{AudioNode, PortId, ProcessResult};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;

/// Real-time safe audio engine for a static audio graph.
///
/// Owns the mutable node state and provides:
///
/// 1. **Clock boundary** — [`process_tick`](Self::process_tick): drains
///    commands (with anti-ack) and runs `pre_process` (feedback mix).
/// 2. **Block processing** — [`process_block`](Self::process_block):
///    iterates nodes in topological order: for each node, calls
///    `process_block`, then `snapshot_feedback`, then `propagate` to
///    downstream nodes via pre-established port connections.
/// 3. **Thread management** — [`start`](Self::start)/[`stop`](Self::stop)
///    manage a cooperative running flag; [`spawn`](Self::spawn) consumes
///    the engine and runs it in a dedicated audio thread.
///
/// A separate control thread communicates via command/telemetry queues.
///
/// # Type Parameters
/// - `T` — floating-point type (`f32` or `f64`)
/// - `BUF_SIZE` — block size (must match the graph)
pub struct AudioEngine<T: Transcendental, const BUF_SIZE: usize> {
    nodes: Vec<NodeVariant<T, BUF_SIZE>>,
    topo_order: Vec<usize>,
    cmd_slots: Vec<Option<CommandEnum>>,
    cmd_rx: Option<Receiver<CommandEnum>>,
    tel_tx: Option<Sender<Telemetry>>,
    running: Arc<AtomicBool>,
}

impl<T: Transcendental, const BUF_SIZE: usize> AudioEngine<T, BUF_SIZE> {
    /// Create a new engine from graph parts.
    pub fn new(
        nodes: Vec<NodeVariant<T, BUF_SIZE>>,
        topo_order: Vec<usize>,
        cmd_rx: Option<Receiver<CommandEnum>>,
        tel_tx: Option<Sender<Telemetry>>,
    ) -> Self {
        let node_count = nodes.len();
        Self {
            nodes,
            topo_order,
            cmd_slots: vec![None; node_count.max(1)],
            cmd_rx,
            tel_tx,
            running: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Process a clock tick — called from the I/O callback when hardware fires.
    ///
    /// Drains pending commands (with anti-ack telemetry on overwrite), then
    /// runs `pre_process` on all input ports (mixes feedback from previous block),
    /// then applies any pending `SetParameter` commands to their target nodes.
    ///
    /// Returns the number of commands applied this tick.
    pub fn process_tick(&mut self, tick: &ClockTick) -> usize {
        let mut applied = 0usize;

        // === 1. Drain command queue into sparse slots ===
        if let Some(ref rx) = self.cmd_rx {
            loop {
                let cmd = match rx.try_recv() {
                    Ok(cmd) => cmd,
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        self.cmd_rx = None;
                        break;
                    }
                };

                let nid = match cmd.target_node_id() {
                    Some(id) => id.inner() as usize,
                    None => continue,
                };

                if nid >= self.cmd_slots.len() {
                    continue;
                }

                // Anti-ack: if slot is occupied, notify control
                if self.cmd_slots[nid].is_some() {
                    let _ = self.tel_tx.as_ref().map(|tx| {
                        let _ = tx.try_send(Telemetry::event(
                            "engine",
                            "command_dropped",
                            vec![nid as f32],
                        ));
                    });
                }

                self.cmd_slots[nid] = Some(cmd);
            }
        }

        // === 2. pre_process on all nodes (feedback mix — block boundary) ===
        for &idx in &self.topo_order {
            let num_inputs = self.nodes[idx].num_audio_inputs();
            for pi in 0..num_inputs {
                if let Some(port) = self.nodes[idx].input_port_mut(pi) {
                    port.pre_process(tick);
                }
            }
        }

        // === 3. Apply pending commands ===
        for &idx in &self.topo_order {
            if let Some(cmd) = self.cmd_slots[idx].take() {
                if let Some(sp) = cmd.as_set_parameter() {
                    let _ = self.nodes[idx].apply_set_parameter(sp);
                    applied += 1;
                }
            }
        }

        applied
    }

    /// Convenience: run a full processing cycle for one block.
    ///
    /// Calls `process_tick`, then iterates nodes in topological order:
    /// **process → snapshot_feedback → propagate** for each node.
    /// This ensures data flows naturally: Source → propagate → Processor → Sink.
    ///
    /// Note: copies port buffers to build input slices for `ProcessContext`.
    /// Production I/O callbacks should call `process_tick` and handle data
    /// propagation themselves for zero-copy processing.
    #[allow(unsafe_code)]
    pub fn process_block(&mut self, tick: &ClockTick) -> ProcessResult<usize> {
        let applied = self.process_tick(tick);

        for &idx in &self.topo_order {
            // Build audio input references:
            // - Zero-copy ports: deref upstream_buffer directly via `Port::upstream_ref`
            //   (no borrow of self.nodes[idx], no lifetime conflict)
            // - Copy-based ports: copy buffer into local storage
            let mut copy_bufs: Vec<[T; BUF_SIZE]> = Vec::new();
            let mut audio_refs: Vec<&[T; BUF_SIZE]> = Vec::new();
            for pi in 0..self.nodes[idx].num_audio_inputs() {
                if let Some(port) = self.nodes[idx].input_port(pi) {
                    match port.upstream_buffer {
                        Some(ptr) => {
                            // SAFETY: the graph is static, single-threaded,
                            // upstream is processed before downstream.
                            let buf = unsafe { Port::upstream_ref(ptr) };
                            audio_refs.push(buf.as_array());
                        }
                        None => {
                            copy_bufs.push(*port.buffer.as_array());
                        }
                    }
                }
            }
            // Extend audio_refs with owned copies (no borrow of self.nodes)
            for buf in &copy_bufs {
                audio_refs.push(buf);
            }

            let owned_control: Vec<T> = (0..self.nodes[idx].num_control_inputs())
                .filter_map(|ci| self.nodes[idx].control_port(ci))
                .map(|p| *p.buffer.as_array().first().unwrap_or(&T::ZERO))
                .collect();

            let owned_clock: Vec<ClockTick> = (0..self.nodes[idx].num_clock_inputs())
                .map(|_| *tick)
                .collect();

            let owned_feedback: Vec<[T; BUF_SIZE]> = (0..self.nodes[idx].num_feedback_ports())
                .filter_map(|fi| self.nodes[idx].input_port(fi))
                .map(|p| *p.buffer.as_array())
                .collect();
            let feedback_refs: Vec<&[T; BUF_SIZE]> = owned_feedback.iter().collect();

            let mut ctx = rill_core::traits::processable::ProcessContext {
                clock: tick,
                audio_inputs: &audio_refs,
                control_inputs: &owned_control,
                clock_inputs: &owned_clock,
                feedback_inputs: &feedback_refs,
            };

            self.nodes[idx].process_block(&mut ctx)?;

            let num_outputs = self.nodes[idx].num_audio_outputs();
            for po in 0..num_outputs {
                if let Some(port) = self.nodes[idx].output_port_mut(po) {
                    port.snapshot_feedback();
                }
            }

            // Propagate outputs to downstream nodes (only for ports
            // without upstream — upstream ports read zero-copy directly)
            for po in 0..num_outputs {
                let (downstream, data) = match self.nodes[idx].output_port(po) {
                    Some(port) if !port.downstream.is_empty() => {
                        (port.downstream.clone(), *port.buffer.as_array())
                    }
                    _ => continue,
                };
                for &(to_n, to_p) in &downstream {
                    if let Some(port) = self.nodes[to_n].input_port_mut(to_p) {
                        if port.upstream_buffer.is_some() {
                            // Zero-copy: skip propagate, node reads directly
                            // from upstream output buffer.
                            continue;
                        }
                        let buf = port.buffer.as_mut_array();
                        *buf = data;
                    }
                }
            }
        }

        Ok(applied)
    }

    /// Access nodes for external processing (I/O layer).
    /// The caller should iterate in `topo_order`.
    pub fn nodes(&self) -> &[NodeVariant<T, BUF_SIZE>] {
        &self.nodes
    }

    /// Mutable access to nodes for external processing.
    pub fn nodes_mut(&mut self) -> &mut [NodeVariant<T, BUF_SIZE>] {
        &mut self.nodes
    }

    /// Topological order — indices into `nodes()`.
    pub fn topo_order(&self) -> &[usize] {
        &self.topo_order
    }

    /// Set the running flag for cooperative thread shutdown.
    pub fn start(&mut self) {
        self.running.store(true, Ordering::SeqCst);
    }

    /// Clear the running flag.
    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }

    /// Check if the engine is running.
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    /// Spawn a dedicated audio thread that runs `process_block` in a loop.
    /// The engine is moved into the thread; communication happens through
    /// command/telemetry queues.
    ///
    /// Returns a handle for joining the thread. Signal shutdown by setting
    /// the running flag to `false` via [`running_flag`](Self::running_flag).
    pub fn spawn(mut self) -> std::thread::JoinHandle<()> {
        let running = self.running.clone();
        running.store(true, Ordering::SeqCst);

        thread::Builder::new()
            .name("rill-audio".into())
            .spawn(move || {
                let mut tick = ClockTick::new(0, BUF_SIZE as u32, 44100.0);
                while running.load(Ordering::SeqCst) {
                    if let Err(e) = self.process_block(&tick) {
                        log::error!("AudioEngine error: {:?}", e);
                        break;
                    }
                    tick.advance(BUF_SIZE as u32);
                }
            })
            .expect("failed to spawn rill-audio thread")
    }

    /// Clone of the running flag, for signaling shutdown from another thread.
    pub fn running_flag(&self) -> Arc<AtomicBool> {
        self.running.clone()
    }

    /// Attach a command receiver after construction.
    pub fn attach_command_rx(&mut self, rx: Receiver<CommandEnum>) {
        self.cmd_rx = Some(rx);
    }

    /// Attach a telemetry sender after construction.
    pub fn attach_telemetry_tx(&mut self, tx: Sender<Telemetry>) {
        self.tel_tx = Some(tx);
    }

    /// Borrow the command slots (for external processing that needs to check
    /// or consume pending commands).
    pub fn cmd_slots(&self) -> &[Option<CommandEnum>] {
        &self.cmd_slots
    }

    /// Mutably borrow the command slots.
    pub fn cmd_slots_mut(&mut self) -> &mut [Option<CommandEnum>] {
        &mut self.cmd_slots
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use rill_core::math::Transcendental;
    use rill_core::queues::signal::{AutomatonCommand, SetParameter, SignalSource};
    use rill_core::queues::CommandQueue;
    use rill_core::traits::{
        AudioNode, NodeCategory, NodeId, NodeMetadata, NodeState, ParamValue, ParameterId, Port,
        PortDirection, PortId, ProcessResult, Processor, Sink, Source,
    };

    // ------------------------------------------------------------------
    // Mock: ConstantSource
    // ------------------------------------------------------------------
    struct ConstantSource<T: Transcendental, const BUF_SIZE: usize> {
        id: NodeId,
        value: T,
        state: NodeState<T, BUF_SIZE>,
        outputs: Vec<Port<T, BUF_SIZE>>,
    }

    impl<T: Transcendental, const BUF_SIZE: usize> ConstantSource<T, BUF_SIZE> {
        fn new(id: NodeId, value: T, sample_rate: f32) -> Self {
            let mut outputs = Vec::with_capacity(1);
            outputs.push(Port {
                id: PortId::audio_out(id, 0),
                name: "output".into(),
                direction: PortDirection::Output,
                action: None,
                pending_command: None,
                buffer: Default::default(),
                feedback_buffer: None,
                downstream: Vec::new(),
                feedback_downstream: Vec::new(),
            upstream_buffer: None,
            });
            Self {
                id,
                value,
                state: NodeState::new(sample_rate),
                outputs,
            }
        }
    }

    impl<T: Transcendental, const BUF_SIZE: usize> AudioNode<T, BUF_SIZE>
        for ConstantSource<T, BUF_SIZE>
    {
        fn metadata(&self) -> NodeMetadata {
            NodeMetadata {
                name: "ConstantSource".into(),
                category: NodeCategory::Source,
                description: String::new(),
                author: String::new(),
                version: "1.0".into(),
                audio_inputs: 0,
                audio_outputs: 1,
                control_inputs: 0,
                control_outputs: 0,
                clock_inputs: 0,
                clock_outputs: 0,
                feedback_ports: 0,
                parameters: vec![],
            }
        }
        fn init(&mut self, _sample_rate: f32) {}
        fn reset(&mut self) {}
        fn get_parameter(&self, _id: &ParameterId) -> Option<ParamValue> {
            None
        }
        fn set_parameter(&mut self, _id: &ParameterId, _value: ParamValue) -> ProcessResult<()> {
            Ok(())
        }
        fn id(&self) -> NodeId {
            self.id
        }
        fn set_id(&mut self, _id: NodeId) {}
        fn num_audio_outputs(&self) -> usize {
            1
        }
        fn input_port(&self, _index: usize) -> Option<&Port<T, BUF_SIZE>> {
            None
        }
        fn input_port_mut(&mut self, _index: usize) -> Option<&mut Port<T, BUF_SIZE>> {
            None
        }
        fn output_port(&self, index: usize) -> Option<&Port<T, BUF_SIZE>> {
            self.outputs.get(index)
        }
        fn output_port_mut(&mut self, index: usize) -> Option<&mut Port<T, BUF_SIZE>> {
            self.outputs.get_mut(index)
        }
        fn control_port(&self, _index: usize) -> Option<&Port<T, BUF_SIZE>> {
            None
        }
        fn control_port_mut(&mut self, _index: usize) -> Option<&mut Port<T, BUF_SIZE>> {
            None
        }
        fn state(&self) -> &NodeState<T, BUF_SIZE> {
            &self.state
        }
        fn state_mut(&mut self) -> &mut NodeState<T, BUF_SIZE> {
            &mut self.state
        }
    }

    impl<T: Transcendental, const BUF_SIZE: usize> Source<T, BUF_SIZE>
        for ConstantSource<T, BUF_SIZE>
    {
        fn generate(
            &mut self,
            _clock: &ClockTick,
            _control_inputs: &[T],
            _clock_inputs: &[ClockTick],
        ) -> ProcessResult<()> {
            let buf = self.outputs[0].buffer.as_mut_array();
            for sample in buf.iter_mut() {
                *sample = self.value;
            }
            Ok(())
        }
    }

    // ------------------------------------------------------------------
    // Mock: NoopProcessor
    // ------------------------------------------------------------------
    struct NoopProcessor<T: Transcendental, const BUF_SIZE: usize> {
        id: NodeId,
        state: NodeState<T, BUF_SIZE>,
        inputs: Vec<Port<T, BUF_SIZE>>,
        outputs: Vec<Port<T, BUF_SIZE>>,
    }

    impl<T: Transcendental, const BUF_SIZE: usize> NoopProcessor<T, BUF_SIZE> {
        fn new(id: NodeId, sample_rate: f32) -> Self {
            let mut inputs = Vec::with_capacity(1);
            inputs.push(Port {
                id: PortId::audio_in(id, 0),
                name: "input".into(),
                direction: PortDirection::Input,
                action: None,
                pending_command: None,
                buffer: Default::default(),
                feedback_buffer: None,
                downstream: Vec::new(),
                feedback_downstream: Vec::new(),
            upstream_buffer: None,
            });
            let mut outputs = Vec::with_capacity(1);
            outputs.push(Port {
                id: PortId::audio_out(id, 0),
                name: "output".into(),
                direction: PortDirection::Output,
                action: None,
                pending_command: None,
                buffer: Default::default(),
                feedback_buffer: None,
                downstream: Vec::new(),
                feedback_downstream: Vec::new(),
            upstream_buffer: None,
            });
            Self {
                id,
                state: NodeState::new(sample_rate),
                inputs,
                outputs,
            }
        }
    }

    impl<T: Transcendental, const BUF_SIZE: usize> AudioNode<T, BUF_SIZE>
        for NoopProcessor<T, BUF_SIZE>
    {
        fn metadata(&self) -> NodeMetadata {
            NodeMetadata {
                name: "NoopProcessor".into(),
                category: NodeCategory::Processor,
                description: String::new(),
                author: String::new(),
                version: "1.0".into(),
                audio_inputs: 1,
                audio_outputs: 1,
                control_inputs: 0,
                control_outputs: 0,
                clock_inputs: 0,
                clock_outputs: 0,
                feedback_ports: 0,
                parameters: vec![],
            }
        }
        fn init(&mut self, _sample_rate: f32) {}
        fn reset(&mut self) {}
        fn get_parameter(&self, _id: &ParameterId) -> Option<ParamValue> {
            None
        }
        fn set_parameter(&mut self, _id: &ParameterId, _value: ParamValue) -> ProcessResult<()> {
            Ok(())
        }
        fn id(&self) -> NodeId {
            self.id
        }
        fn set_id(&mut self, _id: NodeId) {}
        fn num_audio_inputs(&self) -> usize {
            1
        }
        fn num_audio_outputs(&self) -> usize {
            1
        }
        fn input_port(&self, index: usize) -> Option<&Port<T, BUF_SIZE>> {
            self.inputs.get(index)
        }
        fn input_port_mut(&mut self, index: usize) -> Option<&mut Port<T, BUF_SIZE>> {
            self.inputs.get_mut(index)
        }
        fn output_port(&self, index: usize) -> Option<&Port<T, BUF_SIZE>> {
            self.outputs.get(index)
        }
        fn output_port_mut(&mut self, index: usize) -> Option<&mut Port<T, BUF_SIZE>> {
            self.outputs.get_mut(index)
        }
        fn control_port(&self, _index: usize) -> Option<&Port<T, BUF_SIZE>> {
            None
        }
        fn control_port_mut(&mut self, _index: usize) -> Option<&mut Port<T, BUF_SIZE>> {
            None
        }
        fn state(&self) -> &NodeState<T, BUF_SIZE> {
            &self.state
        }
        fn state_mut(&mut self) -> &mut NodeState<T, BUF_SIZE> {
            &mut self.state
        }
    }

    impl<T: Transcendental, const BUF_SIZE: usize> Processor<T, BUF_SIZE>
        for NoopProcessor<T, BUF_SIZE>
    {
        fn process(
            &mut self,
            _clock: &ClockTick,
            audio_inputs: &[&[T; BUF_SIZE]],
            _control_inputs: &[T],
            _clock_inputs: &[ClockTick],
            _feedback_inputs: &[&[T; BUF_SIZE]],
        ) -> ProcessResult<()> {
            let output = self.outputs[0].buffer.as_mut_array();
            if let Some(input) = audio_inputs.first() {
                output.copy_from_slice(*input);
            }
            Ok(())
        }
    }

    // ------------------------------------------------------------------
    // Mock: CaptureSink
    // ------------------------------------------------------------------
    struct CaptureSink<T: Transcendental, const BUF_SIZE: usize> {
        id: NodeId,
        state: NodeState<T, BUF_SIZE>,
        inputs: Vec<Port<T, BUF_SIZE>>,
        captured: Vec<T>,
    }

    impl<T: Transcendental, const BUF_SIZE: usize> CaptureSink<T, BUF_SIZE> {
        fn new(id: NodeId, sample_rate: f32) -> Self {
            let mut inputs = Vec::with_capacity(1);
            inputs.push(Port {
                id: PortId::audio_in(id, 0),
                name: "input".into(),
                direction: PortDirection::Input,
                action: None,
                pending_command: None,
                buffer: Default::default(),
                feedback_buffer: None,
                downstream: Vec::new(),
                feedback_downstream: Vec::new(),
            upstream_buffer: None,
            });
            Self {
                id,
                state: NodeState::new(sample_rate),
                inputs,
                captured: Vec::new(),
            }
        }

        #[allow(dead_code)]
        fn captured(&self) -> &[T] {
            &self.captured
        }
    }

    impl<T: Transcendental, const BUF_SIZE: usize> AudioNode<T, BUF_SIZE>
        for CaptureSink<T, BUF_SIZE>
    {
        fn metadata(&self) -> NodeMetadata {
            NodeMetadata {
                name: "CaptureSink".into(),
                category: NodeCategory::Sink,
                description: String::new(),
                author: String::new(),
                version: "1.0".into(),
                audio_inputs: 1,
                audio_outputs: 0,
                control_inputs: 0,
                control_outputs: 0,
                clock_inputs: 0,
                clock_outputs: 0,
                feedback_ports: 0,
                parameters: vec![],
            }
        }
        fn init(&mut self, _sample_rate: f32) {}
        fn reset(&mut self) {}
        fn get_parameter(&self, _id: &ParameterId) -> Option<ParamValue> {
            None
        }
        fn set_parameter(&mut self, _id: &ParameterId, _value: ParamValue) -> ProcessResult<()> {
            Ok(())
        }
        fn id(&self) -> NodeId {
            self.id
        }
        fn set_id(&mut self, _id: NodeId) {}
        fn num_audio_inputs(&self) -> usize {
            1
        }
        fn input_port(&self, index: usize) -> Option<&Port<T, BUF_SIZE>> {
            self.inputs.get(index)
        }
        fn input_port_mut(&mut self, index: usize) -> Option<&mut Port<T, BUF_SIZE>> {
            self.inputs.get_mut(index)
        }
        fn output_port(&self, _index: usize) -> Option<&Port<T, BUF_SIZE>> {
            None
        }
        fn output_port_mut(&mut self, _index: usize) -> Option<&mut Port<T, BUF_SIZE>> {
            None
        }
        fn control_port(&self, _index: usize) -> Option<&Port<T, BUF_SIZE>> {
            None
        }
        fn control_port_mut(&mut self, _index: usize) -> Option<&mut Port<T, BUF_SIZE>> {
            None
        }
        fn state(&self) -> &NodeState<T, BUF_SIZE> {
            &self.state
        }
        fn state_mut(&mut self) -> &mut NodeState<T, BUF_SIZE> {
            &mut self.state
        }
    }

    impl<T: Transcendental, const BUF_SIZE: usize> Sink<T, BUF_SIZE>
        for CaptureSink<T, BUF_SIZE>
    {
        fn consume(
            &mut self,
            _clock: &ClockTick,
            audio_inputs: &[&[T; BUF_SIZE]],
            _control_inputs: &[T],
            _clock_inputs: &[ClockTick],
            _feedback_inputs: &[&[T; BUF_SIZE]],
        ) -> ProcessResult<()> {
            if let Some(input) = audio_inputs.first() {
                self.captured = input.to_vec();
            }
            Ok(())
        }
    }

    // ==================================================================
    // Tests
    // ==================================================================

    #[test]
    fn test_engine_process_tick_drains_commands() {
        const BUF: usize = 64;
        let cmd_queue = CommandQueue::<CommandEnum>::new("test", 16);
        let cmd_rx = cmd_queue.receiver();
        let tick = ClockTick::new(0, BUF as u32, 44100.0);

        let nodes: Vec<NodeVariant<f32, BUF>> = vec![NodeVariant::Source(Box::new(
            ConstantSource::new(NodeId(0), 1.0, 44100.0),
        ))];

        let mut engine = AudioEngine::<f32, BUF>::new(nodes, vec![0], Some(cmd_rx), None);

        let cmd = CommandEnum::SetParameter(SetParameter::new(
            PortId::param(NodeId(0), 0),
            ParameterId::new("gain").unwrap(),
            0.5,
            SignalSource::Manual,
        ));
        cmd_queue.send(cmd).unwrap();

        let applied = engine.process_tick(&tick);
        assert_eq!(applied, 1);
    }

    #[test]
    fn test_engine_process_tick_anti_ack() {
        const BUF: usize = 64;
        let cmd_queue = CommandQueue::<CommandEnum>::new("test", 16);
        let cmd_rx = cmd_queue.receiver();
        let (tel_tx, tel_rx) = crossbeam_channel::unbounded();
        let tick = ClockTick::new(0, BUF as u32, 44100.0);

        let nodes: Vec<NodeVariant<f32, BUF>> = vec![NodeVariant::Source(Box::new(
            ConstantSource::new(NodeId(0), 1.0, 44100.0),
        ))];

        let mut engine = AudioEngine::<f32, BUF>::new(
            nodes,
            vec![0],
            Some(cmd_rx),
            Some(tel_tx),
        );

        let pid = ParameterId::new("gain").unwrap();
        let cmd1 = CommandEnum::SetParameter(SetParameter::new(
            PortId::param(NodeId(0), 0),
            pid.clone(),
            0.3,
            SignalSource::Manual,
        ));
        let cmd2 = CommandEnum::SetParameter(SetParameter::new(
            PortId::param(NodeId(0), 0),
            pid,
            0.8,
            SignalSource::Manual,
        ));
        cmd_queue.send(cmd1).unwrap();
        cmd_queue.send(cmd2).unwrap();

        let applied = engine.process_tick(&tick);

        // First command was overwritten (anti-ack), second applied
        assert_eq!(applied, 1);

        let tel = tel_rx.try_recv().unwrap();
        match tel {
            Telemetry::Event { kind, data, .. } => {
                assert_eq!(kind, "command_dropped");
                assert_eq!(data, vec![0.0]);
            }
            _ => panic!("expected Event telemetry"),
        }
    }

    #[test]
    fn test_engine_process_tick_skips_non_set_parameter() {
        const BUF: usize = 64;
        let cmd_queue = CommandQueue::<CommandEnum>::new("test", 16);
        let cmd_rx = cmd_queue.receiver();
        let tick = ClockTick::new(0, BUF as u32, 44100.0);

        let nodes: Vec<NodeVariant<f32, BUF>> = vec![NodeVariant::Source(Box::new(
            ConstantSource::new(NodeId(0), 1.0, 44100.0),
        ))];

        let mut engine = AudioEngine::<f32, BUF>::new(nodes, vec![0], Some(cmd_rx), None);

        let cmd = CommandEnum::Automaton(AutomatonCommand::SetEnabled {
            id: "test".into(),
            enabled: true,
        });
        cmd_queue.send(cmd).unwrap();

        let applied = engine.process_tick(&tick);
        assert_eq!(applied, 0);
    }

    #[test]
    fn test_engine_process_block_is_convenience_method() {
        const BUF: usize = 64;
        let tick = ClockTick::new(0, BUF as u32, 44100.0);

        let nodes: Vec<NodeVariant<f32, BUF>> = vec![NodeVariant::Source(Box::new(
            ConstantSource::new(NodeId(0), 1.0, 44100.0),
        ))];

        let mut engine = AudioEngine::<f32, BUF>::new(nodes, vec![0], None, None);

        let result = engine.process_block(&tick).unwrap();
        assert_eq!(result, 0);
    }

    #[test]
    fn test_engine_data_flows_source_to_sink() {
        const BUF: usize = 64;

        // Source → Processor → Sink with manual wiring
        let mut nodes: Vec<NodeVariant<f32, BUF>> = Vec::new();

        let mut source = Box::new(ConstantSource::new(NodeId(0), 42.0, 44100.0));
        source.outputs[0].downstream.push((1, 0));
        nodes.push(NodeVariant::Source(source));

        let mut processor = Box::new(NoopProcessor::new(NodeId(1), 44100.0));
        processor.outputs[0].downstream.push((2, 0));
        nodes.push(NodeVariant::Processor(processor));

        let sink_node = Box::new(CaptureSink::new(NodeId(2), 44100.0));
        nodes.push(NodeVariant::Sink(sink_node));

        let tick = ClockTick::new(0, BUF as u32, 44100.0);
        let mut engine = AudioEngine::<f32, BUF>::new(nodes, vec![0, 1, 2], None, None);

        engine.process_block(&tick).unwrap();

        // Check data flowed through Source → Processor → Sink
        let sink_port = engine.nodes[2].input_port(0).expect("sink input port");
        let buf = sink_port.buffer.as_array();
        assert!(buf.iter().all(|&x| x == 42.0),
            "sink input should be all 42.0, got {:?}", &buf[..5]);
    }

    // ------------------------------------------------------------------
    // ADC Source — simulates hardware ADC by filling output with a
    // block counter. Each block's first sample = block_index.
    // ------------------------------------------------------------------
    struct AdcSource<T: Transcendental, const BUF_SIZE: usize> {
        id: NodeId,
        block_count: u64,
        state: NodeState<T, BUF_SIZE>,
        outputs: Vec<Port<T, BUF_SIZE>>,
    }

    impl<T: Transcendental, const BUF_SIZE: usize> AdcSource<T, BUF_SIZE> {
        fn new(id: NodeId, sample_rate: f32) -> Self {
            let mut outputs = Vec::with_capacity(1);
            outputs.push(Port {
                id: PortId::audio_out(id, 0),
                name: "adc_out".into(),
                direction: PortDirection::Output,
                action: None,
                pending_command: None,
                buffer: Default::default(),
                feedback_buffer: None,
                downstream: Vec::new(),
                feedback_downstream: Vec::new(),
            upstream_buffer: None,
            });
            Self {
                id,
                block_count: 0,
                state: NodeState::new(sample_rate),
                outputs,
            }
        }
    }

    impl<T: Transcendental, const BUF_SIZE: usize> AudioNode<T, BUF_SIZE>
        for AdcSource<T, BUF_SIZE>
    {
        fn metadata(&self) -> NodeMetadata {
            NodeMetadata {
                name: "AdcSource".into(),
                category: NodeCategory::Source,
                description: String::new(),
                author: String::new(),
                version: "1.0".into(),
                audio_inputs: 0,
                audio_outputs: 1,
                control_inputs: 0,
                control_outputs: 0,
                clock_inputs: 0,
                clock_outputs: 0,
                feedback_ports: 0,
                parameters: vec![],
            }
        }
        fn init(&mut self, _sample_rate: f32) {}
        fn reset(&mut self) { self.block_count = 0; }
        fn get_parameter(&self, _id: &ParameterId) -> Option<ParamValue> { None }
        fn set_parameter(&mut self, _id: &ParameterId, _value: ParamValue) -> ProcessResult<()> { Ok(()) }
        fn id(&self) -> NodeId { self.id }
        fn set_id(&mut self, _id: NodeId) {}
        fn num_audio_outputs(&self) -> usize { 1 }
        fn output_port(&self, index: usize) -> Option<&Port<T, BUF_SIZE>> { self.outputs.get(index) }
        fn output_port_mut(&mut self, index: usize) -> Option<&mut Port<T, BUF_SIZE>> { self.outputs.get_mut(index) }
        fn input_port(&self, _index: usize) -> Option<&Port<T, BUF_SIZE>> { None }
        fn input_port_mut(&mut self, _index: usize) -> Option<&mut Port<T, BUF_SIZE>> { None }
        fn control_port(&self, _index: usize) -> Option<&Port<T, BUF_SIZE>> { None }
        fn control_port_mut(&mut self, _index: usize) -> Option<&mut Port<T, BUF_SIZE>> { None }
        fn state(&self) -> &NodeState<T, BUF_SIZE> { &self.state }
        fn state_mut(&mut self) -> &mut NodeState<T, BUF_SIZE> { &mut self.state }
    }

    impl<T: Transcendental, const BUF_SIZE: usize> Source<T, BUF_SIZE>
        for AdcSource<T, BUF_SIZE>
    {
        fn generate(
            &mut self,
            _clock: &ClockTick,
            _control_inputs: &[T],
            _clock_inputs: &[ClockTick],
        ) -> ProcessResult<()> {
            let buf = self.outputs[0].buffer.as_mut_array();
            let count = self.block_count;
            for (i, sample) in buf.iter_mut().enumerate() {
                *sample = T::from_f32(count as f32) + T::from_f32(i as f32);
            }
            self.block_count += 1;
            Ok(())
        }
    }

    // ------------------------------------------------------------------
    // DAC Sink — simulates hardware DAC by capturing the last
    // `capture_depth` blocks into a shared buffer.
    // ------------------------------------------------------------------
    struct DacSink<T: Transcendental, const BUF_SIZE: usize> {
        id: NodeId,
        state: NodeState<T, BUF_SIZE>,
        inputs: Vec<Port<T, BUF_SIZE>>,
        captured: Vec<T>,
    }

    impl<T: Transcendental, const BUF_SIZE: usize> DacSink<T, BUF_SIZE> {
        fn new(id: NodeId, sample_rate: f32) -> Self {
            let mut inputs = Vec::with_capacity(1);
            inputs.push(Port {
                id: PortId::audio_in(id, 0),
                name: "dac_in".into(),
                direction: PortDirection::Input,
                action: None,
                pending_command: None,
                buffer: Default::default(),
                feedback_buffer: None,
                downstream: Vec::new(),
                feedback_downstream: Vec::new(),
            upstream_buffer: None,
            });
            Self {
                id,
                state: NodeState::new(sample_rate),
                inputs,
                captured: Vec::new(),
            }
        }
        fn captured(&self) -> &[T] { &self.captured }
    }

    impl<T: Transcendental, const BUF_SIZE: usize> AudioNode<T, BUF_SIZE>
        for DacSink<T, BUF_SIZE>
    {
        fn metadata(&self) -> NodeMetadata {
            NodeMetadata {
                name: "DacSink".into(),
                category: NodeCategory::Sink,
                description: String::new(),
                author: String::new(),
                version: "1.0".into(),
                audio_inputs: 1,
                audio_outputs: 0,
                control_inputs: 0,
                control_outputs: 0,
                clock_inputs: 0,
                clock_outputs: 0,
                feedback_ports: 0,
                parameters: vec![],
            }
        }
        fn init(&mut self, _sample_rate: f32) {}
        fn reset(&mut self) { self.captured.clear(); }
        fn get_parameter(&self, _id: &ParameterId) -> Option<ParamValue> { None }
        fn set_parameter(&mut self, _id: &ParameterId, _value: ParamValue) -> ProcessResult<()> { Ok(()) }
        fn id(&self) -> NodeId { self.id }
        fn set_id(&mut self, _id: NodeId) {}
        fn num_audio_inputs(&self) -> usize { 1 }
        fn input_port(&self, index: usize) -> Option<&Port<T, BUF_SIZE>> { self.inputs.get(index) }
        fn input_port_mut(&mut self, index: usize) -> Option<&mut Port<T, BUF_SIZE>> { self.inputs.get_mut(index) }
        fn output_port(&self, _index: usize) -> Option<&Port<T, BUF_SIZE>> { None }
        fn output_port_mut(&mut self, _index: usize) -> Option<&mut Port<T, BUF_SIZE>> { None }
        fn control_port(&self, _index: usize) -> Option<&Port<T, BUF_SIZE>> { None }
        fn control_port_mut(&mut self, _index: usize) -> Option<&mut Port<T, BUF_SIZE>> { None }
        fn state(&self) -> &NodeState<T, BUF_SIZE> { &self.state }
        fn state_mut(&mut self) -> &mut NodeState<T, BUF_SIZE> { &mut self.state }
    }

    impl<T: Transcendental, const BUF_SIZE: usize> Sink<T, BUF_SIZE>
        for DacSink<T, BUF_SIZE>
    {
        fn consume(
            &mut self,
            _clock: &ClockTick,
            audio_inputs: &[&[T; BUF_SIZE]],
            _control_inputs: &[T],
            _clock_inputs: &[ClockTick],
            _feedback_inputs: &[&[T; BUF_SIZE]],
        ) -> ProcessResult<()> {
            if let Some(input) = audio_inputs.first() {
                self.captured.extend_from_slice(*input);
            }
            Ok(())
        }
    }

    // ==================================================================
    // Hardware clock simulation test
    //
    // Simulates a real ADC → DSP → DAC chain where the hardware clock
    // fires at regular intervals (block_size / sample_rate seconds).
    // Each call to process_block = one hardware clock tick.
    // No sleeps — pure deterministic processing loop.
    // ==================================================================

    #[test]
    fn test_engine_hardware_clock_simulation() {
        const BUF: usize = 64;
        const NUM_BLOCKS: usize = 10;

        // Build ADC → NoopProcessor → DAC chain
        let mut nodes: Vec<NodeVariant<f32, BUF>> = Vec::new();

        let mut adc = Box::new(AdcSource::new(NodeId(0), 44100.0));
        adc.outputs[0].downstream.push((1, 0));
        nodes.push(NodeVariant::Source(adc));

        let mut proc = Box::new(NoopProcessor::new(NodeId(1), 44100.0));
        proc.outputs[0].downstream.push((2, 0));
        nodes.push(NodeVariant::Processor(proc));

        let dac = Box::new(DacSink::new(NodeId(2), 44100.0));
        nodes.push(NodeVariant::Sink(dac));

        let mut tick = ClockTick::new(0, BUF as u32, 44100.0);
        let mut engine = AudioEngine::<f32, BUF>::new(nodes, vec![0, 1, 2], None, None);

        // Process N blocks — each call is one hardware clock tick
        for _ in 0..NUM_BLOCKS {
            engine.process_block(&tick).unwrap();
            tick.advance(BUF as u32);
        }

        // Verify last block propagated to DAC sink
        let dac_port = engine.nodes[2].input_port(0).expect("dac input");
        let last_block = dac_port.buffer.as_array();

        // Block 9 (0-indexed): ADC writes block_count to first sample,
        // then block_count + sample_index for subsequent samples
        assert_eq!(last_block[0], (NUM_BLOCKS - 1) as f32,
            "first sample of block {} should be {}", NUM_BLOCKS - 1, NUM_BLOCKS - 1);
        assert_eq!(last_block[BUF - 1], (NUM_BLOCKS - 1 + BUF - 1) as f32);
    }

    // ==================================================================
    // Pull model test — active Sink, passive Source.
    //
    // In the pull model, the hardware DAC clock drives processing:
    // the Sink receives the clock tick and data flows from Source
    // through the graph to the Sink.
    //
    // `process_tick` handles the clock boundary (feedback, commands),
    // then `process_block` runs the topo-order processing.
    // The Sink is semantically "active" — it pulls data by virtue
    // of the clock reaching it through the graph.
    // ==================================================================

    #[test]
    fn test_engine_pull_model_active_sink() {
        const BUF: usize = 64;
        const NUM_BLOCKS: usize = 5;

        // Passive Source — generates on each tick
        let mut nodes: Vec<NodeVariant<f32, BUF>> = Vec::new();

        let mut src = Box::new(ConstantSource::new(NodeId(0), 1.0, 44100.0));
        src.outputs[0].downstream.push((1, 0));
        nodes.push(NodeVariant::Source(src));

        let mut proc = Box::new(NoopProcessor::new(NodeId(1), 44100.0));
        proc.outputs[0].downstream.push((2, 0));
        nodes.push(NodeVariant::Processor(proc));

        // Active Sink — simulates DAC pulling data
        let dac = Box::new(DacSink::new(NodeId(2), 44100.0));
        nodes.push(NodeVariant::Sink(dac));

        let mut tick = ClockTick::new(0, BUF as u32, 44100.0);
        let mut engine = AudioEngine::<f32, BUF>::new(nodes, vec![0, 1, 2], None, None);

        // The clock fires — Sink is the active node in the pull model
        for _ in 0..NUM_BLOCKS {
            engine.process_block(&tick).unwrap();
            tick.advance(BUF as u32);
        }

        // Verify data reached the Sink — constant 1.0 from source
        let dac_port = engine.nodes[2].input_port(0).expect("dac input");
        let last_block = dac_port.buffer.as_array();
        assert!(last_block.iter().all(|&x| x == 1.0),
            "pull model: sink should receive 1.0, got {:?}", &last_block[..3]);
    }
}
