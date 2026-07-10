//! Mixer node implementation

use super::channel::{ChannelConfig, ChannelState};
use super::send::{SendConfig, SendType};
use rill_core::traits::{ParamMetadata, ParamRange, ParamType, ParamValue, ParameterId};
use rill_core::RenderContext;
use rill_core::{ProcessError, ProcessResult};
use std::collections::HashMap;

/// Mixer node with multiple channels and aux sends
pub struct MixerNode<const BUF_SIZE: usize> {
    /// Master volume (0.0 - 2.0)
    pub master_volume: f32,
    /// Smoothing factor (0.0 - 1.0)
    pub smoothing: f32,
    /// Channels
    pub channels: Vec<ChannelState>,
    /// Channel names for parameter lookup
    pub channel_names: HashMap<String, usize>,
    /// Aux buses (each bus accumulates signals from sends)
    pub buses: Vec<Vec<f32>>,
    /// Send configurations per channel
    pub sends: Vec<Vec<SendConfig>>,
    /// Current master volume with smoothing
    pub current_master_volume: f32,
    /// Buffer size for buses (updated each block)
    pub buffer_size: usize,
    /// Sample rate
    pub sample_rate: f32,
    /// Control input values (updated from graph)
    pub control_values: Vec<f32>,
    /// Parameter IDs for automation
    pub param_ids: HashMap<String, ParameterId>,
    /// Optional hook called after a parameter changes
    pub after_param_change_closure: fn(&mut Self, &str, f32),
    /// Node ID
    // (removed legacy field)
    /// Audio input ports
    pub input_ports: Vec<Port<f32, BUF_SIZE>>,
    /// Audio output ports
    pub output_ports: Vec<Port<f32, BUF_SIZE>>,
    /// Control ports
    pub control_ports: Vec<Port<f32, BUF_SIZE>>,
    /// Node state
    // (removed legacy field)
}

impl<const BUF_SIZE: usize> MixerNode<BUF_SIZE> {
    /// Create a new mixer with specified number of channels and buses
    pub fn new(num_channels: usize, num_buses: usize) -> Self {
        let mut channels = Vec::with_capacity(num_channels);
        let mut channel_names = HashMap::new();
        let mut sends = Vec::with_capacity(num_channels);

        for i in 0..num_channels {
            let config = ChannelConfig {
                name: format!("Channel {}", i + 1),
                ..Default::default()
            };
            channel_names.insert(config.name.clone(), i);
            channels.push(ChannelState::new(config));
            sends.push(Vec::new()); // no sends initially
        }

        let mut input_ports = Vec::with_capacity(num_channels);
        for i in 0..num_channels {
            input_ports.push(Port::input(
                NodeId::new(0),
                i as u16,
                &format!("ch{}_in", i + 1),
            ));
        }

        let mut output_ports = Vec::with_capacity(2 + num_buses);
        output_ports.push(Port::output(NodeId::new(0), 0, "master_left"));
        output_ports.push(Port::output(NodeId::new(0), 1, "master_right"));
        for bus_idx in 0..num_buses {
            output_ports.push(Port::output(
                NodeId::new(0),
                (2 + bus_idx) as u16,
                &format!("bus{}_out", bus_idx + 1),
            ));
        }

        Self {
            master_volume: 1.0,
            smoothing: 0.1,
            channels,
            channel_names,
            buses: vec![Vec::new(); num_buses],
            sends,
            current_master_volume: 1.0,
            buffer_size: 0,
            sample_rate: 44100.0,
            control_values: Vec::new(),
            param_ids: HashMap::new(),
            after_param_change_closure: |_, _, _| {},
    // (removed legacy field)
            input_ports,
            output_ports,
            control_ports: Vec::new(),
    // (removed legacy field)
        }
    }

    /// Number of audio inputs (channels)
    pub fn num_inputs(&self) -> usize {
        self.num_signal_inputs()
    }

    /// Number of audio outputs (master L/R + buses)
    pub fn num_outputs(&self) -> usize {
        self.num_signal_outputs()
    }

    /// Get parameter value by name (convenience wrapper)
    pub fn get_param(&self, name: &str) -> Option<ParamValue> {
        let id = ParameterId::new(name).ok()?;
        self.get_parameter(&id)
    }

    /// Set parameter value by name (convenience wrapper)
    pub fn set_param(&mut self, name: &str, value: ParamValue) -> ProcessResult<()> {
        let id = ParameterId::new(name)
            .map_err(|e| rill_core::ProcessError::Parameter(e.to_string()))?;
        self.set_parameter(&id, value)
    }

    /// Add a channel
    pub fn add_channel(&mut self, config: ChannelConfig) -> usize {
        let index = self.channels.len();
        self.channel_names.insert(config.name.clone(), index);
        self.channels.push(ChannelState::new(config));
        self.sends.push(Vec::new());
        self.input_ports.push(Port::input(
            NodeId::new(0),
            index as u16,
            &format!("ch{}_in", index + 1),
        ));
        index
    }

    /// Remove a channel by index
    pub fn remove_channel(&mut self, index: usize) -> Result<(), ProcessError> {
        if index >= self.channels.len() {
            return Err(ProcessError::Parameter("Channel index out of range".into()));
        }
        let name = self.channels[index].config().name.clone();
        self.channel_names.remove(&name);
        self.channels.remove(index);
        self.sends.remove(index);
        self.input_ports.remove(index);
        Ok(())
    }

    /// Add a send from a channel to a bus
    pub fn add_send(&mut self, channel_index: usize, send: SendConfig) -> Result<(), ProcessError> {
        if channel_index >= self.sends.len() {
            return Err(ProcessError::Parameter("Channel index out of range".into()));
        }
        if send.bus_index >= self.buses.len() {
            return Err(ProcessError::Parameter("Bus index out of range".into()));
        }
        self.sends[channel_index].push(send);
        Ok(())
    }

    /// Clear sends for a channel
    pub fn clear_sends(&mut self, channel_index: usize) -> Result<(), ProcessError> {
        if channel_index >= self.sends.len() {
            return Err(ProcessError::Parameter("Channel index out of range".into()));
        }
        self.sends[channel_index].clear();
        Ok(())
    }

    /// Set channel volume
    pub fn set_channel_volume(
        &mut self,
        channel_index: usize,
        volume: f32,
    ) -> Result<(), ProcessError> {
        if channel_index >= self.channels.len() {
            return Err(ProcessError::Parameter("Channel index out of range".into()));
        }
        let mut config = self.channels[channel_index].config().clone();
        config.volume = volume.clamp(0.0, 1.0);
        self.channels[channel_index].set_config(config);
        Ok(())
    }

    /// Set channel pan
    pub fn set_channel_pan(&mut self, channel_index: usize, pan: f32) -> Result<(), ProcessError> {
        if channel_index >= self.channels.len() {
            return Err(ProcessError::Parameter("Channel index out of range".into()));
        }
        let mut config = self.channels[channel_index].config().clone();
        config.pan = pan.clamp(-1.0, 1.0);
        self.channels[channel_index].set_config(config);
        Ok(())
    }

    /// Set channel mute
    pub fn set_channel_mute(
        &mut self,
        channel_index: usize,
        mute: bool,
    ) -> Result<(), ProcessError> {
        if channel_index >= self.channels.len() {
            return Err(ProcessError::Parameter("Channel index out of range".into()));
        }
        let mut config = self.channels[channel_index].config().clone();
        config.muted = mute;
        self.channels[channel_index].set_config(config);
        Ok(())
    }

    /// Set master volume
    pub fn set_master_volume(&mut self, volume: f32) {
        self.master_volume = volume.clamp(0.0, 2.0);
    }

    /// Set smoothing factor
    pub fn set_smoothing(&mut self, factor: f32) {
        self.smoothing = factor.clamp(0.0, 1.0);
        for channel in &mut self.channels {
            channel.set_smoothing(factor);
        }
    }
}


    where
        Self: 'static + Sized,
    {
        NodeTypeId::of::<Self>()
    }

    fn init(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.state.sample_rate = sample_rate;
    }

    fn reset(&mut self) {
        self.current_master_volume = self.master_volume;
        self.state.reset();
        for channel in &mut self.channels {
            channel.set_smoothing(self.smoothing);
        }
    }

    fn get_parameter(&self, id: &ParameterId) -> Option<ParamValue> {
        let name = id.as_str();
        if name == "master_volume" {
            return Some(ParamValue::Float(self.master_volume));
        }
        if name.starts_with("ch_") {
            let parts: Vec<&str> = name.split('_').collect();
            if parts.len() >= 3 {
                if let Ok(idx) = parts[1].parse::<usize>() {
                    if idx > 0 && idx <= self.channels.len() {
                        let channel = &self.channels[idx - 1];
                        match parts[2] {
                            "volume" => return Some(ParamValue::Float(channel.config().volume)),
                            "pan" => return Some(ParamValue::Float(channel.config().pan)),
                            "mute" => return Some(ParamValue::Bool(channel.config().muted)),
                            _ => {}
                        }
                    }
                }
            }
        }
        if name == "smoothing" {
            return Some(ParamValue::Float(self.smoothing));
        }
        None
    }

    fn set_parameter(&mut self, id: &ParameterId, value: ParamValue) -> ProcessResult<()> {
        let name = id.as_str();
        if name == "master_volume" {
            if let ParamValue::Float(v) = value {
                self.set_master_volume(v);
                return Ok(());
            }
        }
        if name == "smoothing" {
            if let ParamValue::Float(v) = value {
                self.set_smoothing(v);
                return Ok(());
            }
        }
        if name.starts_with("ch_") {
            let parts: Vec<&str> = name.split('_').collect();
            if parts.len() >= 3 {
                if let Ok(idx) = parts[1].parse::<usize>() {
                    if idx > 0 && idx <= self.channels.len() {
                        match parts[2] {
                            "volume" => {
                                if let ParamValue::Float(v) = value {
                                    return self.set_channel_volume(idx - 1, v).map_err(|e| {
                                        rill_core::ProcessError::Parameter(e.to_string())
                                    });
                                }
                            }
                            "pan" => {
                                if let ParamValue::Float(v) = value {
                                    return self.set_channel_pan(idx - 1, v).map_err(|e| {
                                        rill_core::ProcessError::Parameter(e.to_string())
                                    });
                                }
                            }
                            "mute" => {
                                if let ParamValue::Bool(v) = value {
                                    return self.set_channel_mute(idx - 1, v).map_err(|e| {
                                        rill_core::ProcessError::Parameter(e.to_string())
                                    });
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
        Err(rill_core::ProcessError::Parameter(format!(
            "Unknown parameter: {}",
            name
        )))
    }

    fn id(&self) -> NodeId {
        self.id
    }

    fn set_id(&mut self, id: NodeId) {
        self.id = id;
    }

    fn input_port(&self, index: usize) -> Option<&Port<f32, BUF_SIZE>> {
        self.input_ports.get(index)
    }

    fn input_port_mut(&mut self, index: usize) -> Option<&mut Port<f32, BUF_SIZE>> {
        self.input_ports.get_mut(index)
    }

    fn output_port(&self, index: usize) -> Option<&Port<f32, BUF_SIZE>> {
        self.output_ports.get(index)
    }

    fn output_port_mut(&mut self, index: usize) -> Option<&mut Port<f32, BUF_SIZE>> {
        self.output_ports.get_mut(index)
    }

    fn control_port(&self, index: usize) -> Option<&Port<f32, BUF_SIZE>> {
        self.control_ports.get(index)
    }

    fn control_port_mut(&mut self, index: usize) -> Option<&mut Port<f32, BUF_SIZE>> {
        self.control_ports.get_mut(index)
    }

        &self.state
    }

        &mut self.state
    }

    fn num_signal_inputs(&self) -> usize {
        self.channels.len()
    }

    fn num_signal_outputs(&self) -> usize {
        2 + self.buses.len()
    }

    fn num_control_inputs(&self) -> usize {
        0
    }

    fn num_control_outputs(&self) -> usize {
        0
    }

    fn num_clock_inputs(&self) -> usize {
        0
    }

    fn num_clock_outputs(&self) -> usize {
        0
    }

    fn num_feedback_ports(&self) -> usize {
        0
    }
}

// ── Router trait — N→M configurable routing ────────────────

    fn num_route_inputs(&self) -> usize {
        self.channels.len()
    }

    fn num_route_outputs(&self) -> usize {
        2 + self.buses.len()
    }

    fn set_connection(&mut self, from: usize, to: usize, gain: f32) -> ProcessResult<()> {
        // For the mixer, "connection" means routing channel `from` to output `to`.
        // Channel volume controls the gain to master L/R.
        // Bus sends are managed via add_send().
        if from >= self.channels.len() {
            return Err(ProcessError::Parameter("Channel index out of range".into()));
        }
        if to == 0 || to == 1 {
            // Master L/R: set channel volume (pan is unchanged)
            self.set_channel_volume(from, gain.clamp(0.0, 1.0))
        } else if to >= 2 && to < 2 + self.buses.len() {
            // Aux bus: add/update a send
            let bus_idx = to - 2;
            // Check if a send to this bus already exists
            if let Some(existing) = self.sends[from].iter_mut().find(|s| s.bus_index == bus_idx) {
                existing.level = gain.clamp(0.0, 1.0);
                Ok(())
            } else {
                self.add_send(
                    from,
                    SendConfig {
                        bus_index: bus_idx,
                        level: gain.clamp(0.0, 1.0),
                        send_type: SendType::PostFader,
                    },
                )
            }
        } else {
            Err(ProcessError::Parameter("Output index out of range".into()))
        }
    }

    fn remove_connection(&mut self, from: usize, to: usize) -> ProcessResult<()> {
        if from >= self.channels.len() {
            return Err(ProcessError::Parameter("Channel index out of range".into()));
        }
        if to == 0 || to == 1 {
            // Master L/R: mute the channel
            self.set_channel_mute(from, true)
        } else if to >= 2 && to < 2 + self.buses.len() {
            // Remove the send to this bus
            let bus_idx = to - 2;
            self.sends[from].retain(|s| s.bus_index != bus_idx);
            Ok(())
        } else {
            Err(ProcessError::Parameter("Output index out of range".into()))
        }
    }

    fn routing_matrix(&self) -> Vec<Vec<(usize, f32)>> {
        let n_out = self.num_route_outputs();
        let mut matrix = vec![Vec::new(); n_out];

        // Master L (0): sum of all channels with their volumes
        // Master R (1): same
        for (ch_idx, ch) in self.channels.iter().enumerate() {
            if !ch.config().muted {
                matrix[0].push((ch_idx, ch.config().volume));
                matrix[1].push((ch_idx, ch.config().volume));
            }
        }

        // Buses: send connections
        for (ch_idx, ch_sends) in self.sends.iter().enumerate() {
            for send in ch_sends {
                let out_idx = 2 + send.bus_index;
                if out_idx < n_out {
                    matrix[out_idx].push((ch_idx, send.level));
                }
            }
        }

        matrix
    }
}
