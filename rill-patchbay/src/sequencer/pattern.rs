use super::step::SequenceStep;

/// Playback mode for a pattern.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StepPlayMode {
    /// Play through once then stop.
    OneShot,
    /// Loop the pattern indefinitely.
    Loop,
    /// Forward then backward (ping-pong).
    PingPong,
    /// Pick steps at random.
    Random,
    /// Brownian motion — drift to neighbouring steps.
    Brownian,
}

impl StepPlayMode {
    /// Pick the next step index given the current one and the pattern length.
    pub fn next_index(&self, current: usize, len: usize) -> usize {
        if len == 0 {
            return 0;
        }
        match self {
            StepPlayMode::OneShot => current.min(len.saturating_sub(1)),
            StepPlayMode::Loop => (current + 1) % len,
            StepPlayMode::PingPong => {
                // direction is stored externally — see engine
                current
            }
            StepPlayMode::Random => {
                use rand::Rng;
                let mut rng = rand::thread_rng();
                rng.gen_range(0..len)
            }
            StepPlayMode::Brownian => {
                use rand::Rng;
                let mut rng = rand::thread_rng();
                let offset: isize = rng.gen_range(-1..=1);
                let next = current as isize + offset;
                next.clamp(0, len.saturating_sub(1) as isize) as usize
            }
        }
    }
}

/// A sequence of steps forming a pattern.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone)]
pub struct Pattern {
    pub id: String,
    pub steps: Vec<SequenceStep>,
    pub play_mode: StepPlayMode,
}

impl Pattern {
    pub fn new(id: impl Into<String>, steps: Vec<SequenceStep>) -> Self {
        Self {
            id: id.into(),
            steps,
            play_mode: StepPlayMode::Loop,
        }
    }

    pub fn with_mode(mut self, mode: StepPlayMode) -> Self {
        self.play_mode = mode;
        self
    }

    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    pub fn len(&self) -> usize {
        self.steps.len()
    }
}
