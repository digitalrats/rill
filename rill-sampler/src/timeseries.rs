use rill_core::interpolate::Interpolate;
use rill_core::time::ClockTick;
use rill_core::traits::{
    SignalNode, NodeCategory, NodeId, NodeMetadata, NodeState, ParamValue, ParameterId, Port, Source,
};
use rill_core::Transcendental;
use rill_core::{ProcessError, ProcessResult};
use std::marker::PhantomData;

/// Interpolation strategy for reading between samples.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum InterpMode {
    /// Nearest-neighbour (no interpolation). Works for any `T`.
    Nearest,
    /// Linear interpolation. Requires `T: Transcendental`.
    Linear,
    /// Cubic Hermite interpolation. Requires `T: Transcendental`.
    Cubic,
}

/// One channel of an unevenly-sampled time series.
///
/// `timestamps` must be monotonically non-decreasing and aligned with `values`.
#[derive(Debug, Clone)]
pub struct TimeSeriesChannel<T> {
    /// Channel display name (e.g. `"engine_speed"`).
    pub name: String,
    /// Timestamps in seconds from start (monotonic).
    pub timestamps: Vec<f64>,
    /// Sample values aligned with `timestamps`.
    pub values: Vec<T>,
}

impl<T> TimeSeriesChannel<T> {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            timestamps: Vec::new(),
            values: Vec::new(),
        }
    }

    /// Total duration covered by this channel (seconds).
    pub fn duration(&self) -> f64 {
        if self.timestamps.len() < 2 {
            0.0
        } else {
            self.timestamps[self.timestamps.len() - 1] - self.timestamps[0]
        }
    }

    pub fn len(&self) -> usize {
        self.timestamps.len()
    }

    pub fn is_empty(&self) -> bool {
        self.timestamps.is_empty()
    }

    /// Push one sample (caller must ensure timestamps stay monotonic).
    pub fn push(&mut self, t: f64, value: T) {
        self.timestamps.push(t);
        self.values.push(value);
    }
}

/// Unevenly-sampled time series reader.
///
/// Reads from multiple independent channels at a virtual uniform rate,
/// using the [`Interpolate`] trait for fractional-index interpolation.
///
/// # Type parameter
///
/// `T` must implement `Transcendental` for `Linear` / `Cubic` modes.
/// `Nearest` mode only requires `Copy`.
pub struct TimeSeriesReader<T> {
    channels: Vec<TimeSeriesChannel<T>>,
    interp: InterpMode,
}

impl<T: Transcendental + Copy> TimeSeriesReader<T> {
    pub fn new() -> Self {
        Self {
            channels: Vec::new(),
            interp: InterpMode::Nearest,
        }
    }

    pub fn with_interp(mut self, mode: InterpMode) -> Self {
        self.interp = mode;
        self
    }

    pub fn set_interp(&mut self, mode: InterpMode) {
        self.interp = mode;
    }

    pub fn interp_mode(&self) -> InterpMode {
        self.interp
    }

    pub fn add_channel(&mut self, channel: TimeSeriesChannel<T>) {
        self.channels.push(channel);
    }

    pub fn num_channels(&self) -> usize {
        self.channels.len()
    }

    pub fn channel(&self, index: usize) -> Option<&TimeSeriesChannel<T>> {
        self.channels.get(index)
    }

    pub fn channel_mut(&mut self, index: usize) -> Option<&mut TimeSeriesChannel<T>> {
        self.channels.get_mut(index)
    }

    /// Total time span across all channels (union of ranges).
    pub fn duration(&self) -> f64 {
        self.channels.iter().map(|c| c.duration()).fold(0.0, f64::max)
    }

    /// Read value from a single channel at an arbitrary timestamp.
    pub fn at_time(&self, channel: usize, t: f64) -> T {
        let Some(ch) = self.channels.get(channel) else {
            return T::ZERO;
        };
        if ch.len() < 2 {
            return ch.values.first().copied().unwrap_or(T::ZERO);
        }

        // Binary search for the segment containing t
        let idx = match ch.timestamps.binary_search_by(|&ts| ts.partial_cmp(&t).unwrap()) {
            Ok(i) => {
                // Exact match: return the value directly
                return ch.values[i];
            }
            Err(i) => {
                // i is where t would be inserted
                if i == 0 {
                    return ch.values[0]; // before start → clamp
                }
                if i >= ch.len() {
                    return ch.values[ch.len() - 1]; // past end → clamp
                }
                i - 1 // segment index
            }
        };

        let t0 = ch.timestamps[idx];
        let t1 = ch.timestamps[idx + 1];
        let span = t1 - t0;
        if span <= 0.0 {
            return ch.values[idx];
        }

        let frac = (t - t0) / span;
        let index = idx as f64 + frac;

        match self.interp {
            InterpMode::Nearest => ch.values.interpolate_nearest(index),
            InterpMode::Linear => ch.values.interpolate_linear(index),
            InterpMode::Cubic => ch.values.interpolate_cubic(index),
        }
    }

    /// Fill a planar output buffer.
    ///
    /// Layout: `[ch0_s0, ch0_s1, ..., ch0_s{BUF-1}, ch1_s0, ...]`
    /// i.e. `output[ch * buf_size + i]`.
    pub fn read_block(&self, time: f64, sample_rate: f64, output: &mut [T]) {
        let nch = self.channels.len();
        if nch == 0 {
            for s in output.iter_mut() {
                *s = T::ZERO;
            }
            return;
        }
        let buf_size = output.len() / nch;
        let dt = 1.0 / sample_rate;
        for (ch, s) in output.chunks_mut(buf_size).enumerate() {
            for (i, v) in s.iter_mut().enumerate() {
                *v = self.at_time(ch, time + i as f64 * dt);
            }
        }
    }

    pub fn channels(&self) -> &[TimeSeriesChannel<T>] {
        &self.channels
    }
}

impl<T: Transcendental + Copy> Default for TimeSeriesReader<T> {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Graph node
// ---------------------------------------------------------------------------

/// Source node wrapping [`TimeSeriesReader`].
///
/// Produces one output port per channel, each filled at the configured
/// virtual `sample_rate`. All automatable via patchbay.
pub struct TimeSeriesNode<T: Transcendental, const BUF_SIZE: usize> {
    reader: TimeSeriesReader<T>,
    sample_rate: f64,
    playing: bool,
    time: f64,
    speed: f64,
    outputs: Vec<Port<T, BUF_SIZE>>,
    state: Option<NodeState<T, BUF_SIZE>>,
    _phantom: PhantomData<[T; BUF_SIZE]>,
}

impl<T: Transcendental + Copy, const BUF_SIZE: usize> TimeSeriesNode<T, BUF_SIZE> {
    pub fn new() -> Self {
        Self {
            reader: TimeSeriesReader::new().with_interp(InterpMode::Linear),
            sample_rate: 100.0,
            playing: true,
            time: 0.0,
            speed: 1.0,
            outputs: Vec::new(),
            state: None,
            _phantom: PhantomData,
        }
    }

    pub fn reader(&self) -> &TimeSeriesReader<T> {
        &self.reader
    }

    pub fn reader_mut(&mut self) -> &mut TimeSeriesReader<T> {
        &mut self.reader
    }

    pub fn set_channels(&mut self, channels: Vec<TimeSeriesChannel<T>>) {
        self.outputs.clear();
        for (i, ch) in channels.iter().enumerate() {
            self.outputs
                .push(Port::output(NodeId(0), i as u16, &ch.name));
        }
        self.reader = TimeSeriesReader {
            channels,
            interp: self.reader.interp,
        };
        self.time = 0.0;
    }

    fn param_to_t(value: ParamValue) -> Option<T> {
        match value {
            ParamValue::Float(f) => Some(T::from_f32(f)),
            ParamValue::Int(i) => Some(T::from_f32(i as f32)),
            _ => None,
        }
    }
}

impl<T: Transcendental + Copy, const BUF_SIZE: usize> Default
    for TimeSeriesNode<T, BUF_SIZE>
{
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Transcendental + Copy, const BUF_SIZE: usize> SignalNode<T, BUF_SIZE>
    for TimeSeriesNode<T, BUF_SIZE>
{
    fn metadata(&self) -> NodeMetadata {
        NodeMetadata {
            name: "TimeSeries".to_string(),
            type_name: None,
            category: NodeCategory::Source,
            description: "Unevenly-sampled time series reader with multiple output channels".into(),
            author: "Rill".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            audio_inputs: 0,
            audio_outputs: self.outputs.len(),
            control_inputs: 0,
            control_outputs: 0,
            clock_inputs: 0,
            clock_outputs: 0,
            feedback_ports: 0,
            parameters: vec![],
        }
    }

    fn init(&mut self, sample_rate: f32) {
        self.state = Some(NodeState::new(sample_rate));
    }

    fn reset(&mut self) {
        self.time = 0.0;
        self.playing = true;
        if let Some(state) = &mut self.state {
            state.reset();
        }
    }

    fn get_parameter(&self, id: &ParameterId) -> Option<ParamValue> {
        match id.as_str() {
            "sample_rate" => Some(ParamValue::Float(self.sample_rate as f32)),
            "interpolation" => {
                let s = match self.reader.interp_mode() {
                    InterpMode::Nearest => "nearest",
                    InterpMode::Linear => "linear",
                    InterpMode::Cubic => "cubic",
                };
                Some(ParamValue::Choice(s.into()))
            }
            "play" => Some(ParamValue::Bool(self.playing)),
            "position" => {
                let dur = self.reader.duration();
                if dur > 0.0 {
                    Some(ParamValue::Float((self.time / dur) as f32))
                } else {
                    Some(ParamValue::Float(0.0))
                }
            }
            "speed" => Some(ParamValue::Float(self.speed as f32)),
            _ => None,
        }
    }

    fn set_parameter(&mut self, id: &ParameterId, value: ParamValue) -> ProcessResult<()> {
        match id.as_str() {
            "sample_rate" => {
                if let Some(r) = Self::param_to_t(value) {
                    self.sample_rate = r.to_f64().clamp(0.1, 1_000_000.0);
                    Ok(())
                } else {
                    Err(ProcessError::Parameter("Expected float".into()))
                }
            }
            "interpolation" => {
                if let ParamValue::Choice(s) = &value {
                    self.reader.set_interp(match s.as_str() {
                        "linear" => InterpMode::Linear,
                        "cubic" => InterpMode::Cubic,
                        _ => InterpMode::Nearest,
                    });
                    Ok(())
                } else {
                    Err(ProcessError::Parameter("Expected choice".into()))
                }
            }
            "play" => {
                if let ParamValue::Bool(b) = value {
                    self.playing = b;
                    Ok(())
                } else {
                    Err(ProcessError::Parameter("Expected bool".into()))
                }
            }
            "speed" => {
                if let Some(s) = Self::param_to_t(value) {
                    self.speed = s.to_f64().clamp(0.0, 100.0);
                    Ok(())
                } else {
                    Err(ProcessError::Parameter("Expected float".into()))
                }
            }
            _ => Err(ProcessError::Parameter(format!(
                "Unknown parameter: {}",
                id
            ))),
        }
    }

    fn id(&self) -> NodeId {
        NodeId(0)
    }

    fn set_id(&mut self, _id: NodeId) {}

    fn input_port(&self, index: usize) -> Option<&Port<T, BUF_SIZE>> {
        None
    }

    fn input_port_mut(&mut self, index: usize) -> Option<&mut Port<T, BUF_SIZE>> {
        None
    }

    fn output_port(&self, index: usize) -> Option<&Port<T, BUF_SIZE>> {
        self.outputs.get(index)
    }

    fn output_port_mut(&mut self, index: usize) -> Option<&mut Port<T, BUF_SIZE>> {
        self.outputs.get_mut(index)
    }

    fn control_port(&self, index: usize) -> Option<&Port<T, BUF_SIZE>> {
        None
    }

    fn control_port_mut(&mut self, index: usize) -> Option<&mut Port<T, BUF_SIZE>> {
        None
    }

    fn state(&self) -> &NodeState<T, BUF_SIZE> {
        self.state.as_ref().unwrap()
    }

    fn state_mut(&mut self) -> &mut NodeState<T, BUF_SIZE> {
        self.state.as_mut().unwrap()
    }

    fn num_audio_inputs(&self) -> usize {
        0
    }

    fn num_audio_outputs(&self) -> usize {
        self.outputs.len()
    }
}

impl<T: Transcendental + Copy, const BUF_SIZE: usize> Source<T, BUF_SIZE>
    for TimeSeriesNode<T, BUF_SIZE>
{
    fn generate(
        &mut self,
        _clock: &ClockTick,
        _control_inputs: &[T],
        _clock_inputs: &[ClockTick],
    ) -> ProcessResult<()> {
        if !self.playing || self.reader.num_channels() == 0 {
            for port in self.outputs.iter_mut() {
                port.buffer.as_mut_array().fill(T::ZERO);
            }
            return Ok(());
        }

        let nch = self.reader.num_channels();
        let dur = self.reader.duration();
        let dt = 1.0 / self.sample_rate;

        // Planar per-channel writes
        for (ch_idx, port) in self.outputs.iter_mut().enumerate().take(nch) {
            let buf = port.buffer.as_mut_array();
            for (i, v) in buf.iter_mut().enumerate() {
                let t = self.time + i as f64 * dt;
                *v = self.reader.at_time(ch_idx, t);
            }
        }

        self.time += BUF_SIZE as f64 * dt * self.speed;

        // Clamp and optionally pause at end
        if self.time >= dur && dur > 0.0 {
            if self.speed > 0.0 {
                self.time = dur; // hold last values
                self.playing = false;
            }
        } else if self.time < 0.0 {
            self.time = 0.0;
            self.playing = false;
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// CSV loader
// ---------------------------------------------------------------------------

/// Load a time-series reader from a CSV string.
///
/// Expected format (header optional):
/// ```csv
/// t,channel,value
/// 0.001,engine_speed,1500
/// 0.001,oil_temp,85
/// ```
///
/// Lines that cannot be parsed are silently skipped.
pub fn from_csv<T: Transcendental + Copy>(input: &str) -> TimeSeriesReader<T> {
    use std::collections::BTreeMap;

    let mut raw: BTreeMap<String, Vec<(f64, T)>> = BTreeMap::new();

    for line in input.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with("t,") || line.starts_with("timestamp,") {
            continue;
        }
        let mut parts = line.splitn(3, ',');
        let t: f64 = match parts.next().and_then(|s| s.trim().parse().ok()) {
            Some(v) => v,
            None => continue,
        };
        let name = match parts.next() {
            Some(s) => s.trim().to_string(),
            None => continue,
        };
        let value: f64 = match parts.next().and_then(|s| s.trim().parse().ok()) {
            Some(v) => v,
            None => continue,
        };

        raw.entry(name)
            .or_default()
            .push((t, T::from_f64(value)));
    }

    let mut reader = TimeSeriesReader::new();
    for (name, mut samples) in raw {
        // Sort by timestamp (BTreeMap iteration is key-ordered, but values
        // within a channel may arrive out of order in CSV)
        samples.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        let mut ch = TimeSeriesChannel::new(&name);
        for (t, v) in samples {
            ch.push(t, v);
        }
        reader.add_channel(ch);
    }

    reader
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ae(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    #[test]
    fn test_at_time_exact() {
        let mut ch = TimeSeriesChannel::new("test");
        ch.push(0.0, 10.0);
        ch.push(1.0, 20.0);
        ch.push(2.0, 30.0);
        let mut reader = TimeSeriesReader::new();
        reader.add_channel(ch);

        assert!(ae(reader.at_time(0, 0.0), 10.0));
        assert!(ae(reader.at_time(0, 1.0), 20.0));
        assert!(ae(reader.at_time(0, 2.0), 30.0));
    }

    #[test]
    fn test_at_time_interpolated() {
        let mut ch = TimeSeriesChannel::new("test");
        ch.push(0.0, 0.0);
        ch.push(1.0, 2.0);
        let mut reader = TimeSeriesReader::new().with_interp(InterpMode::Linear);
        reader.add_channel(ch);

        assert!(ae(reader.at_time(0, 0.5), 1.0));
        assert!(ae(reader.at_time(0, 0.25), 0.5));
    }

    #[test]
    fn test_at_time_clamp() {
        let mut ch = TimeSeriesChannel::new("test");
        ch.push(1.0, 100.0);
        ch.push(2.0, 200.0);
        let mut reader = TimeSeriesReader::new();
        reader.add_channel(ch);

        assert!(ae(reader.at_time(0, 0.0), 100.0));
        assert!(ae(reader.at_time(0, 5.0), 200.0));
    }

    #[test]
    fn test_nearest_mode() {
        let mut ch = TimeSeriesChannel::new("test");
        ch.push(0.0, 10.0);
        ch.push(1.0, 20.0);
        let mut reader = TimeSeriesReader::new().with_interp(InterpMode::Nearest);
        reader.add_channel(ch);

        assert!(ae(reader.at_time(0, 0.49), 10.0));
        assert!(ae(reader.at_time(0, 0.5), 20.0));
    }

    #[test]
    fn test_empty_channel() {
        let ch = TimeSeriesChannel::new("empty");
        let mut reader = TimeSeriesReader::new();
        reader.add_channel(ch);
        assert!(ae(reader.at_time(0, 0.5), 0.0));
    }

    #[test]
    fn test_read_block() {
        let mut ch = TimeSeriesChannel::new("ch");
        ch.push(0.0, 1.0);
        ch.push(1.0, 3.0);
        let mut reader = TimeSeriesReader::new().with_interp(InterpMode::Linear);
        reader.add_channel(ch);

        let mut out = [0.0_f64; 4];
        reader.read_block(0.0, 2.0, &mut out);
        assert!(ae(out[0], 1.0));
        assert!(ae(out[1], 2.0));
        assert!(ae(out[2], 3.0));
        assert!(ae(out[3], 3.0));
    }

    #[test]
    fn test_read_multichannel() {
        let mut ch1 = TimeSeriesChannel::new("a");
        ch1.push(0.0, 10.0);
        ch1.push(1.0, 20.0);
        let mut ch2 = TimeSeriesChannel::new("b");
        ch2.push(0.0, 100.0);
        ch2.push(1.0, 200.0);
        let mut reader = TimeSeriesReader::new().with_interp(InterpMode::Linear);
        reader.add_channel(ch1);
        reader.add_channel(ch2);

        let mut out = [0.0_f64; 4];
        reader.read_block(0.5, 2.0, &mut out);
        assert!(ae(out[0], 15.0));
        assert!(ae(out[1], 20.0));
        assert!(ae(out[2], 150.0));
        assert!(ae(out[3], 200.0));
    }

    #[test]
    fn test_csv_loading() {
        let csv = "\
t,channel,value
0.0,speed,100
0.5,speed,200
0.0,temp,25
0.5,temp,30
";
        let reader: TimeSeriesReader<f64> = from_csv(csv);
        assert_eq!(reader.num_channels(), 2);
        let speed = reader.channel(0).unwrap();
        assert_eq!(speed.name, "speed");
        assert!(ae(speed.values[0], 100.0));
        assert!(ae(speed.values[1], 200.0));
    }

    #[test]
    fn test_timeseries_node_basic() {
        let mut ch = TimeSeriesChannel::new("test");
        ch.push(0.0, 1.0);
        ch.push(1.0, 2.0);
        let mut node = TimeSeriesNode::<f64, 4>::new();
        node.set_channels(vec![ch]);
        node.init(44100.0);
        node.sample_rate = 2.0;

        let clock = ClockTick::new(0, 4, 44100.0);
        node.generate(&clock, &[], &[]).unwrap();

        let port = node.output_port(0).unwrap();
        let buf = port.buffer.as_array();
        assert!(ae(buf[0], 1.0));
        assert!(ae(buf[1], 1.5));
        assert!(ae(buf[2], 2.0));
        assert!(ae(buf[3], 2.0));
    }
}
