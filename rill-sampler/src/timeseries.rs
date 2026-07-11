use rill_core::interpolate::Interpolate;
use rill_core::traits::algorithm::{Algorithm, AlgorithmCategory, AlgorithmMetadata};
use rill_core::traits::ProcessResult;
use rill_core::Transcendental;

/// Interpolation strategy for reading between samples.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum InterpMode {
    Nearest,
    Linear,
    Cubic,
}

/// One channel of an unevenly-sampled time series.
#[derive(Debug, Clone)]
pub struct TimeSeriesChannel<T> {
    pub name: String,
    pub timestamps: Vec<f64>,
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

    pub fn push(&mut self, t: f64, value: T) {
        self.timestamps.push(t);
        self.values.push(value);
    }
}

/// Unevenly-sampled time series reader with interpolation.
pub struct TimeSeriesReader<T> {
    channels: Vec<TimeSeriesChannel<T>>,
    interp: InterpMode,
    /// Current time in seconds, advanced during process().
    time: f64,
    sample_rate: f64,
}

impl<T: Transcendental + Copy> TimeSeriesReader<T> {
    pub fn new() -> Self {
        Self {
            channels: Vec::new(),
            interp: InterpMode::Nearest,
            time: 0.0,
            sample_rate: 44100.0,
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

    pub fn duration(&self) -> f64 {
        self.channels
            .iter()
            .map(|c| c.duration())
            .fold(0.0, f64::max)
    }

    pub fn at_time(&self, channel: usize, t: f64) -> T {
        let Some(ch) = self.channels.get(channel) else {
            return T::ZERO;
        };
        if ch.len() < 2 {
            return ch.values.first().copied().unwrap_or(T::ZERO);
        }
        let idx = match ch
            .timestamps
            .binary_search_by(|&ts| ts.partial_cmp(&t).unwrap())
        {
            Ok(i) => return ch.values[i],
            Err(i) => {
                if i == 0 {
                    return ch.values[0];
                }
                if i >= ch.len() {
                    return ch.values[ch.len() - 1];
                }
                i - 1
            }
        };
        let span = ch.timestamps[idx + 1] - ch.timestamps[idx];
        if span <= 0.0 {
            return ch.values[idx];
        }
        let frac = (t - ch.timestamps[idx]) / span;
        let index = idx as f64 + frac;
        match self.interp {
            InterpMode::Nearest => ch.values.interpolate_nearest(index),
            InterpMode::Linear => ch.values.interpolate_linear(index),
            InterpMode::Cubic => ch.values.interpolate_cubic(index),
        }
    }

    pub fn channels(&self) -> &[TimeSeriesChannel<T>] {
        &self.channels
    }
}

impl<T: Transcendental + Copy> Algorithm<T> for TimeSeriesReader<T> {
    fn init(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate as f64;
        self.time = 0.0;
    }

    fn reset(&mut self) {
        self.time = 0.0;
    }

    fn process(&mut self, _input: Option<&[T]>, output: &mut [T]) -> ProcessResult<()> {
        let nch = self.num_channels();
        if nch == 0 {
            output.fill(T::ZERO);
            return Ok(());
        }
        let buf_size = output.len() / nch;
        let dt = 1.0 / self.sample_rate;
        for (ch, s) in output.chunks_mut(buf_size).enumerate() {
            for (i, v) in s.iter_mut().enumerate() {
                *v = self.at_time(ch, self.time + i as f64 * dt);
            }
        }
        self.time += buf_size as f64 * dt;
        Ok(())
    }

    fn metadata(&self) -> AlgorithmMetadata {
        AlgorithmMetadata {
            name: "TimeSeriesReader",
            category: AlgorithmCategory::Analyzer,
            description: "Multichannel time series playback with interpolation",
            author: "Rill",
            version: env!("CARGO_PKG_VERSION"),
        }
    }
}

impl<T: Transcendental + Copy> Default for TimeSeriesReader<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// Parse CSV into a time-series reader.
pub fn from_csv<T: Transcendental + Copy>(input: &str) -> TimeSeriesReader<T> {
    let mut reader = TimeSeriesReader::new().with_interp(InterpMode::Linear);
    for line in input.lines().skip(1) {
        let parts: Vec<&str> = line.splitn(3, ',').collect();
        if parts.len() >= 3 {
            if let (Ok(t), Ok(v)) = (
                parts[0].trim().parse::<f64>(),
                parts[2].trim().parse::<f64>(),
            ) {
                reader.add_sample(parts[1].trim(), t, T::from_f64(v));
            }
        }
    }
    reader
}

impl<T: Transcendental + Copy> TimeSeriesReader<T> {
    fn add_sample(&mut self, channel_name: &str, t: f64, value: T) {
        for ch in &mut self.channels {
            if ch.name == channel_name {
                ch.push(t, value);
                return;
            }
        }
        let mut ch = TimeSeriesChannel::new(channel_name);
        ch.push(t, value);
        self.channels.push(ch);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_channel_creation() {
        let mut ch = TimeSeriesChannel::<f64>::new("test");
        ch.push(0.0, 1.0);
        ch.push(1.0, 2.0);
        assert!(!ch.is_empty());
        assert_eq!(ch.len(), 2);
    }

    #[test]
    fn test_reader_at_time() {
        let mut reader = TimeSeriesReader::<f64>::new().with_interp(InterpMode::Linear);
        let mut ch = TimeSeriesChannel::new("a");
        ch.push(0.0, 0.0);
        ch.push(1.0, 1.0);
        reader.add_channel(ch);

        let v = reader.at_time(0, 0.5);
        assert!((v - 0.5).abs() < 1e-9);
    }

    #[test]
    fn test_algorithm_process() {
        let mut reader = TimeSeriesReader::<f64>::new().with_interp(InterpMode::Linear);
        let mut ch = TimeSeriesChannel::new("a");
        ch.push(0.0, 0.0);
        ch.push(1.0, 10.0);
        reader.add_channel(ch);
        reader.init(100.0);

        let mut out = vec![0.0f64; 4];
        reader.process(None, &mut out).unwrap();
        assert!(out[0] >= 0.0);
        assert!(out[3] > out[0]);
    }
}
