use crate::math::AudioNum;
use crate::traits::NodeId;

/// Fixed-size telemetry frame for RT-safe ring buffer communication.
///
/// Contains a full audio block plus computed metrics (peak, RMS, DC offset).
/// Implements `Copy` + `Default`, compatible with `SpscQueue` (overwrite-oldest).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct TelemetryBlock<T: AudioNum, const BUF_SIZE: usize> {
    /// Source node identifier
    pub node_id: NodeId,
    /// Audio channel index
    pub channel: u32,
    /// Timestamp (microseconds since UNIX epoch)
    pub timestamp: u64,
    /// Sample rate at capture time
    pub sample_rate: f32,
    /// Monotonic block counter
    pub block_index: u64,
    /// Peak amplitude of the block
    pub peak: T,
    /// RMS (root mean square) of the block
    pub rms: T,
    /// DC offset (average) of the block
    pub dc_offset: T,
    /// Full audio block data
    pub data: [T; BUF_SIZE],
}

impl<T: AudioNum, const BUF_SIZE: usize> Default for TelemetryBlock<T, BUF_SIZE> {
    fn default() -> Self {
        Self {
            node_id: NodeId(0),
            channel: 0,
            timestamp: 0,
            sample_rate: 44100.0,
            block_index: 0,
            peak: T::ZERO,
            rms: T::ZERO,
            dc_offset: T::ZERO,
            data: [T::ZERO; BUF_SIZE],
        }
    }
}

impl<T: AudioNum, const BUF_SIZE: usize> TelemetryBlock<T, BUF_SIZE> {
    /// Compute metrics (peak, RMS, DC offset) from the block data.
    #[inline]
    pub fn compute_metrics(&mut self) {
        let mut sum = T::ZERO;
        let mut sq_sum = T::ZERO;
        let mut peak = T::ZERO;

        for &sample in self.data.iter() {
            let abs = sample.abs();
            if abs > peak {
                peak = abs;
            }
            sum = sum + sample;
            sq_sum = sq_sum + sample * sample;
        }

        let len = T::from_f32(BUF_SIZE as f32);
        self.dc_offset = sum / len;
        self.rms = (sq_sum / len).sqrt();
        self.peak = peak;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_telemetry_block_default() {
        let block = TelemetryBlock::<f32, 64>::default();
        assert_eq!(block.node_id, NodeId(0));
        assert_eq!(block.channel, 0);
        assert_eq!(block.timestamp, 0);
        assert_eq!(block.block_index, 0);
        assert_eq!(block.peak, 0.0);
        assert_eq!(block.rms, 0.0);
        assert_eq!(block.dc_offset, 0.0);
        assert_eq!(block.data.len(), 64);
    }

    #[test]
    fn test_telemetry_block_copy() {
        let block = TelemetryBlock::<f32, 64>::default();
        let copied = block;
        assert_eq!(copied.node_id, block.node_id);
    }

    #[test]
    fn test_compute_metrics_sine() {
        let mut block = TelemetryBlock::<f32, 64>::default();
        for (i, sample) in block.data.iter_mut().enumerate() {
            *sample = (i as f32 * std::f32::consts::TAU / 64.0).sin();
        }
        block.compute_metrics();
        assert!((block.peak - 1.0).abs() < 0.01, "peak={}", block.peak);
        assert!((block.rms - 0.707).abs() < 0.01, "rms={}", block.rms);
        assert!(
            block.dc_offset.abs() < 0.01,
            "dc_offset={}",
            block.dc_offset
        );
    }

    #[test]
    fn test_compute_metrics_dc() {
        let mut block = TelemetryBlock::<f32, 64>::default();
        for sample in block.data.iter_mut() {
            *sample = 0.5;
        }
        block.compute_metrics();
        assert_eq!(block.dc_offset, 0.5);
        assert_eq!(block.peak, 0.5);
        assert_eq!(block.rms, 0.5);
    }
}
