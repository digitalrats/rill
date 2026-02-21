//! Mixer node implementation

use std::collections::HashMap;
use kama_core_traits::{
    AudioNode, AudioError, ParamValue, NodeMetadata, NodeCategory, NodeTypeId,
    param::{ParamType, ParamMetadata}
};
use crate::channel::{ChannelConfig, ChannelMode, ChannelState};
use crate::send::{SendConfig, SendType};

/// Mixer node with multiple channels and aux sends
pub struct MixerNode {
    /// Channels
    channels: Vec<ChannelState>,
    /// Channel names for parameter lookup
    channel_names: HashMap<String, usize>,
    /// Aux buses (each bus accumulates signals from sends)
    buses: Vec<Vec<f32>>,
    /// Send configurations per channel
    sends: Vec<Vec<SendConfig>>,
    /// Master volume
    master_volume: f32,
    /// Current master volume with smoothing
    current_master_volume: f32,
    /// Smoothing factor
    smoothing: f32,
    /// Sample rate
    sample_rate: f32,
    /// Buffer size for buses
    buffer_size: usize,
}

impl MixerNode {
    /// Create a new mixer with specified number of channels and buses
    pub fn new(num_channels: usize, num_buses: usize) -> Self {
        let mut channels = Vec::with_capacity(num_channels);
        let mut channel_names = HashMap::new();
        let mut sends = Vec::with_capacity(num_channels);
        
        for i in 0..num_channels {
            let config = ChannelConfig {
                name: format!("Channel {}", i+1),
                ..Default::default()
            };
            channel_names.insert(config.name.clone(), i);
            channels.push(ChannelState::new(config));
            sends.push(Vec::new()); // no sends initially
        }
        
        Self {
            channels,
            channel_names,
            buses: vec![Vec::new(); num_buses],
            sends,
            master_volume: 1.0,
            current_master_volume: 1.0,
            smoothing: 0.1,
            sample_rate: 44100.0,
            buffer_size: 0,
        }
    }
    
    /// Add a channel
    pub fn add_channel(&mut self, config: ChannelConfig) -> usize {
        let index = self.channels.len();
        self.channel_names.insert(config.name.clone(), index);
        self.channels.push(ChannelState::new(config));
        self.sends.push(Vec::new());
        index
    }
    
    /// Remove a channel by index
    pub fn remove_channel(&mut self, index: usize) -> Result<(), AudioError> {
        if index >= self.channels.len() {
            return Err(AudioError::Parameter("Channel index out of range".into()));
        }
        let name = self.channels[index].config().name.clone();
        self.channel_names.remove(&name);
        self.channels.remove(index);
        self.sends.remove(index);
        Ok(())
    }
    
    /// Add a send from a channel to a bus
    pub fn add_send(&mut self, channel_index: usize, send: SendConfig) -> Result<(), AudioError> {
        if channel_index >= self.sends.len() {
            return Err(AudioError::Parameter("Channel index out of range".into()));
        }
        if send.bus_index >= self.buses.len() {
            return Err(AudioError::Parameter("Bus index out of range".into()));
        }
        self.sends[channel_index].push(send);
        Ok(())
    }
    
    /// Clear sends for a channel
    pub fn clear_sends(&mut self, channel_index: usize) -> Result<(), AudioError> {
        if channel_index >= self.sends.len() {
            return Err(AudioError::Parameter("Channel index out of range".into()));
        }
        self.sends[channel_index].clear();
        Ok(())
    }
    
    /// Set channel volume
    pub fn set_channel_volume(&mut self, channel_index: usize, volume: f32) -> Result<(), AudioError> {
        if channel_index >= self.channels.len() {
            return Err(AudioError::Parameter("Channel index out of range".into()));
        }
        let mut config = self.channels[channel_index].config().clone();
        config.volume = volume.clamp(0.0, 1.0);
        self.channels[channel_index].set_config(config);
        Ok(())
    }
    
    /// Set channel pan
    pub fn set_channel_pan(&mut self, channel_index: usize, pan: f32) -> Result<(), AudioError> {
        if channel_index >= self.channels.len() {
            return Err(AudioError::Parameter("Channel index out of range".into()));
        }
        let mut config = self.channels[channel_index].config().clone();
        config.pan = pan.clamp(-1.0, 1.0);
        self.channels[channel_index].set_config(config);
        Ok(())
    }
    
    /// Set channel mute
    pub fn set_channel_mute(&mut self, channel_index: usize, mute: bool) -> Result<(), AudioError> {
        if channel_index >= self.channels.len() {
            return Err(AudioError::Parameter("Channel index out of range".into()));
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

impl AudioNode for MixerNode {
    fn process(&mut self, inputs: &[&[f32]], outputs: &mut [&mut [f32]]) -> Result<(), AudioError> {
        // We expect inputs in interleaved format: for each channel, we have a buffer.
        // If there are more inputs than channels, extra inputs are ignored.
        // Outputs: first two are master left/right, then buses (if any)
        
        if outputs.is_empty() {
            return Ok(());
        }
        
        let num_channels = self.channels.len();
        let num_buses = self.buses.len();
        let buffer_size = outputs[0].len();
        
        // Ensure bus buffers are sized correctly
        for bus in &mut self.buses {
            if bus.len() != buffer_size {
                bus.resize(buffer_size, 0.0);
            } else {
                bus.fill(0.0);
            }
        }
        
        // Prepare temporary output accumulators for master
        let mut master_left = vec![0.0; buffer_size];
        let mut master_right = vec![0.0; buffer_size];
        
        // Process each channel
        for (ch_idx, channel) in self.channels.iter_mut().enumerate() {
            if ch_idx >= inputs.len() {
                // No input for this channel
                continue;
            }
            let input_buf = inputs[ch_idx];
            if input_buf.len() < buffer_size {
                // Not enough samples, skip (should not happen in real use)
                continue;
            }
            
            // Сохраняем текущую громкость для send'ов (до обработки)
            let channel_volume = channel.config().volume;
            
            // Process per sample
            for i in 0..buffer_size {
                let sample = input_buf[i];
                
                // Channel processing (mono input, stereo output with pan)
                let (left_out, right_out) = channel.process_mono(sample);
                
                // Add to master
                master_left[i] += left_out;
                master_right[i] += right_out;
                
                // Process sends
                for send in &self.sends[ch_idx] {
                    if send.bus_index < self.buses.len() {
                        let bus = &mut self.buses[send.bus_index];
                        
                        let send_signal = match send.send_type {
                            SendType::PreFader => sample,
                            SendType::PostFader => sample * channel_volume,
                        };
                        
                        bus[i] += send_signal * send.level;
                    }
                }
            }
        }
        
        // Apply master volume with smoothing
        self.current_master_volume += (self.master_volume - self.current_master_volume) * self.smoothing;
        let master_gain = self.current_master_volume;
        
        // Output master
        if outputs.len() >= 2 {
            for i in 0..buffer_size {
                outputs[0][i] = master_left[i] * master_gain;
                outputs[1][i] = master_right[i] * master_gain;
            }
        } else if outputs.len() == 1 {
            let mono_out = &mut outputs[0];
            for i in 0..buffer_size {
                mono_out[i] = (master_left[i] + master_right[i]) * 0.5 * master_gain;
            }
        }
        
        // Output buses (starting from output index 2)
        for (bus_idx, bus) in self.buses.iter().enumerate() {
            let out_idx = 2 + bus_idx;
            if out_idx < outputs.len() {
                outputs[out_idx].copy_from_slice(&bus[..buffer_size]);
            }
        }
        
        Ok(())
    }
    
    fn get_param(&self, name: &str) -> Option<ParamValue> {
        // Parse parameter names like "ch_1_volume", "ch_1_pan", "ch_1_mute", "master_volume"
        if name == "master_volume" {
            return Some(ParamValue::Float(self.master_volume));
        }
        
        if name.starts_with("ch_") {
            let parts: Vec<&str> = name.split('_').collect();
            if parts.len() >= 3 {
                if let Ok(idx) = parts[1].parse::<usize>() {
                    if idx > 0 && idx <= self.channels.len() {
                        let channel = &self.channels[idx-1];
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
        
        None
    }
    
    fn set_param(&mut self, name: &str, value: ParamValue) -> Result<(), AudioError> {
        if name == "master_volume" {
            if let ParamValue::Float(v) = value {
                self.set_master_volume(v);
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
                                    return self.set_channel_volume(idx-1, v);
                                }
                            }
                            "pan" => {
                                if let ParamValue::Float(v) = value {
                                    return self.set_channel_pan(idx-1, v);
                                }
                            }
                            "mute" => {
                                if let ParamValue::Bool(v) = value {
                                    return self.set_channel_mute(idx-1, v);
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
        
        Err(AudioError::Parameter(format!("Unknown parameter: {}", name)))
    }
    
    fn init(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        // Nothing else to init
    }
    
    fn reset(&mut self) {
        self.current_master_volume = self.master_volume;
        for channel in &mut self.channels {
            channel.set_smoothing(self.smoothing); // reset smoothing? maybe just keep
        }
    }
    
    fn num_inputs(&self) -> usize {
        self.channels.len()
    }
    
    fn num_outputs(&self) -> usize {
        2 + self.buses.len()  // master L/R + buses
    }
    
    fn node_type_id(&self) -> NodeTypeId {
        NodeTypeId::of::<Self>()
    }
    
    fn metadata(&self) -> NodeMetadata {
        let mut params = vec![
            ParamMetadata {
                name: "master_volume".to_string(),
                typ: ParamType::Float,
                default: ParamValue::Float(1.0),
                min: Some(0.0),
                max: Some(2.0),
                step: Some(0.01),
                unit: Some("gain".to_string()),
                choices: None,
            },
        ];
        
        // Add per-channel parameters
        for i in 0..self.channels.len() {
            let ch_num = i+1;
            params.push(ParamMetadata {
                name: format!("ch_{}_volume", ch_num),
                typ: ParamType::Float,
                default: ParamValue::Float(1.0),
                min: Some(0.0),
                max: Some(1.0),
                step: Some(0.01),
                unit: Some("gain".to_string()),
                choices: None,
            });
            params.push(ParamMetadata {
                name: format!("ch_{}_pan", ch_num),
                typ: ParamType::Float,
                default: ParamValue::Float(0.0),
                min: Some(-1.0),
                max: Some(1.0),
                step: Some(0.01),
                unit: Some("pan".to_string()),
                choices: None,
            });
            params.push(ParamMetadata {
                name: format!("ch_{}_mute", ch_num),
                typ: ParamType::Bool,
                default: ParamValue::Bool(false),
                min: None,
                max: None,
                step: None,
                unit: None,
                choices: None,
            });
        }
        
        NodeMetadata {
            name: "Mixer".to_string(),
            category: NodeCategory::Mixer,
            description: format!("Mixer with {} channels and {} buses", self.channels.len(), self.buses.len()),
            author: "Kama Mixer".to_string(),
            version: "0.1.0".to_string(),
            parameters: params,
        }
    }
}