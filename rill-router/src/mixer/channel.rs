//! Mixer channel implementation


/// Channel mode (mono or stereo)
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ChannelMode {
    Mono,
    Stereo,
}

/// Configuration for a mixer channel
#[derive(Debug, Clone)]
pub struct ChannelConfig {
    /// Channel name (for identification)
    pub name: String,
    /// Channel mode
    pub mode: ChannelMode,
    /// Volume (0.0 - 1.0, default 1.0)
    pub volume: f32,
    /// Pan (-1.0 left, 0.0 center, 1.0 right)
    pub pan: f32,
    /// Mute state
    pub muted: bool,
    /// Solo state
    pub soloed: bool,
}

impl Default for ChannelConfig {
    fn default() -> Self {
        Self {
            name: "Channel".to_string(),
            mode: ChannelMode::Mono,
            volume: 1.0,
            pan: 0.0,
            muted: false,
            soloed: false,
        }
    }
}

/// Runtime state of a mixer channel
#[derive(Debug, Clone)]
pub struct ChannelState {
    config: ChannelConfig,
    /// Current volume with smoothing
    current_volume: f32,
    /// Current pan
    current_pan: f32,
    /// Smoothing factor (0.0 - 1.0)
    smoothing: f32,
}

impl ChannelState {
    /// Create a new channel state
    pub fn new(config: ChannelConfig) -> Self {
        let current_volume = config.volume;
        let current_pan = config.pan;
        Self {
            config,
            current_volume,
            current_pan,
            smoothing: 0.1, // default smoothing
        }
    }

    /// Process a mono sample through the channel with pan
    pub fn process_mono(&mut self, input: f32) -> (f32, f32) {
        if self.config.muted {
            return (0.0, 0.0);
        }

        // Smooth volume and pan
        self.current_volume += (self.config.volume - self.current_volume) * self.smoothing;
        self.current_pan += (self.config.pan - self.current_pan) * self.smoothing;

        // Apply pan (simple constant power panning)
        let (left_gain, right_gain) = if self.current_pan <= 0.0 {
            (1.0, 1.0 + self.current_pan)
        } else {
            (1.0 - self.current_pan, 1.0)
        };

        let left_out = input * self.current_volume * left_gain;
        let right_out = input * self.current_volume * right_gain;

        (left_out, right_out)
    }

    /// Process a stereo sample through the channel
    pub fn process_stereo(&mut self, left: f32, right: f32) -> (f32, f32) {
        if self.config.muted {
            return (0.0, 0.0);
        }

        // Smooth volume and pan
        self.current_volume += (self.config.volume - self.current_volume) * self.smoothing;
        self.current_pan += (self.config.pan - self.current_pan) * self.smoothing;

        // Apply pan (simple constant power panning)
        let (left_gain, right_gain) = if self.current_pan <= 0.0 {
            (1.0, 1.0 + self.current_pan)
        } else {
            (1.0 - self.current_pan, 1.0)
        };

        let left_out = left * self.current_volume * left_gain;
        let right_out = right * self.current_volume * right_gain;

        (left_out, right_out)
    }

    /// Update configuration
    pub fn set_config(&mut self, config: ChannelConfig) {
        self.config = config;
    }

    /// Get current configuration
    pub fn config(&self) -> &ChannelConfig {
        &self.config
    }

    /// Set smoothing factor (0.0 = instant, 1.0 = very slow)
    pub fn set_smoothing(&mut self, factor: f32) {
        self.smoothing = factor.clamp(0.0, 1.0);
    }

    /// Get current volume (after smoothing)
    pub fn current_volume(&self) -> f32 {
        self.current_volume
    }

    /// Get current pan (after smoothing)
    pub fn current_pan(&self) -> f32 {
        self.current_pan
    }
}
