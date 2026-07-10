use rill_core::math::Transcendental;
use rill_core::queues::spsc::SpscQueue;
use rill_core::queues::TelemetryBlock;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct TelemetryProbe<T: Transcendental, const BUF_SIZE: usize, const QUEUE_CAP: usize> {
    queue: Arc<SpscQueue<TelemetryBlock<T, BUF_SIZE>, QUEUE_CAP>>,
    interval: u32,
    counter: u32,
    block_index: u64,
    channel: u32,
}

impl<T: Transcendental, const BUF_SIZE: usize, const QUEUE_CAP: usize>
    TelemetryProbe<T, BUF_SIZE, QUEUE_CAP>
{
    pub fn new(
        queue: Arc<SpscQueue<TelemetryBlock<T, BUF_SIZE>, QUEUE_CAP>>,
        interval: u32,
        channel: u32,
    ) -> Self {
        Self {
            queue,
            interval,
            counter: 0,
            block_index: 0,
            channel,
        }
    }
    pub fn process_block(&mut self, input: &[T; BUF_SIZE], output: &mut [T; BUF_SIZE]) {
        output.copy_from_slice(input);
        self.counter += 1;
        if self.counter < self.interval {
            return;
        }
        self.counter = 0;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_micros() as u64;
        let mut block = TelemetryBlock::<T, BUF_SIZE>::default();
        block.node_id = 0;
        block.channel = self.channel;
        block.timestamp = now;
        block.sample_rate = 44100.0;
        block.block_index = self.block_index;
        self.block_index += 1;
        block.data.copy_from_slice(input);
        block.compute_metrics();
        let _ = self.queue.push(block);
    }
}
