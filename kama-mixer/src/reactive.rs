//! Реактивный микшер с событиями

use kama_core::mixer::basic::{BasicMixer, MixerConfig};
use crate::events::{MixerEvent, MixerEventSystem};

#[cfg(feature = "reactive")]
use tokio::sync::mpsc;

/// Обновление параметра
#[derive(Debug, Clone)]
pub enum ParameterUpdate {
    ChannelLevel { channel_id: usize, value: f64 },
    ChannelPan { channel_id: usize, value: f64 },
    ChannelMute { channel_id: usize, muted: bool },
    ChannelSolo { channel_id: usize, soloed: bool },
    MasterLevel(f64),
    MasterPan(f64),
}

/// Реактивный микшер
#[cfg(feature = "reactive")]
pub struct ReactiveMixer {
    base_mixer: BasicMixer,
    event_system: MixerEventSystem,
    #[allow(dead_code)]
    update_tx: mpsc::UnboundedSender<ParameterUpdate>,
}

#[cfg(feature = "reactive")]
impl ReactiveMixer {
    pub fn new(config: MixerConfig) -> Self {
        let base_mixer = BasicMixer::new(config);
        let event_system = MixerEventSystem::new(100);
        let (update_tx, _update_rx) = mpsc::unbounded_channel();
        
        Self {
            base_mixer,
            event_system: event_system.clone(),
            update_tx,
        }
    }
    
    pub fn process(&mut self, inputs: &[f64]) -> (f64, f64) {
        let outputs = self.base_mixer.process(inputs);
        
        self.event_system.emit(MixerEvent::SignalProcessed {
            inputs: inputs.to_vec(),
            outputs,
        });
        
        outputs
    }
    
    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<MixerEvent> {
        self.event_system.subscribe()
    }
    
    pub fn base_mixer(&self) -> &BasicMixer {
        &self.base_mixer
    }
    
    pub fn base_mixer_mut(&mut self) -> &mut BasicMixer {
        &mut self.base_mixer
    }
}

/// Заглушка для случая без feature reactive
#[cfg(not(feature = "reactive"))]
pub struct ReactiveMixer;

#[cfg(not(feature = "reactive"))]
impl ReactiveMixer {
    pub fn new(_config: MixerConfig) -> Self {
        panic!("ReactiveMixer requires the 'reactive' feature");
    }
    
    pub fn process(&mut self, _inputs: &[f64]) -> (f64, f64) {
        panic!("ReactiveMixer requires the 'reactive' feature");
    }
}