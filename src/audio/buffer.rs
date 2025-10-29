use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use crate::models::AudioBuffer;
use crate::error::AudioError;

/// Thread-safe ring buffer for audio data with atomic read/write positions
#[derive(Debug)]
pub struct RingBuffer {
    buffer: Vec<f32>,
    capacity: usize,
    read_pos: AtomicUsize,
    write_pos: AtomicUsize,
    channels: u16,
    sample_rate: u32,
}

impl RingBuffer {
    /// Create a new ring buffer with the specified capacity in frames
    pub fn new(capacity_frames: usize, channels: u16, sample_rate: u32) -> Self {
        let capacity = capacity_frames * channels as usize;
        Self {
            buffer: vec![0.0; capacity],
            capacity,
            read_pos: AtomicUsize::new(0),
            write_pos: AtomicUsize::new(0),
            channels,
            sample_rate,
        }
    }

    /// Get the total capacity in samples
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Get the capacity in frames
    pub fn capacity_frames(&self) -> usize {
        self.capacity / self.channels as usize
    }

    pub fn channels(&self) -> u16 {
        self.channels
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Get the number of samples available for reading
    pub fn available_read(&self) -> usize {
        let write_pos = self.write_pos.load(Ordering::Acquire);
        let read_pos = self.read_pos.load(Ordering::Acquire);

        if write_pos >= read_pos {
            write_pos - read_pos
        } else {
            self.capacity - read_pos + write_pos
        }
    }

    /// Get the number of samples available for writing
    pub fn available_write(&self) -> usize {
        let available_read = self.available_read();
        // Leave one sample space to distinguish between full and empty
        if available_read == 0 {
            self.capacity - 1
        } else {
            self.capacity - available_read - 1
        }
    }

    /// Get the number of frames available for reading
    pub fn available_read_frames(&self) -> usize {
        self.available_read() / self.channels as usize
    }

    /// Get the number of frames available for writing
    pub fn available_write_frames(&self) -> usize {
        self.available_write() / self.channels as usize
    }

    /// Check if the buffer is empty
    pub fn is_empty(&self) -> bool {
        self.available_read() == 0
    }

    /// Check if the buffer is full
    pub fn is_full(&self) -> bool {
        self.available_write() == 0
    }

    /// Write audio data to the buffer
    /// Returns the number of samples actually written
    pub fn write(&self, data: &[f32]) -> usize {
        let available = self.available_write();
        let to_write = data.len().min(available);

        if to_write == 0 {
            return 0;
        }

        let write_pos = self.write_pos.load(Ordering::Acquire);

        // Handle wrap-around
        let end_space = self.capacity - write_pos;
        if to_write <= end_space {
            // No wrap-around needed
            unsafe {
                let buffer_ptr = self.buffer.as_ptr() as *mut f32;
                std::ptr::copy_nonoverlapping(
                    data.as_ptr(),
                    buffer_ptr.add(write_pos),
                    to_write,
                );
            }
        } else {
            // Need to wrap around
            let first_chunk = end_space;
            let second_chunk = to_write - first_chunk;

            unsafe {
                let buffer_ptr = self.buffer.as_ptr() as *mut f32;
                // Write first chunk to end of buffer
                std::ptr::copy_nonoverlapping(
                    data.as_ptr(),
                    buffer_ptr.add(write_pos),
                    first_chunk,
                );
                // Write second chunk to beginning of buffer
                std::ptr::copy_nonoverlapping(
                    data.as_ptr().add(first_chunk),
                    buffer_ptr,
                    second_chunk,
                );
            }
        }

        // Update write position
        let new_write_pos = (write_pos + to_write) % self.capacity;
        self.write_pos.store(new_write_pos, Ordering::Release);

        to_write
    }

    /// Read audio data from the buffer
    /// Returns the number of samples actually read
    pub fn read(&self, data: &mut [f32]) -> usize {
        let available = self.available_read();
        let to_read = data.len().min(available);

        if to_read == 0 {
            return 0;
        }

        let read_pos = self.read_pos.load(Ordering::Acquire);

        // Handle wrap-around
        let end_space = self.capacity - read_pos;
        if to_read <= end_space {
            // No wrap-around needed
            unsafe {
                let buffer_ptr = self.buffer.as_ptr();
                std::ptr::copy_nonoverlapping(
                    buffer_ptr.add(read_pos),
                    data.as_mut_ptr(),
                    to_read,
                );
            }
        } else {
            // Need to wrap around
            let first_chunk = end_space;
            let second_chunk = to_read - first_chunk;

            unsafe {
                let buffer_ptr = self.buffer.as_ptr();
                // Read first chunk from end of buffer
                std::ptr::copy_nonoverlapping(
                    buffer_ptr.add(read_pos),
                    data.as_mut_ptr(),
                    first_chunk,
                );
                // Read second chunk from beginning of buffer
                std::ptr::copy_nonoverlapping(
                    buffer_ptr,
                    data.as_mut_ptr().add(first_chunk),
                    second_chunk,
                );
            }
        }

        // Update read position
        let new_read_pos = (read_pos + to_read) % self.capacity;
        self.read_pos.store(new_read_pos, Ordering::Release);

        to_read
    }

    /// Write an AudioBuffer to the ring buffer
    /// Returns the number of frames actually written
    pub fn write_audio_buffer(&self, audio_buffer: &AudioBuffer) -> usize {
        if audio_buffer.channels != self.channels {
            return 0; // Channel mismatch
        }

        let samples_written = self.write(&audio_buffer.samples);
        samples_written / self.channels as usize
    }

    /// Read audio data into an AudioBuffer
    /// Returns the number of frames actually read
    pub fn read_audio_buffer(&self, frames: usize) -> AudioBuffer {
        let samples_to_read = frames * self.channels as usize;
        let mut samples = vec![0.0; samples_to_read];
        let samples_read = self.read(&mut samples);

        samples.truncate(samples_read);
        let frames_read = samples_read / self.channels as usize;

        AudioBuffer {
            samples,
            channels: self.channels,
            sample_rate: self.sample_rate,
            frames: frames_read,
        }
    }

    /// Clear the buffer
    pub fn clear(&self) {
        self.read_pos.store(0, Ordering::Release);
        self.write_pos.store(0, Ordering::Release);
    }

    /// Get the current fill level as a percentage (0.0 to 1.0)
    pub fn fill_level(&self) -> f32 {
        self.available_read() as f32 / self.capacity as f32
    }

    /// Get the duration of audio currently in the buffer
    pub fn buffered_duration(&self) -> Duration {
        let frames = self.available_read_frames();
        if self.sample_rate > 0 {
            Duration::from_secs_f64(frames as f64 / self.sample_rate as f64)
        } else {
            Duration::from_secs(0)
        }
    }
}

/// Buffer manager for handling audio buffering and underrun detection
#[derive(Debug)]
pub struct BufferManager {
    ring_buffer: Arc<RingBuffer>,
    target_buffer_duration: Duration,
    min_buffer_duration: Duration,
    underrun_count: AtomicUsize,
    last_underrun: std::sync::Mutex<Option<Instant>>,
}

impl BufferManager {
    /// Create a new buffer manager
    pub fn new(
        capacity_frames: usize,
        channels: u16,
        sample_rate: u32,
        target_buffer_ms: u64,
        min_buffer_ms: u64,
    ) -> Self {
        let ring_buffer = Arc::new(RingBuffer::new(capacity_frames, channels, sample_rate));

        Self {
            ring_buffer,
            target_buffer_duration: Duration::from_millis(target_buffer_ms),
            min_buffer_duration: Duration::from_millis(min_buffer_ms),
            underrun_count: AtomicUsize::new(0),
            last_underrun: std::sync::Mutex::new(None),
        }
    }

    /// Get a reference to the ring buffer
    pub fn ring_buffer(&self) -> Arc<RingBuffer> {
        Arc::clone(&self.ring_buffer)
    }

    /// Check if buffer needs more data
    pub fn needs_data(&self) -> bool {
        self.ring_buffer.buffered_duration() < self.target_buffer_duration
    }

    /// Check for buffer underrun
    pub fn check_underrun(&self) -> bool {
        let buffered = self.ring_buffer.buffered_duration();
        if buffered < self.min_buffer_duration && !self.ring_buffer.is_empty() {
            self.record_underrun();
            true
        } else {
            false
        }
    }

    /// Record a buffer underrun
    fn record_underrun(&self) {
        self.underrun_count.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut last_underrun) = self.last_underrun.lock() {
            *last_underrun = Some(Instant::now());
        }
    }

    /// Get the total number of underruns
    pub fn underrun_count(&self) -> usize {
        self.underrun_count.load(Ordering::Relaxed)
    }

    /// Get the time of the last underrun
    pub fn last_underrun(&self) -> Option<Instant> {
        self.last_underrun.lock().ok().and_then(|guard| *guard)
    }

    /// Reset underrun statistics
    pub fn reset_underrun_stats(&self) {
        self.underrun_count.store(0, Ordering::Relaxed);
        if let Ok(mut last_underrun) = self.last_underrun.lock() {
            *last_underrun = None;
        }
    }

    /// Get buffer status information
    pub fn buffer_status(&self) -> BufferStatus {
        BufferStatus {
            fill_level: self.ring_buffer.fill_level(),
            buffered_duration: self.ring_buffer.buffered_duration(),
            available_frames: self.ring_buffer.available_read_frames(),
            capacity_frames: self.ring_buffer.capacity_frames(),
            underrun_count: self.underrun_count(),
            needs_data: self.needs_data(),
            is_underrun: self.check_underrun(),
        }
    }

    /// Attempt to recover from underrun by clearing and requesting more data
    pub fn recover_from_underrun(&self) -> Result<(), AudioError> {
        // Clear the buffer to start fresh
        self.ring_buffer.clear();

        // Reset underrun stats for this recovery attempt
        self.reset_underrun_stats();

        Ok(())
    }
}

/// Buffer status information
#[derive(Debug, Clone)]
pub struct BufferStatus {
    pub fill_level: f32,
    pub buffered_duration: Duration,
    pub available_frames: usize,
    pub capacity_frames: usize,
    pub underrun_count: usize,
    pub needs_data: bool,
    pub is_underrun: bool,
}

impl BufferStatus {
    /// Check if the buffer is healthy (not underrunning and has sufficient data)
    pub fn is_healthy(&self) -> bool {
        !self.is_underrun && self.fill_level > 0.1 // At least 10% full
    }

    /// Get a human-readable status description
    pub fn status_description(&self) -> String {
        if self.is_underrun {
            "Buffer underrun detected".to_string()
        } else if self.needs_data {
            "Buffer needs more data".to_string()
        } else if self.fill_level > 0.8 {
            "Buffer is well-filled".to_string()
        } else {
            "Buffer is normal".to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::sync::Arc;

    #[test]
    fn test_ring_buffer_creation() {
        let buffer = RingBuffer::new(1024, 2, 44100);

        assert_eq!(buffer.capacity(), 2048); // 1024 frames * 2 channels
        assert_eq!(buffer.capacity_frames(), 1024);
        assert!(buffer.is_empty());
        assert!(!buffer.is_full());
        assert_eq!(buffer.available_read(), 0);
        assert_eq!(buffer.available_write(), 2047); // capacity - 1
    }

    #[test]
    fn test_ring_buffer_write_read() {
        let buffer = RingBuffer::new(100, 2, 44100);
        let test_data = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0]; // 3 frames

        // Write data
        let written = buffer.write(&test_data);
        assert_eq!(written, 6);
        assert_eq!(buffer.available_read(), 6);
        assert_eq!(buffer.available_read_frames(), 3);

        // Read data back
        let mut read_data = vec![0.0; 6];
        let read = buffer.read(&mut read_data);
        assert_eq!(read, 6);
        assert_eq!(read_data, test_data);
        assert!(buffer.is_empty());
    }

    #[test]
    fn test_ring_buffer_wrap_around() {
        let buffer = RingBuffer::new(4, 1, 44100); // 4 samples capacity

        // Fill buffer almost to capacity (leave 1 space for full detection)
        let data1 = vec![1.0, 2.0, 3.0]; // 3 samples
        let written1 = buffer.write(&data1);
        assert_eq!(written1, 3);

        // Read some data to make space
        let mut read_data = vec![0.0; 2];
        let read1 = buffer.read(&mut read_data);
        assert_eq!(read1, 2);
        assert_eq!(read_data, vec![1.0, 2.0]);

        // Write more data that will wrap around
        let data2 = vec![4.0, 5.0]; // 2 samples (can only fit 2 more due to capacity limit)
        let written2 = buffer.write(&data2);
        assert_eq!(written2, 2);

        // Read all remaining data
        let mut all_data = vec![0.0; 3];
        let read2 = buffer.read(&mut all_data);
        assert_eq!(read2, 3);
        assert_eq!(all_data, vec![3.0, 4.0, 5.0]);
    }

    #[test]
    fn test_ring_buffer_audio_buffer_integration() {
        let buffer = RingBuffer::new(100, 2, 44100);

        // Create an audio buffer
        let mut audio_buffer = AudioBuffer::new(2, 44100, 10);
        for i in 0..audio_buffer.samples.len() {
            audio_buffer.samples[i] = i as f32;
        }

        // Write audio buffer to ring buffer
        let frames_written = buffer.write_audio_buffer(&audio_buffer);
        assert_eq!(frames_written, 10);

        // Read back as audio buffer
        let read_buffer = buffer.read_audio_buffer(10);
        assert_eq!(read_buffer.frames, 10);
        assert_eq!(read_buffer.channels, 2);
        assert_eq!(read_buffer.samples, audio_buffer.samples);
    }

    #[test]
    fn test_ring_buffer_thread_safety() {
        let buffer = Arc::new(RingBuffer::new(1000, 2, 44100));
        let buffer_writer = Arc::clone(&buffer);
        let buffer_reader = Arc::clone(&buffer);

        let total_samples = 2000;

        // Writer thread
        let writer = thread::spawn(move || {
            let mut written_total = 0;
            for i in 0..100 {
                let data = vec![i as f32; 20]; // 10 frames
                let mut written = 0;
                while written < data.len() && written_total < total_samples {
                    let chunk_written = buffer_writer.write(&data[written..]);
                    written += chunk_written;
                    written_total += chunk_written;
                    if chunk_written == 0 {
                        thread::yield_now();
                    }
                }
                if written_total >= total_samples {
                    break;
                }
            }
        });

        // Reader thread
        let reader = thread::spawn(move || {
            let mut total_read = 0;
            while total_read < total_samples {
                let mut data = vec![0.0; 20];
                let read = buffer_reader.read(&mut data);
                total_read += read;
                if read == 0 {
                    thread::yield_now();
                }
            }
            total_read
        });

        writer.join().unwrap();
        let total_read = reader.join().unwrap();
        assert_eq!(total_read, total_samples);
    }

    #[test]
    fn test_buffer_manager_creation() {
        let manager = BufferManager::new(1024, 2, 44100, 100, 50);

        let status = manager.buffer_status();
        assert_eq!(status.capacity_frames, 1024);
        assert_eq!(status.available_frames, 0);
        assert_eq!(status.underrun_count, 0);
        assert!(status.needs_data);
        assert!(!status.is_underrun);
    }

    #[test]
    fn test_buffer_manager_underrun_detection() {
        let manager = BufferManager::new(1024, 2, 44100, 100, 50);
        let ring_buffer = manager.ring_buffer();

        // Add a small amount of data (less than min buffer duration)
        let small_data = vec![1.0; 100]; // Very small amount
        ring_buffer.write(&small_data);

        // Check for underrun
        let is_underrun = manager.check_underrun();
        assert!(is_underrun);
        assert_eq!(manager.underrun_count(), 1);
    }

    #[test]
    fn test_buffer_manager_recovery() {
        let manager = BufferManager::new(1024, 2, 44100, 100, 50);
        let ring_buffer = manager.ring_buffer();

        // Simulate underrun
        let small_data = vec![1.0; 100];
        ring_buffer.write(&small_data);
        manager.check_underrun();
        assert_eq!(manager.underrun_count(), 1);

        // Recover from underrun
        manager.recover_from_underrun().unwrap();
        assert_eq!(manager.underrun_count(), 0);
        assert!(ring_buffer.is_empty());
    }

    #[test]
    fn test_buffer_status() {
        // Create a buffer manager with a larger buffer to ensure we can meet the target duration
        let manager = BufferManager::new(10000, 2, 44100, 100, 50); // 10000 frames = ~227ms at 44.1kHz
        let ring_buffer = manager.ring_buffer();

        // Fill buffer with enough data to exceed target buffer duration (100ms)
        // At 44100 Hz, 100ms = 4410 samples for stereo
        let data = vec![1.0; 8820]; // 4410 frames = ~100ms of stereo audio
        let written = ring_buffer.write(&data);

        let status = manager.buffer_status();
        assert!(status.fill_level > 0.0);
        assert!(status.buffered_duration > Duration::from_secs(0));
        assert_eq!(status.available_frames, written / 2); // Convert samples to frames
        assert!(!status.needs_data); // Should have enough data now

        let description = status.status_description();
        assert!(!description.is_empty());
    }

    #[test]
    fn test_ring_buffer_fill_level() {
        let buffer = RingBuffer::new(100, 2, 44100); // 200 samples capacity

        assert_eq!(buffer.fill_level(), 0.0);

        // Fill half the buffer
        let data = vec![1.0; 100]; // 100 samples
        buffer.write(&data);
        assert!((buffer.fill_level() - 0.5).abs() < 0.01);

        // Fill completely
        let more_data = vec![1.0; 99]; // 99 more samples (199 total, leaving 1 for full detection)
        buffer.write(&more_data);
        assert!(buffer.fill_level() > 0.99);
    }

    #[test]
    fn test_ring_buffer_buffered_duration() {
        let buffer = RingBuffer::new(44100, 1, 44100); // 1 second capacity at 44.1kHz

        assert_eq!(buffer.buffered_duration(), Duration::from_secs(0));

        // Add 0.5 seconds of audio
        let data = vec![1.0; 22050];
        buffer.write(&data);

        let duration = buffer.buffered_duration();
        assert!((duration.as_secs_f64() - 0.5).abs() < 0.01);
    }
}
