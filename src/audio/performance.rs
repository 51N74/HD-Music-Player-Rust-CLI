use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use std::collections::VecDeque;
use sysinfo::{System, SystemExt, ProcessExt, CpuExt};

/// Performance profiler for high-resolution audio processing
#[derive(Debug)]
pub struct AudioPerformanceProfiler {
    // CPU monitoring
    system: Arc<Mutex<System>>,
    cpu_samples: Arc<Mutex<VecDeque<f32>>>,
    max_cpu_samples: usize,
    
    // Memory tracking
    memory_usage: AtomicU64,
    peak_memory_usage: AtomicU64,
    
    // Decode performance
    decode_times: Arc<Mutex<VecDeque<Duration>>>,
    total_decode_time: AtomicU64,
    decode_count: AtomicUsize,
    
    // Buffer performance
    buffer_underruns: AtomicUsize,
    buffer_fill_times: Arc<Mutex<VecDeque<Duration>>>,
    
    // High-res specific metrics
    high_res_decode_times: Arc<Mutex<VecDeque<Duration>>>,
    sample_rate_performance: Arc<Mutex<std::collections::HashMap<u32, PerformanceStats>>>,
    bit_depth_performance: Arc<Mutex<std::collections::HashMap<u16, PerformanceStats>>>,
}

/// Performance statistics for specific configurations
#[derive(Debug, Clone)]
pub struct PerformanceStats {
    pub avg_decode_time: Duration,
    pub max_decode_time: Duration,
    pub min_decode_time: Duration,
    pub sample_count: usize,
    pub total_time: Duration,
}

impl PerformanceStats {
    pub fn new() -> Self {
        Self {
            avg_decode_time: Duration::ZERO,
            max_decode_time: Duration::ZERO,
            min_decode_time: Duration::MAX,
            sample_count: 0,
            total_time: Duration::ZERO,
        }
    }

    pub fn update(&mut self, decode_time: Duration) {
        self.sample_count += 1;
        self.total_time += decode_time;
        self.avg_decode_time = self.total_time / self.sample_count as u32;
        
        if decode_time > self.max_decode_time {
            self.max_decode_time = decode_time;
        }
        
        if decode_time < self.min_decode_time {
            self.min_decode_time = decode_time;
        }
    }
}

impl AudioPerformanceProfiler {
    /// Create a new performance profiler
    pub fn new() -> Self {
        let mut system = System::new_all();
        system.refresh_all();
        
        Self {
            system: Arc::new(Mutex::new(system)),
            cpu_samples: Arc::new(Mutex::new(VecDeque::new())),
            max_cpu_samples: 100, // Keep last 100 CPU samples
            
            memory_usage: AtomicU64::new(0),
            peak_memory_usage: AtomicU64::new(0),
            
            decode_times: Arc::new(Mutex::new(VecDeque::new())),
            total_decode_time: AtomicU64::new(0),
            decode_count: AtomicUsize::new(0),
            
            buffer_underruns: AtomicUsize::new(0),
            buffer_fill_times: Arc::new(Mutex::new(VecDeque::new())),
            
            high_res_decode_times: Arc::new(Mutex::new(VecDeque::new())),
            sample_rate_performance: Arc::new(Mutex::new(std::collections::HashMap::new())),
            bit_depth_performance: Arc::new(Mutex::new(std::collections::HashMap::new())),
        }
    }

    /// Start profiling a decode operation
    pub fn start_decode_profile(&self) -> DecodeProfiler {
        DecodeProfiler {
            start_time: Instant::now(),
            profiler: self,
        }
    }

    /// Record decode performance for specific audio format
    pub fn record_decode_performance(
        &self,
        decode_time: Duration,
        sample_rate: u32,
        bit_depth: u16,
        is_high_res: bool,
    ) {
        // Update general decode metrics
        self.decode_count.fetch_add(1, Ordering::Relaxed);
        self.total_decode_time.fetch_add(decode_time.as_nanos() as u64, Ordering::Relaxed);
        
        // Store recent decode times
        {
            let mut decode_times = self.decode_times.lock().unwrap();
            decode_times.push_back(decode_time);
            if decode_times.len() > 100 {
                decode_times.pop_front();
            }
        }
        
        // Track high-resolution specific performance
        if is_high_res {
            let mut high_res_times = self.high_res_decode_times.lock().unwrap();
            high_res_times.push_back(decode_time);
            if high_res_times.len() > 50 {
                high_res_times.pop_front();
            }
        }
        
        // Update sample rate specific performance
        {
            let mut sample_rate_perf = self.sample_rate_performance.lock().unwrap();
            let stats = sample_rate_perf.entry(sample_rate).or_insert_with(PerformanceStats::new);
            stats.update(decode_time);
        }
        
        // Update bit depth specific performance
        {
            let mut bit_depth_perf = self.bit_depth_performance.lock().unwrap();
            let stats = bit_depth_perf.entry(bit_depth).or_insert_with(PerformanceStats::new);
            stats.update(decode_time);
        }
    }

    /// Update CPU usage monitoring
    pub fn update_cpu_usage(&self) {
        let mut system = self.system.lock().unwrap();
        system.refresh_cpu();
        
        // Get overall CPU usage
        let cpu_usage = system.global_cpu_info().cpu_usage();
        
        // Store CPU sample
        {
            let mut cpu_samples = self.cpu_samples.lock().unwrap();
            cpu_samples.push_back(cpu_usage);
            if cpu_samples.len() > self.max_cpu_samples {
                cpu_samples.pop_front();
            }
        }
    }

    /// Update memory usage tracking
    pub fn update_memory_usage(&self, current_usage: u64) {
        self.memory_usage.store(current_usage, Ordering::Relaxed);
        
        // Update peak memory usage
        let current_peak = self.peak_memory_usage.load(Ordering::Relaxed);
        if current_usage > current_peak {
            self.peak_memory_usage.store(current_usage, Ordering::Relaxed);
        }
    }

    /// Record buffer underrun
    pub fn record_buffer_underrun(&self) {
        self.buffer_underruns.fetch_add(1, Ordering::Relaxed);
    }

    /// Record buffer fill time
    pub fn record_buffer_fill_time(&self, fill_time: Duration) {
        let mut fill_times = self.buffer_fill_times.lock().unwrap();
        fill_times.push_back(fill_time);
        if fill_times.len() > 100 {
            fill_times.pop_front();
        }
    }

    /// Get current CPU usage percentage
    pub fn current_cpu_usage(&self) -> f32 {
        let cpu_samples = self.cpu_samples.lock().unwrap();
        cpu_samples.back().copied().unwrap_or(0.0)
    }

    /// Get average CPU usage over recent samples
    pub fn average_cpu_usage(&self) -> f32 {
        let cpu_samples = self.cpu_samples.lock().unwrap();
        if cpu_samples.is_empty() {
            return 0.0;
        }
        
        let sum: f32 = cpu_samples.iter().sum();
        sum / cpu_samples.len() as f32
    }

    /// Get current memory usage in bytes
    pub fn current_memory_usage(&self) -> u64 {
        self.memory_usage.load(Ordering::Relaxed)
    }

    /// Get peak memory usage in bytes
    pub fn peak_memory_usage(&self) -> u64 {
        self.peak_memory_usage.load(Ordering::Relaxed)
    }

    /// Get average decode time
    pub fn average_decode_time(&self) -> Duration {
        let count = self.decode_count.load(Ordering::Relaxed);
        if count == 0 {
            return Duration::ZERO;
        }
        
        let total_nanos = self.total_decode_time.load(Ordering::Relaxed);
        Duration::from_nanos(total_nanos / count as u64)
    }

    /// Get recent decode times
    pub fn recent_decode_times(&self) -> Vec<Duration> {
        self.decode_times.lock().unwrap().iter().copied().collect()
    }

    /// Get high-resolution decode performance
    pub fn high_res_decode_performance(&self) -> Vec<Duration> {
        self.high_res_decode_times.lock().unwrap().iter().copied().collect()
    }

    /// Get buffer underrun count
    pub fn buffer_underrun_count(&self) -> usize {
        self.buffer_underruns.load(Ordering::Relaxed)
    }

    /// Get performance statistics for a specific sample rate
    pub fn sample_rate_stats(&self, sample_rate: u32) -> Option<PerformanceStats> {
        self.sample_rate_performance.lock().unwrap().get(&sample_rate).cloned()
    }

    /// Get performance statistics for a specific bit depth
    pub fn bit_depth_stats(&self, bit_depth: u16) -> Option<PerformanceStats> {
        self.bit_depth_performance.lock().unwrap().get(&bit_depth).cloned()
    }

    /// Get comprehensive performance report
    pub fn performance_report(&self) -> PerformanceReport {
        let cpu_usage = self.average_cpu_usage();
        let memory_usage = self.current_memory_usage();
        let peak_memory = self.peak_memory_usage();
        let avg_decode_time = self.average_decode_time();
        let underrun_count = self.buffer_underrun_count();
        
        let high_res_times = self.high_res_decode_performance();
        let high_res_avg = if !high_res_times.is_empty() {
            high_res_times.iter().sum::<Duration>() / high_res_times.len() as u32
        } else {
            Duration::ZERO
        };
        
        // Get sample rate performance summary
        let sample_rate_perf = self.sample_rate_performance.lock().unwrap();
        let mut sample_rate_summary = Vec::new();
        for (&rate, stats) in sample_rate_perf.iter() {
            sample_rate_summary.push((rate, stats.clone()));
        }
        
        // Get bit depth performance summary
        let bit_depth_perf = self.bit_depth_performance.lock().unwrap();
        let mut bit_depth_summary = Vec::new();
        for (&depth, stats) in bit_depth_perf.iter() {
            bit_depth_summary.push((depth, stats.clone()));
        }
        
        PerformanceReport {
            cpu_usage_percent: cpu_usage,
            memory_usage_bytes: memory_usage,
            peak_memory_bytes: peak_memory,
            average_decode_time: avg_decode_time,
            high_res_average_decode_time: high_res_avg,
            buffer_underruns: underrun_count,
            total_decodes: self.decode_count.load(Ordering::Relaxed),
            sample_rate_performance: sample_rate_summary,
            bit_depth_performance: bit_depth_summary,
        }
    }

    /// Check if performance is within acceptable thresholds
    pub fn is_performance_healthy(&self) -> bool {
        let cpu_usage = self.average_cpu_usage();
        let recent_decode_times = self.recent_decode_times();
        
        // Check CPU usage (should be under 80% for real-time audio)
        if cpu_usage > 80.0 {
            return false;
        }
        
        // Check for consistent decode performance (no spikes over 10ms)
        for decode_time in recent_decode_times.iter().rev().take(10) {
            if decode_time.as_millis() > 10 {
                return false;
            }
        }
        
        // Check buffer underruns (should be minimal)
        let underruns = self.buffer_underrun_count();
        if underruns > 5 {
            return false;
        }
        
        true
    }

    /// Reset performance statistics
    pub fn reset_stats(&self) {
        self.decode_count.store(0, Ordering::Relaxed);
        self.total_decode_time.store(0, Ordering::Relaxed);
        self.buffer_underruns.store(0, Ordering::Relaxed);
        self.peak_memory_usage.store(0, Ordering::Relaxed);
        
        self.decode_times.lock().unwrap().clear();
        self.cpu_samples.lock().unwrap().clear();
        self.buffer_fill_times.lock().unwrap().clear();
        self.high_res_decode_times.lock().unwrap().clear();
        self.sample_rate_performance.lock().unwrap().clear();
        self.bit_depth_performance.lock().unwrap().clear();
    }
}

/// RAII profiler for decode operations
pub struct DecodeProfiler<'a> {
    start_time: Instant,
    profiler: &'a AudioPerformanceProfiler,
}

impl<'a> DecodeProfiler<'a> {
    /// Finish profiling and record results
    pub fn finish(self, sample_rate: u32, bit_depth: u16) {
        let decode_time = self.start_time.elapsed();
        let is_high_res = bit_depth >= 24 || sample_rate >= 96000;
        
        self.profiler.record_decode_performance(decode_time, sample_rate, bit_depth, is_high_res);
    }
}

/// Comprehensive performance report
#[derive(Debug, Clone)]
pub struct PerformanceReport {
    pub cpu_usage_percent: f32,
    pub memory_usage_bytes: u64,
    pub peak_memory_bytes: u64,
    pub average_decode_time: Duration,
    pub high_res_average_decode_time: Duration,
    pub buffer_underruns: usize,
    pub total_decodes: usize,
    pub sample_rate_performance: Vec<(u32, PerformanceStats)>,
    pub bit_depth_performance: Vec<(u16, PerformanceStats)>,
}

impl PerformanceReport {
    /// Format the report as a human-readable string
    pub fn format_report(&self) -> String {
        let mut report = String::new();
        
        report.push_str("=== Audio Performance Report ===\n");
        report.push_str(&format!("CPU Usage: {:.1}%\n", self.cpu_usage_percent));
        report.push_str(&format!("Memory Usage: {:.2} MB\n", self.memory_usage_bytes as f64 / 1024.0 / 1024.0));
        report.push_str(&format!("Peak Memory: {:.2} MB\n", self.peak_memory_bytes as f64 / 1024.0 / 1024.0));
        report.push_str(&format!("Average Decode Time: {:.2}ms\n", self.average_decode_time.as_millis()));
        report.push_str(&format!("High-Res Decode Time: {:.2}ms\n", self.high_res_average_decode_time.as_millis()));
        report.push_str(&format!("Buffer Underruns: {}\n", self.buffer_underruns));
        report.push_str(&format!("Total Decodes: {}\n", self.total_decodes));
        
        if !self.sample_rate_performance.is_empty() {
            report.push_str("\n--- Sample Rate Performance ---\n");
            for (rate, stats) in &self.sample_rate_performance {
                report.push_str(&format!(
                    "{} Hz: avg={:.2}ms, max={:.2}ms, samples={}\n",
                    rate,
                    stats.avg_decode_time.as_millis(),
                    stats.max_decode_time.as_millis(),
                    stats.sample_count
                ));
            }
        }
        
        if !self.bit_depth_performance.is_empty() {
            report.push_str("\n--- Bit Depth Performance ---\n");
            for (depth, stats) in &self.bit_depth_performance {
                report.push_str(&format!(
                    "{}-bit: avg={:.2}ms, max={:.2}ms, samples={}\n",
                    depth,
                    stats.avg_decode_time.as_millis(),
                    stats.max_decode_time.as_millis(),
                    stats.sample_count
                ));
            }
        }
        
        report
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn test_performance_profiler_creation() {
        let profiler = AudioPerformanceProfiler::new();
        
        assert_eq!(profiler.current_cpu_usage(), 0.0);
        assert_eq!(profiler.current_memory_usage(), 0);
        assert_eq!(profiler.average_decode_time(), Duration::ZERO);
        assert_eq!(profiler.buffer_underrun_count(), 0);
    }

    #[test]
    fn test_decode_profiling() {
        let profiler = AudioPerformanceProfiler::new();
        
        // Simulate decode operation
        {
            let decode_profiler = profiler.start_decode_profile();
            thread::sleep(Duration::from_millis(1)); // Simulate work
            decode_profiler.finish(44100, 16);
        }
        
        assert!(profiler.average_decode_time() > Duration::ZERO);
        assert_eq!(profiler.decode_count.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn test_high_res_tracking() {
        let profiler = AudioPerformanceProfiler::new();
        
        // Record high-res decode
        profiler.record_decode_performance(
            Duration::from_millis(5),
            192000, // High sample rate
            24,     // High bit depth
            true
        );
        
        let high_res_times = profiler.high_res_decode_performance();
        assert_eq!(high_res_times.len(), 1);
        assert_eq!(high_res_times[0], Duration::from_millis(5));
    }

    #[test]
    fn test_memory_tracking() {
        let profiler = AudioPerformanceProfiler::new();
        
        profiler.update_memory_usage(1024 * 1024); // 1MB
        assert_eq!(profiler.current_memory_usage(), 1024 * 1024);
        assert_eq!(profiler.peak_memory_usage(), 1024 * 1024);
        
        profiler.update_memory_usage(2 * 1024 * 1024); // 2MB
        assert_eq!(profiler.current_memory_usage(), 2 * 1024 * 1024);
        assert_eq!(profiler.peak_memory_usage(), 2 * 1024 * 1024);
        
        profiler.update_memory_usage(512 * 1024); // 512KB (lower)
        assert_eq!(profiler.current_memory_usage(), 512 * 1024);
        assert_eq!(profiler.peak_memory_usage(), 2 * 1024 * 1024); // Peak unchanged
    }

    #[test]
    fn test_buffer_underrun_tracking() {
        let profiler = AudioPerformanceProfiler::new();
        
        assert_eq!(profiler.buffer_underrun_count(), 0);
        
        profiler.record_buffer_underrun();
        profiler.record_buffer_underrun();
        
        assert_eq!(profiler.buffer_underrun_count(), 2);
    }

    #[test]
    fn test_sample_rate_performance_tracking() {
        let profiler = AudioPerformanceProfiler::new();
        
        // Record performance for different sample rates
        profiler.record_decode_performance(Duration::from_millis(2), 44100, 16, false);
        profiler.record_decode_performance(Duration::from_millis(4), 96000, 24, true);
        profiler.record_decode_performance(Duration::from_millis(3), 44100, 16, false);
        
        let stats_44k = profiler.sample_rate_stats(44100).unwrap();
        assert_eq!(stats_44k.sample_count, 2);
        assert_eq!(stats_44k.avg_decode_time, Duration::from_millis(2)); // (2+3)/2 = 2.5, rounded to 2
        
        let stats_96k = profiler.sample_rate_stats(96000).unwrap();
        assert_eq!(stats_96k.sample_count, 1);
        assert_eq!(stats_96k.avg_decode_time, Duration::from_millis(4));
    }

    #[test]
    fn test_performance_health_check() {
        let profiler = AudioPerformanceProfiler::new();
        
        // Initially should be healthy (no data)
        assert!(profiler.is_performance_healthy());
        
        // Add some good performance data
        profiler.record_decode_performance(Duration::from_millis(1), 44100, 16, false);
        profiler.record_decode_performance(Duration::from_millis(2), 44100, 16, false);
        
        // Should still be healthy
        assert!(profiler.is_performance_healthy());
        
        // Add many buffer underruns
        for _ in 0..10 {
            profiler.record_buffer_underrun();
        }
        
        // Should now be unhealthy due to underruns
        assert!(!profiler.is_performance_healthy());
    }

    #[test]
    fn test_performance_report() {
        let profiler = AudioPerformanceProfiler::new();
        
        // Add some test data
        profiler.update_memory_usage(1024 * 1024);
        profiler.record_decode_performance(Duration::from_millis(3), 44100, 16, false);
        profiler.record_decode_performance(Duration::from_millis(5), 192000, 24, true);
        profiler.record_buffer_underrun();
        
        let report = profiler.performance_report();
        
        assert_eq!(report.memory_usage_bytes, 1024 * 1024);
        assert_eq!(report.buffer_underruns, 1);
        assert_eq!(report.total_decodes, 2);
        assert!(report.average_decode_time > Duration::ZERO);
        assert!(report.high_res_average_decode_time > Duration::ZERO);
        
        // Test report formatting
        let formatted = report.format_report();
        assert!(formatted.contains("Audio Performance Report"));
        assert!(formatted.contains("CPU Usage"));
        assert!(formatted.contains("Memory Usage"));
    }

    #[test]
    fn test_stats_reset() {
        let profiler = AudioPerformanceProfiler::new();
        
        // Add some data
        profiler.record_decode_performance(Duration::from_millis(3), 44100, 16, false);
        profiler.record_buffer_underrun();
        profiler.update_memory_usage(1024);
        
        assert!(profiler.average_decode_time() > Duration::ZERO);
        assert_eq!(profiler.buffer_underrun_count(), 1);
        
        // Reset stats
        profiler.reset_stats();
        
        assert_eq!(profiler.average_decode_time(), Duration::ZERO);
        assert_eq!(profiler.buffer_underrun_count(), 0);
        assert_eq!(profiler.decode_count.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn test_performance_stats_update() {
        let mut stats = PerformanceStats::new();
        
        assert_eq!(stats.sample_count, 0);
        assert_eq!(stats.avg_decode_time, Duration::ZERO);
        
        stats.update(Duration::from_millis(5));
        assert_eq!(stats.sample_count, 1);
        assert_eq!(stats.avg_decode_time, Duration::from_millis(5));
        assert_eq!(stats.max_decode_time, Duration::from_millis(5));
        assert_eq!(stats.min_decode_time, Duration::from_millis(5));
        
        stats.update(Duration::from_millis(3));
        assert_eq!(stats.sample_count, 2);
        assert_eq!(stats.avg_decode_time, Duration::from_millis(4)); // (5+3)/2
        assert_eq!(stats.max_decode_time, Duration::from_millis(5));
        assert_eq!(stats.min_decode_time, Duration::from_millis(3));
    }
}