use std::sync::Arc;
use std::time::{Duration, Instant};
use std::thread;

use crate::audio::{
    AudioPerformanceProfiler, HighResBufferAllocator, AudioMemoryManager,
    AudioBuffer, AudioDecoder, AudioMetadata
};
use crate::error::DecodeError;

/// Mock high-resolution audio decoder for performance testing
struct MockHighResDecoder {
    sample_rate: u32,
    bit_depth: u16,
    channels: u16,
    duration: Duration,
    current_position: Duration,
    decode_delay: Duration, // Simulated decode time
    metadata: AudioMetadata, // Store metadata to avoid temporary reference
}

impl MockHighResDecoder {
    fn new(sample_rate: u32, bit_depth: u16, channels: u16, decode_delay_ms: u64) -> Self {
        Self {
            sample_rate,
            bit_depth,
            channels,
            duration: Duration::from_secs(300), // 5 minutes
            current_position: Duration::ZERO,
            decode_delay: Duration::from_millis(decode_delay_ms),
            metadata: AudioMetadata::new(),
        }
    }

    fn is_high_resolution(&self) -> bool {
        self.bit_depth >= 24 || self.sample_rate >= 96000
    }
}

impl AudioDecoder for MockHighResDecoder {
    fn decode_next(&mut self) -> Result<Option<AudioBuffer>, DecodeError> {
        // Simulate decode work
        thread::sleep(self.decode_delay);
        
        if self.current_position >= self.duration {
            return Ok(None);
        }
        
        // Generate 100ms of audio data
        let frames_per_100ms = (self.sample_rate as f64 * 0.1) as usize;
        let total_samples = frames_per_100ms * self.channels as usize;
        
        let buffer = AudioBuffer {
            samples: vec![0.0; total_samples],
            channels: self.channels,
            sample_rate: self.sample_rate,
            frames: frames_per_100ms,
        };
        
        self.current_position += Duration::from_millis(100);
        Ok(Some(buffer))
    }

    fn seek(&mut self, position: Duration) -> Result<(), DecodeError> {
        self.current_position = position.min(self.duration);
        Ok(())
    }

    fn metadata(&self) -> &AudioMetadata {
        &self.metadata
    }

    fn duration(&self) -> Duration {
        self.duration
    }

    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    fn bit_depth(&self) -> u16 {
        self.bit_depth
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_high_res_decode_performance() {
        let profiler = Arc::new(AudioPerformanceProfiler::new());
        
        // Test different high-resolution formats
        let test_formats = [
            (96000, 24, 2, "96kHz/24-bit"),
            (192000, 24, 2, "192kHz/24-bit"),
            (192000, 32, 2, "192kHz/32-bit"),
            (384000, 32, 2, "384kHz/32-bit"),
        ];
        
        for (sample_rate, bit_depth, channels, description) in test_formats {
            println!("Testing {} format", description);
            
            let mut decoder = MockHighResDecoder::new(sample_rate, bit_depth, channels, 2); // 2ms decode time
            
            // Decode several buffers and measure performance
            let start_time = Instant::now();
            let mut decode_count = 0;
            
            for _ in 0..10 {
                let decode_profiler = profiler.start_decode_profile();
                
                match decoder.decode_next() {
                    Ok(Some(_buffer)) => {
                        decode_count += 1;
                        decode_profiler.finish(sample_rate, bit_depth);
                    }
                    Ok(None) => break,
                    Err(e) => panic!("Decode error: {}", e),
                }
            }
            
            let total_time = start_time.elapsed();
            let avg_decode_time = profiler.average_decode_time();
            
            println!("  Decoded {} buffers in {:?}", decode_count, total_time);
            println!("  Average decode time: {:?}", avg_decode_time);
            
            // Performance assertions
            assert!(decode_count > 0, "Should have decoded at least one buffer");
            assert!(avg_decode_time.as_millis() < 50, "Average decode time should be under 50ms for {}", description);
            
            // Check format-specific performance
            if let Some(stats) = profiler.sample_rate_stats(sample_rate) {
                // Note: stats.sample_count may be higher due to profiler being shared across test formats
                assert!(stats.sample_count >= decode_count, "Should have at least {} samples, got {}", decode_count, stats.sample_count);
                assert!(stats.max_decode_time.as_millis() < 200, "Max decode time should be under 200ms for {}", description);
            }
        }
        
        // Generate performance report
        let report = profiler.performance_report();
        println!("\n{}", report.format_report());
        
        // Verify overall performance health
        assert!(profiler.is_performance_healthy(), "Performance should be healthy after test");
    }

    #[test]
    fn test_memory_usage_high_res_files() {
        let allocator = Arc::new(HighResBufferAllocator::new());
        
        // Test memory allocation for different high-res formats
        let test_cases = [
            (96000, 24, 2, 1000, "96kHz/24-bit 1s"),
            (192000, 24, 2, 1000, "192kHz/24-bit 1s"),
            (192000, 32, 2, 1000, "192kHz/32-bit 1s"),
            (384000, 32, 8, 1000, "384kHz/32-bit 8ch 1s"), // Surround sound
        ];
        
        let mut allocated_buffers = Vec::new();
        
        for (sample_rate, bit_depth, channels, duration_ms, description) in test_cases {
            println!("Testing memory allocation for {}", description);
            
            let buffer = allocator.allocate_for_format(sample_rate, bit_depth, channels, duration_ms)
                .expect("Should allocate buffer successfully");
            
            let expected_samples = (sample_rate as f64 * (duration_ms as f64 / 1000.0)) as usize;
            let expected_size = expected_samples * channels as usize * (bit_depth as usize / 8);
            
            println!("  Buffer size: {} bytes (expected ~{})", buffer.size(), expected_size);
            
            // Verify buffer is large enough
            assert!(buffer.size() >= expected_size, "Buffer should be large enough for format");
            
            // Test buffer operations
            let mut buffer = buffer;
            buffer.zero();
            
            // Verify buffer is properly aligned and accessible
            let f32_slice = buffer.as_f32_mut_slice();
            f32_slice[0] = 1.0;
            assert_eq!(buffer.as_f32_slice()[0], 1.0);
            
            allocated_buffers.push(buffer);
        }
        
        // Check memory statistics
        let stats = allocator.memory_stats();
        println!("\n{}", stats.format_stats());
        
        assert!(stats.current_usage > 0, "Should have allocated memory");
        assert!(stats.allocation_count > 0, "Should have performed allocations");
        
        // Test memory optimization
        allocator.optimize();
        let optimized_stats = allocator.memory_stats();
        
        // Memory usage should remain reasonable
        assert!(optimized_stats.current_usage <= stats.peak_usage, "Memory usage should not exceed peak");
    }

    #[test]
    fn test_cpu_usage_monitoring() {
        let profiler = Arc::new(AudioPerformanceProfiler::new());
        
        // Simulate CPU-intensive audio processing
        let start_time = Instant::now();
        
        while start_time.elapsed() < Duration::from_millis(500) {
            // Update CPU monitoring
            profiler.update_cpu_usage();
            
            // Simulate some work
            let _: Vec<f32> = (0..1000).map(|i| (i as f32).sin()).collect();
            
            thread::sleep(Duration::from_millis(10));
        }
        
        let avg_cpu = profiler.average_cpu_usage();
        let current_cpu = profiler.current_cpu_usage();
        
        println!("Average CPU usage: {:.1}%", avg_cpu);
        println!("Current CPU usage: {:.1}%", current_cpu);
        
        // CPU usage should be measurable
        assert!(avg_cpu >= 0.0, "CPU usage should be non-negative");
        assert!(current_cpu >= 0.0, "Current CPU usage should be non-negative");
        
        // Performance should be healthy for reasonable CPU usage
        if avg_cpu < 80.0 {
            assert!(profiler.is_performance_healthy(), "Performance should be healthy with low CPU usage");
        }
    }

    #[test]
    fn test_buffer_underrun_detection() {
        let profiler = Arc::new(AudioPerformanceProfiler::new());
        
        // Initially no underruns
        assert_eq!(profiler.buffer_underrun_count(), 0);
        assert!(profiler.is_performance_healthy());
        
        // Simulate some buffer underruns
        for i in 0..3 {
            profiler.record_buffer_underrun();
            println!("Recorded underrun #{}", i + 1);
        }
        
        assert_eq!(profiler.buffer_underrun_count(), 3);
        assert!(profiler.is_performance_healthy()); // Still healthy with few underruns
        
        // Simulate many underruns (performance degradation)
        for _ in 0..10 {
            profiler.record_buffer_underrun();
        }
        
        assert_eq!(profiler.buffer_underrun_count(), 13);
        assert!(!profiler.is_performance_healthy()); // Should be unhealthy now
        
        // Test buffer fill time recording
        profiler.record_buffer_fill_time(Duration::from_millis(5));
        profiler.record_buffer_fill_time(Duration::from_millis(3));
        profiler.record_buffer_fill_time(Duration::from_millis(7));
        
        let report = profiler.performance_report();
        assert_eq!(report.buffer_underruns, 13);
    }

    #[test]
    fn test_memory_pool_efficiency() {
        let manager = Arc::new(AudioMemoryManager::new());
        
        // Test repeated allocation and deallocation of same size
        let buffer_size = 8192;
        let num_iterations = 100;
        
        let start_time = Instant::now();
        
        for i in 0..num_iterations {
            let buffer = manager.allocate_buffer(buffer_size)
                .expect("Should allocate buffer");
            
            // Use the buffer briefly
            let mut buffer = buffer;
            buffer.zero();
            
            // Buffer is automatically returned to pool when dropped
            
            if i % 10 == 0 {
                println!("Completed {} allocations", i + 1);
            }
        }
        
        let total_time = start_time.elapsed();
        let avg_time_per_allocation = total_time / num_iterations;
        
        println!("Total time for {} allocations: {:?}", num_iterations, total_time);
        println!("Average time per allocation: {:?}", avg_time_per_allocation);
        
        // Pool should improve allocation performance
        assert!(avg_time_per_allocation.as_micros() < 100, "Average allocation time should be under 100μs");
        
        // Check pool statistics
        let pool_stats = manager.pool_stats();
        assert!(!pool_stats.is_empty(), "Should have pool statistics");
        
        let buffer_pool = pool_stats.iter().find(|s| s.buffer_size == buffer_size);
        assert!(buffer_pool.is_some(), "Should have pool for buffer size");
        
        let pool = buffer_pool.unwrap();
        assert!(pool.available_buffers > 0, "Should have buffers available in pool");
        
        println!("Pool stats: {} available, {} max, {} total allocated", 
                 pool.available_buffers, pool.max_buffers, pool.total_allocated);
    }

    #[test]
    fn test_performance_regression_detection() {
        let profiler = Arc::new(AudioPerformanceProfiler::new());
        
        // Establish baseline performance
        let mut decoder = MockHighResDecoder::new(192000, 24, 2, 1); // 1ms decode time
        
        // Record baseline performance
        for _ in 0..10 {
            let decode_profiler = profiler.start_decode_profile();
            let _ = decoder.decode_next().unwrap();
            decode_profiler.finish(192000, 24);
        }
        
        let baseline_avg = profiler.average_decode_time();
        println!("Baseline average decode time: {:?}", baseline_avg);
        
        // Simulate performance regression
        let mut slow_decoder = MockHighResDecoder::new(192000, 24, 2, 8); // 8ms decode time
        
        for _ in 0..5 {
            let decode_profiler = profiler.start_decode_profile();
            let _ = slow_decoder.decode_next().unwrap();
            decode_profiler.finish(192000, 24);
        }
        
        let regressed_avg = profiler.average_decode_time();
        println!("After regression average decode time: {:?}", regressed_avg);
        
        // Should detect performance regression
        assert!(regressed_avg > baseline_avg, "Should detect performance regression");
        assert!(!profiler.is_performance_healthy(), "Should detect unhealthy performance");
        
        // Check that high-res specific metrics show the regression
        let high_res_times = profiler.high_res_decode_performance();
        let recent_times: Vec<_> = high_res_times.iter().rev().take(5).collect();
        
        for &time in &recent_times {
            assert!(time.as_millis() > 5, "Recent decode times should show regression");
        }
    }

    #[test]
    fn test_concurrent_performance_monitoring() {
        let profiler = Arc::new(AudioPerformanceProfiler::new());
        
        // Simulate concurrent decode operations
        let handles: Vec<_> = (0..4).map(|thread_id| {
            let profiler = Arc::clone(&profiler);
            thread::spawn(move || {
                let mut decoder = MockHighResDecoder::new(96000, 24, 2, 2);
                
                for i in 0..25 {
                    let decode_profiler = profiler.start_decode_profile();
                    let _ = decoder.decode_next().unwrap();
                    decode_profiler.finish(96000, 24);
                    
                    if i % 10 == 0 {
                        println!("Thread {} completed {} decodes", thread_id, i + 1);
                    }
                    
                    thread::sleep(Duration::from_millis(1));
                }
            })
        }).collect();
        
        // Wait for all threads to complete
        for handle in handles {
            handle.join().unwrap();
        }
        
        // Check that all operations were recorded
        let report = profiler.performance_report();
        assert_eq!(report.total_decodes, 100); // 4 threads * 25 decodes each
        
        println!("Concurrent test results:");
        println!("{}", report.format_report());
        
        // Performance should still be healthy with concurrent access
        assert!(profiler.is_performance_healthy(), "Performance should be healthy with concurrent access");
    }

    #[test]
    fn test_memory_leak_detection() {
        let allocator = Arc::new(HighResBufferAllocator::new());
        
        let initial_stats = allocator.memory_stats();
        let initial_usage = initial_stats.current_usage;
        
        // Allocate and immediately drop many buffers
        for i in 0..100 {
            let _buffer = allocator.allocate_for_format(48000, 16, 2, 100)
                .expect("Should allocate buffer");
            
            if i % 20 == 0 {
                let current_stats = allocator.memory_stats();
                println!("Iteration {}: {} bytes allocated", i, current_stats.current_usage);
            }
        }
        
        // Force garbage collection and optimization
        allocator.optimize();
        
        let final_stats = allocator.memory_stats();
        let final_usage = final_stats.current_usage;
        
        println!("Initial usage: {} bytes", initial_usage);
        println!("Final usage: {} bytes", final_usage);
        println!("Peak usage: {} bytes", final_stats.peak_usage);
        
        // Memory usage should return to reasonable levels (allowing for pooled buffers)
        let usage_increase = final_usage.saturating_sub(initial_usage);
        let reasonable_overhead = 1024 * 1024; // 1MB overhead is reasonable for pools
        
        assert!(usage_increase < reasonable_overhead, 
                "Memory usage increase ({} bytes) should be reasonable", usage_increase);
        
        // Peak usage should be recorded
        assert!(final_stats.peak_usage >= final_usage, "Peak usage should be at least current usage");
    }
}