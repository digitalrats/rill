//! Fan-out and fan-in buffers for complex routing
//!
//! - `FanOutBuffer`: One producer, multiple consumers (broadcast)
//! - `FanInBuffer`: Multiple producers, one consumer (mixing)

//! Fan-out and fan-in buffers

use crate::math::AudioNum;
use crate::buffer::{AlignedStorage, AtomicStats, array_from_fn};
use core::sync::atomic::{AtomicUsize, AtomicBool, Ordering};
use core::marker::PhantomData;

/// Fan-out buffer (one producer, multiple consumers)
#[repr(align(64))]
pub struct FanOutBuffer<T: AudioNum, const N: usize, const CONSUMERS: usize> {
    storage: AlignedStorage<T, N>,
    version: AtomicUsize,
    read_versions: [AtomicUsize; CONSUMERS],
    valid: AtomicBool,
    stats: AtomicStats,
    _phantom: PhantomData<T>,
}

impl<T: AudioNum, const N: usize, const CONSUMERS: usize> FanOutBuffer<T, N, CONSUMERS> {
    /// Create new fan-out buffer
    pub fn new() -> Self {
        Self {
            storage: AlignedStorage::new(),
            version: AtomicUsize::new(0),
            read_versions: array_from_fn(|_| AtomicUsize::new(0)),
            valid: AtomicBool::new(false),
            stats: AtomicStats::new(),
            _phantom: PhantomData,
        }
    }
    
    /// Write data to all consumers
    #[inline(always)]
    pub fn write(&self, data: &[T; N]) {
        for i in 0..N {
            self.storage.write(i, data[i]);
        }
        
        self.valid.store(true, Ordering::Release);
        self.stats.record_write();
        self.stats.update_peak(1);
    }
    
    /// Try to read for a specific consumer
    #[inline(always)]
    pub fn try_read(&self, consumer_id: usize) -> Option<[T; N]> {
        if consumer_id >= CONSUMERS {
            return None;
        }
        
        let current_version = self.version.load(Ordering::Acquire);
        let last_read = self.read_versions[consumer_id].load(Ordering::Relaxed);
        
        if last_read == current_version || !self.valid.load(Ordering::Acquire) {
            self.stats.record_underflow();
            return None;
        }
        
        let mut result = [T::ZERO; N];
        for i in 0..N {
            unsafe {
                result[i] = self.storage.read(i);
            }
        }
        
        self.read_versions[consumer_id].store(current_version, Ordering::Release);
        self.stats.record_read();
        
        Some(result)
    }
    
    /// Check if a specific consumer has new data
    #[inline(always)]
    pub fn has_new_data(&self, consumer_id: usize) -> bool {
        if consumer_id >= CONSUMERS {
            return false;
        }
        
        let current_version = self.version.load(Ordering::Acquire);
        let last_read = self.read_versions[consumer_id].load(Ordering::Relaxed);
        
        current_version != last_read && self.valid.load(Ordering::Acquire)
    }
    
    /// Get number of consumers
    pub fn consumer_count(&self) -> usize {
        CONSUMERS
    }
}

/// Fan-in buffer (multiple producers, one consumer)
#[repr(align(64))]
pub struct FanInBuffer<T: AudioNum, const N: usize, const PRODUCERS: usize> {
    storage: [AlignedStorage<T, N>; PRODUCERS],
    valid: [AtomicBool; PRODUCERS],
    write_seq: [AtomicUsize; PRODUCERS],
    /// Track which producers have been read
    read_seq: [AtomicUsize; PRODUCERS],
    stats: AtomicStats,
    _phantom: PhantomData<T>,
}

impl<T: AudioNum, const N: usize, const PRODUCERS: usize> FanInBuffer<T, N, PRODUCERS> {
    /// Create new fan-in buffer
    pub fn new() -> Self {
        Self {
            storage: array_from_fn(|_| AlignedStorage::new()),
            valid: array_from_fn(|_| AtomicBool::new(false)),
            write_seq: array_from_fn(|_| AtomicUsize::new(0)),
            read_seq: array_from_fn(|_| AtomicUsize::new(0)),
            stats: AtomicStats::new(),
            _phantom: PhantomData,
        }
    }
    
    /// Write data from a specific producer
    #[inline(always)]
    pub fn write(&self, producer_id: usize, data: &[T; N]) {
        if producer_id >= PRODUCERS {
            return;
        }
        
        for i in 0..N {
            self.storage[producer_id].write(i, data[i]);
        }
        
        self.valid[producer_id].store(true, Ordering::Release);
        self.write_seq[producer_id].fetch_add(1, Ordering::Release);
        self.stats.record_write();
    }
    
    /// Try to read mixed data from all producers
    #[inline(always)]
    pub fn try_read(&self) -> Option<[T; N]> {
        let mut result = [T::ZERO; N];
        let mut any_valid = false;
        let mut active_producers = 0;
        
        for producer in 0..PRODUCERS {
            if self.valid[producer].load(Ordering::Acquire) {
                let write_seq = self.write_seq[producer].load(Ordering::Relaxed);
                let read_seq = self.read_seq[producer].load(Ordering::Relaxed);
                
                // Include if this producer has written since last read
                if write_seq > read_seq {
                    any_valid = true;
                    active_producers += 1;
                    for i in 0..N {
                        unsafe {
                            result[i] = result[i] + self.storage[producer].read(i);
                        }
                    }
                }
            }
        }
        
        if any_valid {
            // Mark all producers as read
            for producer in 0..PRODUCERS {
                if self.valid[producer].load(Ordering::Acquire) {
                    let write_seq = self.write_seq[producer].load(Ordering::Relaxed);
                    self.read_seq[producer].store(write_seq, Ordering::Release);
                }
            }
            
            self.stats.record_read();
            self.stats.update_peak(active_producers);
            Some(result)
        } else {
            self.stats.record_underflow();
            None
        }
    }
    
    /// Get number of producers
    pub fn producer_count(&self) -> usize {
        PRODUCERS
    }
    
    /// Check if a specific producer has new data
    #[inline(always)]
    pub fn producer_has_data(&self, producer_id: usize) -> bool {
        if producer_id >= PRODUCERS {
            return false;
        }
        
        let write_seq = self.write_seq[producer_id].load(Ordering::Relaxed);
        let read_seq = self.read_seq[producer_id].load(Ordering::Relaxed);
        
        write_seq > read_seq && self.valid[producer_id].load(Ordering::Acquire)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_fan_in_partial() {
        let buffer = FanInBuffer::<f32, 64, 3>::new();
        
        // Producer 0 writes
        buffer.write(0, &[1.0; 64]);
        eprintln!("After write 0 - read_seq[0]: {}, write_seq[0]: {}", 
                  buffer.read_seq[0].load(Ordering::Relaxed),
                  buffer.write_seq[0].load(Ordering::Relaxed));
        
        // Should get data from producer 0
        let mixed1 = buffer.try_read();
        assert!(mixed1.is_some(), "Should get data from producer 0");
        if let Some(data) = mixed1 {
            assert_eq!(data[0], 1.0, "Data from producer 0 should be 1.0");
        }
        
        eprintln!("After read 1 - read_seq[0]: {}, write_seq[0]: {}", 
                  buffer.read_seq[0].load(Ordering::Relaxed),
                  buffer.write_seq[0].load(Ordering::Relaxed));
        
        // Producer 1 writes newer data
        buffer.write(1, &[2.0; 64]);
        eprintln!("After write 1 - read_seq[1]: {}, write_seq[1]: {}", 
                  buffer.read_seq[1].load(Ordering::Relaxed),
                  buffer.write_seq[1].load(Ordering::Relaxed));
        
        // Should get producer 1's data
        let mixed2 = buffer.try_read();
        assert!(mixed2.is_some(), "Should get data from producer 1");
        if let Some(data) = mixed2 {
            assert_eq!(data[0], 2.0, "Data from producer 1 should be 2.0");
        }
        
        eprintln!("After read 2 - read_seq[1]: {}, write_seq[1]: {}", 
                  buffer.read_seq[1].load(Ordering::Relaxed),
                  buffer.write_seq[1].load(Ordering::Relaxed));
        
        // No more data after reading
        let mixed3 = buffer.try_read();
        assert!(mixed3.is_none(), "No more data after reading both producers");
    }
}