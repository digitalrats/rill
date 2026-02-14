//! Реактивная система событий для микшера

use tokio::sync::broadcast;

/// События микшера
#[derive(Debug, Clone)]
pub enum MixerEvent {
    LevelChanged {
        channel_id: usize,
        value: f64,
    },
    PanChanged {
        channel_id: usize,
        value: f64,
    },
    MuteToggled {
        channel_id: usize,
        muted: bool,
    },
    SoloToggled {
        channel_id: usize,
        soloed: bool,
    },
    MasterLevelChanged(f64),
    MasterPanChanged(f64),
    SignalProcessed {
        inputs: Vec<f64>,
        outputs: (f64, f64),
    },
}

/// Система событий микшера
#[derive(Debug, Clone)]
pub struct MixerEventSystem {
    tx: broadcast::Sender<MixerEvent>,
}

impl MixerEventSystem {
    pub fn new(capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(capacity);
        Self { tx }
    }
    
    pub fn emit(&self, event: MixerEvent) {
        let _ = self.tx.send(event);
    }
    
    pub fn subscribe(&self) -> broadcast::Receiver<MixerEvent> {
        self.tx.subscribe()
    }
}