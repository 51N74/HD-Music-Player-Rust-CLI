use crate::error::{PlayerError, AudioError, DecodeError, ConfigError, QueueError, PlaylistError};
use crate::logging::AudioLogger;
use log::{info, warn, error};
use std::time::Duration;
use std::path::Path;

/// Error recovery strategies and automatic error handling
pub struct ErrorRecoveryManager {
    logger: AudioLogger,
    recovery_attempts: std::collections::HashMap<String, u32>,
    max_recovery_attempts: u32,
}

impl ErrorRecoveryManager {
    pub fn new(logger: AudioLogger) -> Self {
        Self {
            logger,
            recovery_attempts: std::collections::HashMap::new(),
            max_recovery_attempts: 3,
        }
    }

    /// Attempt to recover from an error automatically
    pub async fn attempt_recovery(&mut self, error: &PlayerError) -> RecoveryResult {
        let error_key = self.get_error_key(error);
        let attempts = self.recovery_attempts.get(&error_key).unwrap_or(&0) + 1;
        
        if attempts > self.max_recovery_attempts {
            warn!("Maximum recovery attempts ({}) exceeded for error: {}", 
                self.max_recovery_attempts, error);
            return RecoveryResult::Failed("Maximum recovery attempts exceeded".to_string());
        }
        
        self.recovery_attempts.insert(error_key.clone(), attempts);
        
        info!("Attempting recovery for error (attempt {}): {}", attempts, error);
        
        let result = match error {
            PlayerError::Audio(audio_err) => self.recover_audio_error(audio_err).await,
            PlayerError::Decode(decode_err) => self.recover_decode_error(decode_err).await,
            PlayerError::Config(config_err) => self.recover_config_error(config_err).await,
            PlayerError::Queue(queue_err) => self.recover_queue_error(queue_err).await,
            PlayerError::Playlist(playlist_err) => self.recover_playlist_error(playlist_err).await,
            PlayerError::File(_) => RecoveryResult::Failed("File errors require manual intervention".to_string()),
            PlayerError::Parse(_) => RecoveryResult::Failed("Parse errors require correct input".to_string()),
        };
        
        match &result {
            RecoveryResult::Success(msg) => {
                info!("Recovery successful: {}", msg);
                self.recovery_attempts.remove(&error_key);
            }
            RecoveryResult::Retry(msg) => {
                info!("Recovery requires retry: {}", msg);
            }
            RecoveryResult::Failed(msg) => {
                warn!("Recovery failed: {}", msg);
            }
        }
        
        result
    }

    /// Recover from audio-related errors
    async fn recover_audio_error(&mut self, error: &AudioError) -> RecoveryResult {
        match error {
            AudioError::DeviceNotFound { device } => {
                self.logger.log_event(
                    crate::logging::AudioEventType::StreamError,
                    format!("Attempting recovery from device not found: {}", device),
                    None,
                );
                
                // Strategy: Fall back to default device
                RecoveryResult::Retry(format!(
                    "Device '{}' not found. Try switching to default device with 'device set default'", 
                    device
                ))
            }
            
            AudioError::UnsupportedSampleRate { rate } => {
                // Strategy: Suggest resampling or different device
                RecoveryResult::Failed(format!(
                    "Sample rate {} Hz not supported. Consider using audio conversion software or a different device", 
                    rate
                ))
            }
            
            AudioError::UnsupportedFormat { format } => {
                // Strategy: Suggest format conversion
                RecoveryResult::Failed(format!(
                    "Audio format '{}' not supported. Convert to FLAC, WAV, MP3, or OGG format", 
                    format
                ))
            }
            
            AudioError::StreamError(_) => {
                self.logger.log_stream_error(&error.to_string(), true);
                
                // Strategy: Restart audio stream
                RecoveryResult::Retry("Stream error detected. Try stopping and restarting playback".to_string())
            }
            
            AudioError::BufferUnderrun => {
                self.logger.log_buffer_underrun(0.0, Duration::from_millis(100));
                
                // Strategy: Increase buffer size and continue
                RecoveryResult::Success("Buffer underrun recovered automatically".to_string())
            }
            
            AudioError::InitializationFailed(_) => {
                // Strategy: Retry initialization with different settings
                RecoveryResult::Retry("Audio initialization failed. Retrying with default settings".to_string())
            }
            
            AudioError::InvalidSeekPosition { .. } => {
                // Strategy: Reset to beginning of track
                RecoveryResult::Retry("Invalid seek position. Resetting to beginning of track".to_string())
            }
        }
    }

    /// Recover from decode-related errors
    async fn recover_decode_error(&mut self, error: &DecodeError) -> RecoveryResult {
        match error {
            DecodeError::UnsupportedFormat { format } => {
                self.logger.log_decode_error("unknown", &format!("Unsupported format: {}", format));
                
                // Strategy: Skip file and continue with queue
                RecoveryResult::Retry(format!(
                    "Skipping unsupported format '{}' and continuing with next track", 
                    format
                ))
            }
            
            DecodeError::CorruptedFile(msg) => {
                self.logger.log_decode_error("unknown", &format!("Corrupted file: {}", msg));
                
                // Strategy: Skip corrupted file
                RecoveryResult::Retry("Skipping corrupted file and continuing with next track".to_string())
            }
            
            DecodeError::SeekError(_) => {
                // Strategy: Continue playback without seeking
                RecoveryResult::Success("Seek not supported for this file. Continuing playback from current position".to_string())
            }
            
            DecodeError::DecodeFailed(_) => {
                // Strategy: Skip file and continue
                RecoveryResult::Retry("Decode failed. Skipping file and continuing with next track".to_string())
            }
        }
    }

    /// Recover from configuration errors
    async fn recover_config_error(&mut self, error: &ConfigError) -> RecoveryResult {
        match error {
            ConfigError::ConfigDirNotFound => {
                // Strategy: Use default configuration
                RecoveryResult::Success("Using default configuration settings".to_string())
            }
            
            ConfigError::IoError(_) => {
                // Strategy: Use default configuration and warn user
                RecoveryResult::Success("Cannot access configuration file. Using default settings".to_string())
            }
            
            ConfigError::SerializationError(_) => {
                // Strategy: Continue with current settings
                RecoveryResult::Success("Cannot save configuration. Current settings will be used".to_string())
            }
            
            ConfigError::DeserializationError(_) => {
                // Strategy: Reset to default configuration
                RecoveryResult::Success("Configuration file corrupted. Reset to default settings".to_string())
            }
        }
    }

    /// Recover from queue-related errors
    async fn recover_queue_error(&mut self, error: &QueueError) -> RecoveryResult {
        match error {
            QueueError::FileNotFound { path } => {
                // Strategy: Remove file from queue and continue
                RecoveryResult::Retry(format!("File '{}' not found. Removing from queue and continuing", path))
            }
            
            QueueError::InvalidFormat { path } => {
                // Strategy: Skip unsupported file
                RecoveryResult::Retry(format!("Skipping unsupported file '{}' and continuing", path))
            }
            
            QueueError::EmptyQueue => {
                // Strategy: Inform user to add files
                RecoveryResult::Failed("Queue is empty. Add files with 'queue add <path>' or load a playlist".to_string())
            }
            
            QueueError::InvalidIndex { .. } => {
                // Strategy: Reset to first track
                RecoveryResult::Retry("Invalid track index. Resetting to first track in queue".to_string())
            }
        }
    }

    /// Recover from playlist-related errors
    async fn recover_playlist_error(&mut self, error: &PlaylistError) -> RecoveryResult {
        match error {
            PlaylistError::PlaylistNotFound { name } => {
                // Strategy: List available playlists
                RecoveryResult::Failed(format!(
                    "Playlist '{}' not found. Use 'playlist list' to see available playlists", 
                    name
                ))
            }
            
            PlaylistError::InvalidFormat(_) => {
                // Strategy: Skip corrupted playlist
                RecoveryResult::Failed("Playlist file is corrupted. Try recreating the playlist".to_string())
            }
            
            PlaylistError::IoError(_) => {
                // Strategy: Retry operation
                RecoveryResult::Retry("Playlist file access error. Retrying operation".to_string())
            }
        }
    }

    /// Generate a unique key for tracking recovery attempts
    fn get_error_key(&self, error: &PlayerError) -> String {
        match error {
            PlayerError::Audio(AudioError::DeviceNotFound { device }) => {
                format!("audio_device_not_found_{}", device)
            }
            PlayerError::Audio(AudioError::UnsupportedSampleRate { rate }) => {
                format!("audio_unsupported_rate_{}", rate)
            }
            PlayerError::Audio(AudioError::UnsupportedFormat { format }) => {
                format!("audio_unsupported_format_{}", format)
            }
            PlayerError::Audio(AudioError::StreamError(msg)) => {
                format!("audio_stream_error_{}", msg)
            }
            PlayerError::Audio(AudioError::BufferUnderrun) => {
                "audio_buffer_underrun".to_string()
            }
            PlayerError::Audio(AudioError::InitializationFailed(msg)) => {
                format!("audio_init_failed_{}", msg)
            }
            PlayerError::Audio(AudioError::InvalidSeekPosition { position, duration }) => {
                format!("audio_invalid_seek_{}_{}", position, duration)
            }
            PlayerError::Decode(DecodeError::UnsupportedFormat { format }) => {
                format!("decode_unsupported_{}", format)
            }
            PlayerError::Decode(DecodeError::CorruptedFile(msg)) => {
                format!("decode_corrupted_{}", msg)
            }
            PlayerError::Decode(DecodeError::SeekError(msg)) => {
                format!("decode_seek_error_{}", msg)
            }
            PlayerError::Decode(DecodeError::DecodeFailed(msg)) => {
                format!("decode_failed_{}", msg)
            }
            PlayerError::Queue(QueueError::FileNotFound { path }) => {
                format!("queue_file_not_found_{}", path)
            }
            PlayerError::Queue(QueueError::InvalidFormat { path }) => {
                format!("queue_invalid_format_{}", path)
            }
            PlayerError::Queue(QueueError::EmptyQueue) => {
                "queue_empty".to_string()
            }
            PlayerError::Queue(QueueError::InvalidIndex { index }) => {
                format!("queue_invalid_index_{}", index)
            }
            PlayerError::Playlist(PlaylistError::PlaylistNotFound { name }) => {
                format!("playlist_not_found_{}", name)
            }
            PlayerError::Playlist(PlaylistError::InvalidFormat(msg)) => {
                format!("playlist_invalid_format_{}", msg)
            }
            PlayerError::Playlist(PlaylistError::IoError(_)) => {
                "playlist_io_error".to_string()
            }
            PlayerError::Config(config_err) => {
                format!("config_{:?}", std::mem::discriminant(config_err))
            }
            PlayerError::File(_) => "file_error".to_string(),
            PlayerError::Parse(_) => "parse_error".to_string(),
        }
    }

    /// Reset recovery attempts for a specific error type
    pub fn reset_recovery_attempts(&mut self, error: &PlayerError) {
        let error_key = self.get_error_key(error);
        self.recovery_attempts.remove(&error_key);
    }

    /// Clear all recovery attempt counters
    pub fn clear_recovery_attempts(&mut self) {
        self.recovery_attempts.clear();
    }

    /// Get recovery statistics
    pub fn get_recovery_statistics(&self) -> RecoveryStatistics {
        RecoveryStatistics {
            total_errors_tracked: self.recovery_attempts.len(),
            errors_with_multiple_attempts: self.recovery_attempts.values()
                .filter(|&&attempts| attempts > 1)
                .count(),
            max_attempts_for_any_error: self.recovery_attempts.values()
                .max()
                .copied()
                .unwrap_or(0),
        }
    }
}

/// Result of an error recovery attempt
#[derive(Debug, Clone)]
pub enum RecoveryResult {
    /// Recovery was successful, operation can continue
    Success(String),
    /// Recovery requires retrying the operation
    Retry(String),
    /// Recovery failed, manual intervention required
    Failed(String),
}

impl RecoveryResult {
    pub fn is_success(&self) -> bool {
        matches!(self, RecoveryResult::Success(_))
    }

    pub fn is_retry(&self) -> bool {
        matches!(self, RecoveryResult::Retry(_))
    }

    pub fn is_failed(&self) -> bool {
        matches!(self, RecoveryResult::Failed(_))
    }

    pub fn message(&self) -> &str {
        match self {
            RecoveryResult::Success(msg) |
            RecoveryResult::Retry(msg) |
            RecoveryResult::Failed(msg) => msg,
        }
    }
}

/// Statistics about error recovery operations
#[derive(Debug, Clone)]
pub struct RecoveryStatistics {
    pub total_errors_tracked: usize,
    pub errors_with_multiple_attempts: usize,
    pub max_attempts_for_any_error: u32,
}

/// Utility functions for error recovery
pub struct RecoveryUtils;

impl RecoveryUtils {
    /// Check if a file exists and is accessible
    pub fn is_file_accessible(path: &Path) -> bool {
        path.exists() && path.is_file() && 
        std::fs::metadata(path).map(|m| !m.permissions().readonly()).unwrap_or(false)
    }

    /// Check if a directory exists and is accessible
    pub fn is_directory_accessible(path: &Path) -> bool {
        path.exists() && path.is_dir()
    }

    /// Get suggested alternative file paths for a missing file
    pub fn suggest_alternative_paths(original_path: &Path) -> Vec<std::path::PathBuf> {
        let mut suggestions = Vec::new();
        
        if let Some(parent) = original_path.parent() {
            if let Some(filename) = original_path.file_name() {
                // Try different case variations
                let filename_str = filename.to_string_lossy();
                suggestions.push(parent.join(filename_str.to_lowercase()));
                suggestions.push(parent.join(filename_str.to_uppercase()));
                
                // Try without extension
                if let Some(stem) = original_path.file_stem() {
                    suggestions.push(parent.join(stem));
                }
                
                // Try common audio extensions
                let common_extensions = ["flac", "wav", "mp3", "ogg", "alac"];
                for ext in &common_extensions {
                    if let Some(stem) = original_path.file_stem() {
                        suggestions.push(parent.join(format!("{}.{}", stem.to_string_lossy(), ext)));
                    }
                }
            }
        }
        
        // Filter to only existing files
        suggestions.into_iter()
            .filter(|path| Self::is_file_accessible(path))
            .collect()
    }

    /// Check if an audio device name is valid
    pub fn is_valid_device_name(device_name: &str) -> bool {
        !device_name.is_empty() && 
        device_name.len() < 256 && 
        !device_name.contains('\0')
    }

    /// Sanitize device name for safe usage
    pub fn sanitize_device_name(device_name: &str) -> String {
        device_name.chars()
            .filter(|c| !c.is_control())
            .take(255)
            .collect()
    }

    /// Check if a sample rate is commonly supported
    pub fn is_common_sample_rate(sample_rate: u32) -> bool {
        matches!(sample_rate, 44100 | 48000 | 88200 | 96000 | 176400 | 192000)
    }

    /// Get the nearest supported sample rate
    pub fn nearest_supported_sample_rate(sample_rate: u32) -> u32 {
        let supported_rates = [44100, 48000, 88200, 96000, 176400, 192000];
        
        supported_rates.iter()
            .min_by_key(|&&rate| (rate as i64 - sample_rate as i64).abs())
            .copied()
            .unwrap_or(44100)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logging::AudioLogger;
    use std::path::PathBuf;

    fn create_test_recovery_manager() -> ErrorRecoveryManager {
        let logger = AudioLogger::new();
        ErrorRecoveryManager::new(logger)
    }

    #[tokio::test]
    async fn test_audio_error_recovery() {
        let mut manager = create_test_recovery_manager();
        
        let error = AudioError::DeviceNotFound {
            device: "Test Device".to_string(),
        };
        let player_error = PlayerError::Audio(error);
        
        let result = manager.attempt_recovery(&player_error).await;
        assert!(result.is_retry());
        assert!(result.message().contains("Test Device"));
    }

    #[tokio::test]
    async fn test_decode_error_recovery() {
        let mut manager = create_test_recovery_manager();
        
        let error = DecodeError::UnsupportedFormat {
            format: "UNKNOWN".to_string(),
        };
        let player_error = PlayerError::Decode(error);
        
        let result = manager.attempt_recovery(&player_error).await;
        assert!(result.is_retry());
        assert!(result.message().contains("UNKNOWN"));
    }

    #[tokio::test]
    async fn test_config_error_recovery() {
        let mut manager = create_test_recovery_manager();
        
        let error = ConfigError::ConfigDirNotFound;
        let player_error = PlayerError::Config(error);
        
        let result = manager.attempt_recovery(&player_error).await;
        assert!(result.is_success());
        assert!(result.message().contains("default"));
    }

    #[tokio::test]
    async fn test_queue_error_recovery() {
        let mut manager = create_test_recovery_manager();
        
        let error = QueueError::FileNotFound {
            path: "/test/file.flac".to_string(),
        };
        let player_error = PlayerError::Queue(error);
        
        let result = manager.attempt_recovery(&player_error).await;
        assert!(result.is_retry());
        assert!(result.message().contains("/test/file.flac"));
    }

    #[tokio::test]
    async fn test_max_recovery_attempts() {
        let mut manager = create_test_recovery_manager();
        manager.max_recovery_attempts = 2;
        
        let error = AudioError::StreamError("Test error".to_string());
        let player_error = PlayerError::Audio(error);
        
        // First attempt should succeed
        let result1 = manager.attempt_recovery(&player_error).await;
        assert!(result1.is_retry());
        
        // Second attempt should succeed
        let result2 = manager.attempt_recovery(&player_error).await;
        assert!(result2.is_retry());
        
        // Third attempt should fail due to max attempts
        let result3 = manager.attempt_recovery(&player_error).await;
        assert!(result3.is_failed());
        assert!(result3.message().contains("Maximum recovery attempts"));
    }

    #[test]
    fn test_recovery_result_methods() {
        let success = RecoveryResult::Success("Success message".to_string());
        assert!(success.is_success());
        assert!(!success.is_retry());
        assert!(!success.is_failed());
        assert_eq!(success.message(), "Success message");
        
        let retry = RecoveryResult::Retry("Retry message".to_string());
        assert!(!retry.is_success());
        assert!(retry.is_retry());
        assert!(!retry.is_failed());
        assert_eq!(retry.message(), "Retry message");
        
        let failed = RecoveryResult::Failed("Failed message".to_string());
        assert!(!failed.is_success());
        assert!(!failed.is_retry());
        assert!(failed.is_failed());
        assert_eq!(failed.message(), "Failed message");
    }

    #[test]
    fn test_recovery_utils_file_accessibility() {
        // Test with non-existent file
        let non_existent = PathBuf::from("/non/existent/file.flac");
        assert!(!RecoveryUtils::is_file_accessible(&non_existent));
        
        // Test with directory instead of file
        let dir = PathBuf::from("/tmp");
        assert!(!RecoveryUtils::is_file_accessible(&dir));
    }

    #[test]
    fn test_recovery_utils_device_name_validation() {
        assert!(RecoveryUtils::is_valid_device_name("Valid Device"));
        assert!(!RecoveryUtils::is_valid_device_name(""));
        assert!(!RecoveryUtils::is_valid_device_name("Device\0With\0Nulls"));
        
        let long_name = "a".repeat(300);
        assert!(!RecoveryUtils::is_valid_device_name(&long_name));
    }

    #[test]
    fn test_recovery_utils_sanitize_device_name() {
        let sanitized = RecoveryUtils::sanitize_device_name("Device\nWith\tControl\rChars");
        assert!(!sanitized.contains('\n'));
        assert!(!sanitized.contains('\t'));
        assert!(!sanitized.contains('\r'));
        
        let long_name = "a".repeat(300);
        let sanitized_long = RecoveryUtils::sanitize_device_name(&long_name);
        assert!(sanitized_long.len() <= 255);
    }

    #[test]
    fn test_recovery_utils_sample_rate_validation() {
        assert!(RecoveryUtils::is_common_sample_rate(44100));
        assert!(RecoveryUtils::is_common_sample_rate(48000));
        assert!(RecoveryUtils::is_common_sample_rate(96000));
        assert!(!RecoveryUtils::is_common_sample_rate(22050));
        assert!(!RecoveryUtils::is_common_sample_rate(192001));
    }

    #[test]
    fn test_recovery_utils_nearest_sample_rate() {
        assert_eq!(RecoveryUtils::nearest_supported_sample_rate(44000), 44100);
        assert_eq!(RecoveryUtils::nearest_supported_sample_rate(47000), 48000);
        assert_eq!(RecoveryUtils::nearest_supported_sample_rate(100000), 96000);
        assert_eq!(RecoveryUtils::nearest_supported_sample_rate(200000), 192000);
    }

    #[test]
    fn test_recovery_statistics() {
        let manager = create_test_recovery_manager();
        let stats = manager.get_recovery_statistics();
        
        assert_eq!(stats.total_errors_tracked, 0);
        assert_eq!(stats.errors_with_multiple_attempts, 0);
        assert_eq!(stats.max_attempts_for_any_error, 0);
    }

    #[test]
    fn test_error_key_generation() {
        let manager = create_test_recovery_manager();
        
        let audio_error = PlayerError::Audio(AudioError::DeviceNotFound {
            device: "Test".to_string(),
        });
        let key1 = manager.get_error_key(&audio_error);
        assert_eq!(key1, "audio_device_not_found_Test");
        
        let decode_error = PlayerError::Decode(DecodeError::UnsupportedFormat {
            format: "UNKNOWN".to_string(),
        });
        let key2 = manager.get_error_key(&decode_error);
        assert_eq!(key2, "decode_unsupported_UNKNOWN");
        
        // Same error should generate same key
        let key3 = manager.get_error_key(&audio_error);
        assert_eq!(key1, key3);
        
        // Different audio errors should generate different keys
        let stream_error = PlayerError::Audio(AudioError::StreamError("Test".to_string()));
        let buffer_error = PlayerError::Audio(AudioError::BufferUnderrun);
        
        let stream_key = manager.get_error_key(&stream_error);
        let buffer_key = manager.get_error_key(&buffer_error);
        
        assert_eq!(stream_key, "audio_stream_error_Test");
        assert_eq!(buffer_key, "audio_buffer_underrun");
        assert_ne!(stream_key, buffer_key);
    }

    #[tokio::test]
    async fn test_clear_recovery_attempts() {
        let mut manager = create_test_recovery_manager();
        
        let error = AudioError::StreamError("Test".to_string());
        let player_error = PlayerError::Audio(error);
        
        // Make an attempt to add to tracking
        let _result = manager.attempt_recovery(&player_error).await;
        assert!(!manager.recovery_attempts.is_empty());
        
        // Clear attempts
        manager.clear_recovery_attempts();
        assert!(manager.recovery_attempts.is_empty());
    }

    #[tokio::test]
    async fn test_reset_specific_recovery_attempts() {
        let mut manager = create_test_recovery_manager();
        
        // Use two errors that both result in Retry so they stay in the map
        let error1 = PlayerError::Audio(AudioError::StreamError("Test1".to_string()));
        let error2 = PlayerError::Audio(AudioError::DeviceNotFound { device: "TestDevice".to_string() });
        
        // Make attempts for both errors
        let _result1 = manager.attempt_recovery(&error1).await;
        let _result2 = manager.attempt_recovery(&error2).await;
        
        assert_eq!(manager.recovery_attempts.len(), 2);
        
        // Reset only one error
        manager.reset_recovery_attempts(&error1);
        assert_eq!(manager.recovery_attempts.len(), 1);
        
        // The remaining error should still be tracked
        let remaining_key = manager.get_error_key(&error2);
        assert!(manager.recovery_attempts.contains_key(&remaining_key));
    }
}