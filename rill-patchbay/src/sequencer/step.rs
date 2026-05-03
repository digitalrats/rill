use super::snapshot::ParameterTarget;

/// A single step in a sequencer pattern.
///
/// Each step carries zero or more *parameter locks* (p-locks): specific
/// parameter values that are sent when the step becomes active.  Parameters
/// not listed in `parameters` keep their current value — they are *not*
/// reset or cleared.
///
/// # Duration
///
/// `duration_notes` expresses the step length in quarter-note units at the
/// current tempo:
///
/// | `duration_notes` | Musical duration |
/// |---|---|
/// | 4.0  | whole note      |
/// | 2.0  | half note       |
/// | 1.0  | quarter note    |
/// | 0.5  | eighth note     |
/// | 0.25 | sixteenth note  |
/// | 1.5  | dotted quarter  |
/// | …    | any value >= 0  |
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone)]
pub struct SequenceStep {
    /// Parameter target locks for this step.
    pub parameters: Vec<ParameterTarget>,
    /// Step duration in quarter-note units.
    pub duration_notes: f64,
}

impl SequenceStep {
    /// Create a new step with the given p-locks and duration.
    pub fn new(parameters: Vec<ParameterTarget>, duration_notes: f64) -> Self {
        Self {
            parameters,
            duration_notes: duration_notes.max(0.0),
        }
    }

    /// Create a single-parameter step (convenience constructor).
    pub fn single(
        node_id: impl Into<rill_core::NodeId>,
        param: impl Into<String>,
        value: f32,
        duration_notes: f64,
    ) -> Self {
        Self {
            parameters: vec![ParameterTarget::new(node_id.into(), param, value)],
            duration_notes: duration_notes.max(0.0),
        }
    }

    /// Return the step duration in audio samples for the given tempo and
    /// sample rate.
    ///
    /// `quarter_note_samples = (60.0 / tempo) * sample_rate`
    pub fn duration_samples(&self, tempo: f32, sample_rate: f32) -> u64 {
        let qn = (60.0 / tempo.max(1.0)) * sample_rate;
        (qn * self.duration_notes as f32) as u64
    }
}
