use log::{info, warn, error, debug, trace};
use std::time::{Duration, Instant};
use std::sync::{Arc, Mutex};
use std::collections::VecDeque;
use chrono::{DateTime, Utc};

/// Performance metrics collector for audio operations
#[derive(Debug, Clone)]
pub struct PerformanceMetrics {
    pub decode_time: Duration,
    pub buffer_fill_time: Duration,
    pub seek_time: Duration,
    pub device_switch_time: Duration,
    pub file_load_time: Duration,
}

impl PerformanceMetrics {
    pub fn new() -> Self {
        Self {
            decode_time: Duration::ZERO,
            buffer_fill_time: Duration::ZERO,
            seek_time: Duration::ZERO,
            device_switch_time: Duration::ZERO,
            file_load_time: Duration::ZERO,
        }
    }
}

/// Audio event for logging and debugging
#[derive(Debug, Clone)]
pub struct AudioEvent {
    pub timestamp: DateTime<Utc>,
    pub event_type: AudioEventType,
    pub duration: Option<Duration>,
    pub details: String,
}

#[derive(Debug, Clone)]
pub enum AudioEventType {
    PlaybackStarted,
    PlaybackPaused,
    PlaybackStopped,
    TrackChanged,
    DeviceChanged,
    BufferUnderrun,
    SeekOperation,
    DecodeError,
    StreamError,
    PerformanceWarning,
}

impl AudioEventType {
    pub fn as_str(&self) -> &'static str {
        match self {
            AudioEventType::PlaybackStarted => "PLAYBACK_STARTED",
            AudioEventType::PlaybackPaused => "PLAYBACK_PAUSED",
            AudioEventType::PlaybackStopped => "PLAYBACK_STOPPED",
            AudioEventType::TrackChanged => "TRACK_CHANGED",
            AudioEventType::DeviceChanged => "DEVICE_CHANGED",
            AudioEventType::BufferUnderrun => "BUFFER_UNDERRUN",
            AudioEventType::SeekOperation => "SEEK_OPERATION",
            AudioEventType::DecodeError => "DECODE_ERROR",
            AudioEventType::StreamError => "STREAM_ERROR",
            AudioEventType::PerformanceWarning => "PERFORMANCE_WARNING",
        }
    }
}

/// Logger for audio player operations and debugging
#[derive(Clone)]
pub struct AudioLogger {
    events: Arc<Mutex<VecDeque<AudioEvent>>>,
    max_events: usize,
    performance_metrics: Arc<Mutex<PerformanceMetrics>>,
}

impl AudioLogger {
    pub fn new() -> Self {
        Self {
            events: Arc::new(Mutex::new(VecDeque::new())),
            max_events: 1000, // Keep last 1000 events
            performance_metrics: Arc::new(Mutex::new(PerformanceMetrics::new())),
        }
    }

    /// Initialize logging system with appropriate log level
    pub fn init() -> Result<(), Box<dyn std::error::Error>> {
        // Set log level based on environment variable or default to Info
        let log_level = std::env::var("HIRES_PLAYER_LOG_LEVEL")
            .unwrap_or_else(|_| "info".to_string());

        let mut builder = env_logger::Builder::new();
        
        // Set custom format for better readability
        builder.format(|buf, record| {
            use std::io::Write;
            writeln!(
                buf,
                "{} [{}] [{}:{}] {}",
                chrono::Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"),
                record.level(),
                record.file().unwrap_or("unknown"),
                record.line().unwrap_or(0),
                record.args()
            )
        });

        // Parse and set log level
        match log_level.to_lowercase().as_str() {
            "trace" => builder.filter_level(log::LevelFilter::Trace),
            "debug" => builder.filter_level(log::LevelFilter::Debug),
            "info" => builder.filter_level(log::LevelFilter::Info),
            "warn" => builder.filter_level(log::LevelFilter::Warn),
            "error" => builder.filter_level(log::LevelFilter::Error),
            _ => builder.filter_level(log::LevelFilter::Info),
        };

        builder.try_init()?;
        
        info!("Audio player logging initialized with level: {}", log_level);
        Ok(())
    }

    /// Log an audio event
    pub fn log_event(&self, event_type: AudioEventType, details: String, duration: Option<Duration>) {
        let event = AudioEvent {
            timestamp: Utc::now(),
            event_type: event_type.clone(),
            duration,
            details: details.clone(),
        };

        // Add to event history
        {
            let mut events = self.events.lock().unwrap();
            events.push_back(event);
            
            // Keep only the last max_events
            while events.len() > self.max_events {
                events.pop_front();
            }
        }

        // Log to standard logger based on event type
        match event_type {
            AudioEventType::PlaybackStarted | 
            AudioEventType::PlaybackPaused | 
            AudioEventType::PlaybackStopped |
            AudioEventType::TrackChanged => {
                info!("[{}] {}", event_type.as_str(), details);
            }
            AudioEventType::DeviceChanged => {
                info!("[{}] {}", event_type.as_str(), details);
            }
            AudioEventType::SeekOperation => {
                debug!("[{}] {} (took: {:?})", event_type.as_str(), details, duration);
            }
            AudioEventType::BufferUnderrun => {
                warn!("[{}] {}", event_type.as_str(), details);
            }
            AudioEventType::DecodeError | 
            AudioEventType::StreamError => {
                error!("[{}] {}", event_type.as_str(), details);
            }
            AudioEventType::PerformanceWarning => {
                warn!("[{}] {} (duration: {:?})", event_type.as_str(), details, duration);
            }
        }
    }

    /// Log playback started event
    pub fn log_playback_started(&self, track_path: &str, format_info: &str) {
        self.log_event(
            AudioEventType::PlaybackStarted,
            format!("Started playing: {} ({})", track_path, format_info),
            None,
        );
    }

    /// Log playback paused event
    pub fn log_playback_paused(&self, position: Duration) {
        self.log_event(
            AudioEventType::PlaybackPaused,
            format!("Playback paused at position: {:.2}s", position.as_secs_f64()),
            None,
        );
    }

    /// Log playback stopped event
    pub fn log_playback_stopped(&self, reason: &str) {
        self.log_event(
            AudioEventType::PlaybackStopped,
            format!("Playback stopped: {}", reason),
            None,
        );
    }

    /// Log track change event
    pub fn log_track_changed(&self, from_track: Option<&str>, to_track: &str) {
        let details = match from_track {
            Some(from) => format!("Track changed from '{}' to '{}'", from, to_track),
            None => format!("Track loaded: '{}'", to_track),
        };
        self.log_event(AudioEventType::TrackChanged, details, None);
    }

    /// Log device change event
    pub fn log_device_changed(&self, from_device: Option<&str>, to_device: &str, switch_time: Duration) {
        let details = match from_device {
            Some(from) => format!("Audio device changed from '{}' to '{}'", from, to_device),
            None => format!("Audio device set to '{}'", to_device),
        };
        self.log_event(AudioEventType::DeviceChanged, details, Some(switch_time));
        
        // Update performance metrics
        {
            let mut metrics = self.performance_metrics.lock().unwrap();
            metrics.device_switch_time = switch_time;
        }
    }

    /// Log buffer underrun event
    pub fn log_buffer_underrun(&self, buffer_level: f32, recovery_time: Duration) {
        self.log_event(
            AudioEventType::BufferUnderrun,
            format!("Buffer underrun detected (level: {:.1}%, recovered in: {:.2}ms)", 
                buffer_level * 100.0, recovery_time.as_millis()),
            Some(recovery_time),
        );
    }

    /// Log seek operation
    pub fn log_seek_operation(&self, from_position: Duration, to_position: Duration, seek_time: Duration) {
        self.log_event(
            AudioEventType::SeekOperation,
            format!("Seek from {:.2}s to {:.2}s", 
                from_position.as_secs_f64(), to_position.as_secs_f64()),
            Some(seek_time),
        );
        
        // Update performance metrics
        {
            let mut metrics = self.performance_metrics.lock().unwrap();
            metrics.seek_time = seek_time;
        }
    }

    /// Log decode error
    pub fn log_decode_error(&self, file_path: &str, error: &str) {
        self.log_event(
            AudioEventType::DecodeError,
            format!("Decode error for '{}': {}", file_path, error),
            None,
        );
    }

    /// Log stream error
    pub fn log_stream_error(&self, error: &str, recovery_attempted: bool) {
        self.log_event(
            AudioEventType::StreamError,
            format!("Stream error: {} (recovery attempted: {})", error, recovery_attempted),
            None,
        );
    }

    /// Log performance warning
    pub fn log_performance_warning(&self, operation: &str, duration: Duration, threshold: Duration) {
        self.log_event(
            AudioEventType::PerformanceWarning,
            format!("{} took {:.2}ms (threshold: {:.2}ms)", 
                operation, duration.as_millis(), threshold.as_millis()),
            Some(duration),
        );
    }

    /// Update decode performance metrics
    pub fn update_decode_metrics(&self, decode_time: Duration) {
        let mut metrics = self.performance_metrics.lock().unwrap();
        metrics.decode_time = decode_time;
        
        // Log warning if decode time is too high
        let threshold = Duration::from_millis(100); // 100ms threshold
        if decode_time > threshold {
            drop(metrics); // Release lock before logging
            self.log_performance_warning("Audio decode", decode_time, threshold);
        }
    }

    /// Update buffer fill performance metrics
    pub fn update_buffer_fill_metrics(&self, fill_time: Duration) {
        let mut metrics = self.performance_metrics.lock().unwrap();
        metrics.buffer_fill_time = fill_time;
        
        // Log warning if buffer fill time is too high
        let threshold = Duration::from_millis(50); // 50ms threshold
        if fill_time > threshold {
            drop(metrics); // Release lock before logging
            self.log_performance_warning("Buffer fill", fill_time, threshold);
        }
    }

    /// Update file load performance metrics
    pub fn update_file_load_metrics(&self, load_time: Duration) {
        let mut metrics = self.performance_metrics.lock().unwrap();
        metrics.file_load_time = load_time;
        
        // Log warning if file load time is too high
        let threshold = Duration::from_millis(200); // 200ms threshold
        if load_time > threshold {
            drop(metrics); // Release lock before logging
            self.log_performance_warning("File load", load_time, threshold);
        }
    }

    /// Get recent events for debugging
    pub fn get_recent_events(&self, count: usize) -> Vec<AudioEvent> {
        let events = self.events.lock().unwrap();
        events.iter()
            .rev()
            .take(count)
            .cloned()
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect()
    }

    /// Get current performance metrics
    pub fn get_performance_metrics(&self) -> PerformanceMetrics {
        self.performance_metrics.lock().unwrap().clone()
    }

    /// Clear event history
    pub fn clear_events(&self) {
        let mut events = self.events.lock().unwrap();
        events.clear();
    }

    /// Get event statistics
    pub fn get_event_statistics(&self) -> EventStatistics {
        let events = self.events.lock().unwrap();
        let mut stats = EventStatistics::new();
        
        for event in events.iter() {
            match event.event_type {
                AudioEventType::BufferUnderrun => stats.buffer_underruns += 1,
                AudioEventType::DecodeError => stats.decode_errors += 1,
                AudioEventType::StreamError => stats.stream_errors += 1,
                AudioEventType::SeekOperation => stats.seek_operations += 1,
                AudioEventType::DeviceChanged => stats.device_changes += 1,
                _ => {}
            }
        }
        
        stats.total_events = events.len();
        stats
    }
}

/// Statistics about logged events
#[derive(Debug, Clone)]
pub struct EventStatistics {
    pub total_events: usize,
    pub buffer_underruns: usize,
    pub decode_errors: usize,
    pub stream_errors: usize,
    pub seek_operations: usize,
    pub device_changes: usize,
}

impl EventStatistics {
    pub fn new() -> Self {
        Self {
            total_events: 0,
            buffer_underruns: 0,
            decode_errors: 0,
            stream_errors: 0,
            seek_operations: 0,
            device_changes: 0,
        }
    }
}

/// Timer utility for measuring operation durations
pub struct OperationTimer {
    start_time: Instant,
    operation_name: String,
}

impl OperationTimer {
    pub fn new(operation_name: String) -> Self {
        trace!("Starting operation: {}", operation_name);
        Self {
            start_time: Instant::now(),
            operation_name,
        }
    }

    pub fn elapsed(&self) -> Duration {
        self.start_time.elapsed()
    }

    pub fn finish(self) -> Duration {
        let duration = self.elapsed();
        trace!("Completed operation '{}' in {:.2}ms", self.operation_name, duration.as_millis());
        duration
    }

    pub fn finish_with_threshold(self, threshold: Duration) -> Duration {
        let duration = self.elapsed();
        if duration > threshold {
            warn!("Operation '{}' took {:.2}ms (threshold: {:.2}ms)", 
                self.operation_name, duration.as_millis(), threshold.as_millis());
        } else {
            debug!("Completed operation '{}' in {:.2}ms", self.operation_name, duration.as_millis());
        }
        duration
    }
}

/// Macro for timing operations
#[macro_export]
macro_rules! time_operation {
    ($name:expr, $code:block) => {{
        let timer = $crate::logging::OperationTimer::new($name.to_string());
        let result = $code;
        let _duration = timer.finish();
        result
    }};
}

/// Macro for timing operations with threshold warnings
#[macro_export]
macro_rules! time_operation_with_threshold {
    ($name:expr, $threshold:expr, $code:block) => {{
        let timer = $crate::logging::OperationTimer::new($name.to_string());
        let result = $code;
        let _duration = timer.finish_with_threshold($threshold);
        result
    }};
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn test_audio_logger_creation() {
        let logger = AudioLogger::new();
        assert_eq!(logger.max_events, 1000);
        
        let events = logger.get_recent_events(10);
        assert!(events.is_empty());
    }

    #[test]
    fn test_log_event() {
        let logger = AudioLogger::new();
        
        logger.log_event(
            AudioEventType::PlaybackStarted,
            "Test playback".to_string(),
            None,
        );
        
        let events = logger.get_recent_events(1);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].details, "Test playback");
        assert!(matches!(events[0].event_type, AudioEventType::PlaybackStarted));
    }

    #[test]
    fn test_event_history_limit() {
        let mut logger = AudioLogger::new();
        logger.max_events = 3; // Set small limit for testing
        
        // Add more events than the limit
        for i in 0..5 {
            logger.log_event(
                AudioEventType::PlaybackStarted,
                format!("Event {}", i),
                None,
            );
        }
        
        let events = logger.get_recent_events(10);
        assert_eq!(events.len(), 3); // Should only keep last 3 events
        assert_eq!(events[0].details, "Event 2");
        assert_eq!(events[2].details, "Event 4");
    }

    #[test]
    fn test_performance_metrics_update() {
        let logger = AudioLogger::new();
        
        let decode_time = Duration::from_millis(50);
        logger.update_decode_metrics(decode_time);
        
        let metrics = logger.get_performance_metrics();
        assert_eq!(metrics.decode_time, decode_time);
    }

    #[test]
    fn test_event_statistics() {
        let logger = AudioLogger::new();
        
        // Add various events
        logger.log_event(AudioEventType::BufferUnderrun, "Test".to_string(), None);
        logger.log_event(AudioEventType::BufferUnderrun, "Test".to_string(), None);
        logger.log_event(AudioEventType::DecodeError, "Test".to_string(), None);
        logger.log_event(AudioEventType::SeekOperation, "Test".to_string(), None);
        
        let stats = logger.get_event_statistics();
        assert_eq!(stats.total_events, 4);
        assert_eq!(stats.buffer_underruns, 2);
        assert_eq!(stats.decode_errors, 1);
        assert_eq!(stats.seek_operations, 1);
    }

    #[test]
    fn test_operation_timer() {
        let timer = OperationTimer::new("test_operation".to_string());
        
        // Simulate some work
        thread::sleep(Duration::from_millis(10));
        
        let duration = timer.finish();
        assert!(duration >= Duration::from_millis(10));
    }

    #[test]
    fn test_clear_events() {
        let logger = AudioLogger::new();
        
        logger.log_event(AudioEventType::PlaybackStarted, "Test".to_string(), None);
        assert_eq!(logger.get_recent_events(10).len(), 1);
        
        logger.clear_events();
        assert_eq!(logger.get_recent_events(10).len(), 0);
    }

    #[test]
    fn test_audio_event_type_as_str() {
        assert_eq!(AudioEventType::PlaybackStarted.as_str(), "PLAYBACK_STARTED");
        assert_eq!(AudioEventType::BufferUnderrun.as_str(), "BUFFER_UNDERRUN");
        assert_eq!(AudioEventType::DecodeError.as_str(), "DECODE_ERROR");
    }

    #[test]
    fn test_performance_metrics_new() {
        let metrics = PerformanceMetrics::new();
        assert_eq!(metrics.decode_time, Duration::ZERO);
        assert_eq!(metrics.buffer_fill_time, Duration::ZERO);
        assert_eq!(metrics.seek_time, Duration::ZERO);
        assert_eq!(metrics.device_switch_time, Duration::ZERO);
        assert_eq!(metrics.file_load_time, Duration::ZERO);
    }

    #[test]
    fn test_event_statistics_new() {
        let stats = EventStatistics::new();
        assert_eq!(stats.total_events, 0);
        assert_eq!(stats.buffer_underruns, 0);
        assert_eq!(stats.decode_errors, 0);
        assert_eq!(stats.stream_errors, 0);
        assert_eq!(stats.seek_operations, 0);
        assert_eq!(stats.device_changes, 0);
    }

    #[test]
    fn test_specific_log_methods() {
        let logger = AudioLogger::new();
        
        logger.log_playback_started("/test/file.flac", "FLAC 44.1kHz 16-bit");
        logger.log_playback_paused(Duration::from_secs(30));
        logger.log_playback_stopped("User requested");
        logger.log_track_changed(Some("/old/track.flac"), "/new/track.flac");
        logger.log_device_changed(Some("Old Device"), "New Device", Duration::from_millis(100));
        logger.log_buffer_underrun(0.1, Duration::from_millis(50));
        logger.log_seek_operation(Duration::from_secs(10), Duration::from_secs(20), Duration::from_millis(25));
        logger.log_decode_error("/bad/file.flac", "Corrupted data");
        logger.log_stream_error("Device disconnected", true);
        logger.log_performance_warning("Slow operation", Duration::from_millis(200), Duration::from_millis(100));
        
        let events = logger.get_recent_events(20);
        assert_eq!(events.len(), 10);
        
        // Check that all event types are represented
        let event_types: Vec<_> = events.iter().map(|e| e.event_type.as_str()).collect();
        assert!(event_types.contains(&"PLAYBACK_STARTED"));
        assert!(event_types.contains(&"PLAYBACK_PAUSED"));
        assert!(event_types.contains(&"PLAYBACK_STOPPED"));
        assert!(event_types.contains(&"TRACK_CHANGED"));
        assert!(event_types.contains(&"DEVICE_CHANGED"));
        assert!(event_types.contains(&"BUFFER_UNDERRUN"));
        assert!(event_types.contains(&"SEEK_OPERATION"));
        assert!(event_types.contains(&"DECODE_ERROR"));
        assert!(event_types.contains(&"STREAM_ERROR"));
        assert!(event_types.contains(&"PERFORMANCE_WARNING"));
    }
}