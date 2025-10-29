use thiserror::Error;

/// Main player error type
#[derive(Debug, Error)]
pub enum PlayerError {
    #[error("Audio error: {0}")]
    Audio(#[from] AudioError),

    #[error("File error: {0}")]
    File(#[from] std::io::Error),

    #[error("Decode error: {0}")]
    Decode(#[from] DecodeError),

    #[error("Configuration error: {0}")]
    Config(#[from] ConfigError),

    #[error("Queue error: {0}")]
    Queue(#[from] QueueError),

    #[error("Playlist error: {0}")]
    Playlist(#[from] PlaylistError),

    #[error("CLI parse error: {0}")]
    Parse(#[from] crate::cli::ParseError),
}

impl PlayerError {
    /// Get user-friendly error message with suggested solutions
    pub fn user_message(&self) -> String {
        match self {
            PlayerError::Audio(err) => err.user_message(),
            PlayerError::File(err) => Self::format_file_error(err),
            PlayerError::Decode(err) => err.user_message(),
            PlayerError::Config(err) => err.user_message(),
            PlayerError::Queue(err) => err.user_message(),
            PlayerError::Playlist(err) => err.user_message(),
            PlayerError::Parse(err) => format!("Command error: {}", err),
        }
    }

    /// Get suggested recovery actions for the error
    pub fn recovery_suggestions(&self) -> Vec<String> {
        match self {
            PlayerError::Audio(err) => err.recovery_suggestions(),
            PlayerError::File(err) => Self::file_error_suggestions(err),
            PlayerError::Decode(err) => err.recovery_suggestions(),
            PlayerError::Config(err) => err.recovery_suggestions(),
            PlayerError::Queue(err) => err.recovery_suggestions(),
            PlayerError::Playlist(err) => err.recovery_suggestions(),
            PlayerError::Parse(_) => vec!["Type 'help' to see available commands".to_string()],
        }
    }

    /// Check if this error allows for automatic recovery
    pub fn is_recoverable(&self) -> bool {
        match self {
            PlayerError::Audio(err) => err.is_recoverable(),
            PlayerError::File(_) => false, // File errors usually require user intervention
            PlayerError::Decode(err) => err.is_recoverable(),
            PlayerError::Config(err) => err.is_recoverable(),
            PlayerError::Queue(err) => err.is_recoverable(),
            PlayerError::Playlist(err) => err.is_recoverable(),
            PlayerError::Parse(_) => false, // Parse errors require correct input
        }
    }

    /// Get error severity level
    pub fn severity(&self) -> ErrorSeverity {
        match self {
            PlayerError::Audio(AudioError::BufferUnderrun) => ErrorSeverity::Warning,
            PlayerError::Audio(AudioError::DeviceNotFound { .. }) => ErrorSeverity::Error,
            PlayerError::Audio(_) => ErrorSeverity::Critical,
            PlayerError::File(_) => ErrorSeverity::Error,
            PlayerError::Decode(DecodeError::UnsupportedFormat { .. }) => ErrorSeverity::Warning,
            PlayerError::Decode(_) => ErrorSeverity::Error,
            PlayerError::Config(_) => ErrorSeverity::Warning,
            PlayerError::Queue(QueueError::EmptyQueue) => ErrorSeverity::Info,
            PlayerError::Queue(_) => ErrorSeverity::Warning,
            PlayerError::Playlist(_) => ErrorSeverity::Warning,
            PlayerError::Parse(_) => ErrorSeverity::Info,
        }
    }

    fn format_file_error(err: &std::io::Error) -> String {
        match err.kind() {
            std::io::ErrorKind::NotFound => "File or directory not found".to_string(),
            std::io::ErrorKind::PermissionDenied => "Permission denied - cannot access file".to_string(),
            std::io::ErrorKind::InvalidData => "File contains invalid or corrupted data".to_string(),
            std::io::ErrorKind::UnexpectedEof => "File appears to be truncated or corrupted".to_string(),
            _ => format!("File system error: {}", err),
        }
    }

    fn file_error_suggestions(err: &std::io::Error) -> Vec<String> {
        match err.kind() {
            std::io::ErrorKind::NotFound => vec![
                "Check that the file path is correct".to_string(),
                "Use 'queue list' to see available files".to_string(),
                "Try using absolute path instead of relative path".to_string(),
            ],
            std::io::ErrorKind::PermissionDenied => vec![
                "Check file permissions".to_string(),
                "Try running with appropriate permissions".to_string(),
                "Ensure the file is not locked by another application".to_string(),
            ],
            std::io::ErrorKind::InvalidData | std::io::ErrorKind::UnexpectedEof => vec![
                "Try re-downloading or re-copying the file".to_string(),
                "Check if the file is completely downloaded".to_string(),
                "Verify file integrity with a checksum if available".to_string(),
            ],
            _ => vec!["Try the operation again".to_string()],
        }
    }
}

/// Error severity levels for logging and user feedback
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorSeverity {
    Info,
    Warning,
    Error,
    Critical,
}

impl ErrorSeverity {
    pub fn as_str(&self) -> &'static str {
        match self {
            ErrorSeverity::Info => "INFO",
            ErrorSeverity::Warning => "WARNING",
            ErrorSeverity::Error => "ERROR",
            ErrorSeverity::Critical => "CRITICAL",
        }
    }

    pub fn log_level(&self) -> log::Level {
        match self {
            ErrorSeverity::Info => log::Level::Info,
            ErrorSeverity::Warning => log::Level::Warn,
            ErrorSeverity::Error => log::Level::Error,
            ErrorSeverity::Critical => log::Level::Error,
        }
    }
}

/// Audio-related errors
#[derive(Debug, Error)]
pub enum AudioError {
    #[error("Device not found: {device}")]
    DeviceNotFound { device: String },

    #[error("Unsupported sample rate: {rate}")]
    UnsupportedSampleRate { rate: u32 },

    #[error("Unsupported format: {format}")]
    UnsupportedFormat { format: String },

    #[error("Stream error: {0}")]
    StreamError(String),

    #[error("Buffer underrun")]
    BufferUnderrun,

    #[error("Audio initialization failed: {0}")]
    InitializationFailed(String),

    #[error("Invalid seek position: {position:.2}s exceeds track duration {duration:.2}s")]
    InvalidSeekPosition { position: f64, duration: f64 },
}

impl AudioError {
    pub fn user_message(&self) -> String {
        match self {
            AudioError::DeviceNotFound { device } => {
                format!("Audio device '{}' is not available or has been disconnected", device)
            }
            AudioError::UnsupportedSampleRate { rate } => {
                format!("Sample rate {} Hz is not supported by the current audio device", rate)
            }
            AudioError::UnsupportedFormat { format } => {
                format!("Audio format '{}' is not supported", format)
            }
            AudioError::StreamError(msg) => {
                format!("Audio playback interrupted: {}", msg)
            }
            AudioError::BufferUnderrun => {
                "Audio buffer underrun detected - playback may stutter".to_string()
            }
            AudioError::InitializationFailed(msg) => {
                format!("Failed to initialize audio system: {}", msg)
            }
            AudioError::InvalidSeekPosition { position, duration } => {
                format!("Cannot seek to {:.1}s - track is only {:.1}s long", position, duration)
            }
        }
    }

    pub fn recovery_suggestions(&self) -> Vec<String> {
        match self {
            AudioError::DeviceNotFound { .. } => vec![
                "Use 'device list' to see available audio devices".to_string(),
                "Check that your audio device is connected and powered on".to_string(),
                "Try selecting a different audio device with 'device set <name>'".to_string(),
                "Restart the application to refresh device list".to_string(),
            ],
            AudioError::UnsupportedSampleRate { rate: _ } => vec![
                format!("Try a different sample rate (common rates: 44100, 48000, 96000 Hz)"),
                "Check your audio device specifications".to_string(),
                "Use audio conversion software to change the file's sample rate".to_string(),
            ],
            AudioError::UnsupportedFormat { .. } => vec![
                "Supported formats: FLAC, WAV, ALAC, MP3, OGG/Vorbis".to_string(),
                "Convert the file to a supported format".to_string(),
                "Check if the file extension matches the actual format".to_string(),
            ],
            AudioError::StreamError(_) => vec![
                "Try pausing and resuming playback".to_string(),
                "Check audio device connections".to_string(),
                "Restart the audio stream with 'stop' then 'play'".to_string(),
            ],
            AudioError::BufferUnderrun => vec![
                "This is usually temporary - playback should recover automatically".to_string(),
                "Close other audio applications to reduce system load".to_string(),
                "Consider increasing buffer size in configuration".to_string(),
            ],
            AudioError::InitializationFailed(_) => vec![
                "Restart the application".to_string(),
                "Check that no other applications are using exclusive audio access".to_string(),
                "Try selecting a different audio device".to_string(),
                "Verify audio drivers are properly installed".to_string(),
            ],
            AudioError::InvalidSeekPosition { duration, .. } => vec![
                format!("Use a position between 0 and {:.1} seconds", duration),
                "Try seeking to an earlier position in the track".to_string(),
            ],
        }
    }

    pub fn is_recoverable(&self) -> bool {
        match self {
            AudioError::DeviceNotFound { .. } => true,  // Can fallback to default device
            AudioError::UnsupportedSampleRate { .. } => false, // Requires different file or device
            AudioError::UnsupportedFormat { .. } => false, // Requires different file or conversion
            AudioError::StreamError(_) => true,  // Can restart stream
            AudioError::BufferUnderrun => true,  // Usually recovers automatically
            AudioError::InitializationFailed(_) => true,  // Can retry initialization
            AudioError::InvalidSeekPosition { .. } => false, // Requires valid position
        }
    }
}

/// Audio decoding errors
#[derive(Debug, Error)]
pub enum DecodeError {
    #[error("Unsupported format: {format}")]
    UnsupportedFormat { format: String },

    #[error("Corrupted file: {0}")]
    CorruptedFile(String),

    #[error("Seek error: {0}")]
    SeekError(String),

    #[error("Decode failed: {0}")]
    DecodeFailed(String),
}

impl DecodeError {
    pub fn user_message(&self) -> String {
        match self {
            DecodeError::UnsupportedFormat { format } => {
                format!("Audio format '{}' is not supported by this player", format)
            }
            DecodeError::CorruptedFile(msg) => {
                format!("Audio file appears to be corrupted or damaged: {}", msg)
            }
            DecodeError::SeekError(msg) => {
                format!("Cannot seek in this audio file: {}", msg)
            }
            DecodeError::DecodeFailed(msg) => {
                format!("Failed to decode audio data: {}", msg)
            }
        }
    }

    pub fn recovery_suggestions(&self) -> Vec<String> {
        match self {
            DecodeError::UnsupportedFormat { format } => vec![
                "Supported formats: FLAC, WAV, ALAC, MP3, OGG/Vorbis".to_string(),
                format!("Convert '{}' to a supported format using audio conversion software", format),
                "Check if the file extension matches the actual format".to_string(),
            ],
            DecodeError::CorruptedFile(_) => vec![
                "Try re-downloading or re-copying the file".to_string(),
                "Check if the file transfer completed successfully".to_string(),
                "Verify file integrity with a checksum if available".to_string(),
                "Try playing the file in another audio player to confirm corruption".to_string(),
            ],
            DecodeError::SeekError(_) => vec![
                "Some audio formats don't support seeking".to_string(),
                "Try converting to a format that supports seeking (like FLAC or WAV)".to_string(),
                "Play from the beginning instead of seeking".to_string(),
            ],
            DecodeError::DecodeFailed(_) => vec![
                "Try re-encoding the file with different settings".to_string(),
                "Check if the file is completely downloaded".to_string(),
                "Verify the file is not corrupted".to_string(),
            ],
        }
    }

    pub fn is_recoverable(&self) -> bool {
        match self {
            DecodeError::UnsupportedFormat { .. } => false, // Requires different file
            DecodeError::CorruptedFile(_) => false, // Requires file repair/replacement
            DecodeError::SeekError(_) => true, // Can continue without seeking
            DecodeError::DecodeFailed(_) => false, // Usually indicates file issues
        }
    }
}

/// Configuration-related errors
#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("Configuration directory not found")]
    ConfigDirNotFound,

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    SerializationError(#[from] toml::ser::Error),

    #[error("Deserialization error: {0}")]
    DeserializationError(#[from] toml::de::Error),
}

impl ConfigError {
    pub fn user_message(&self) -> String {
        match self {
            ConfigError::ConfigDirNotFound => {
                "Cannot find or create configuration directory".to_string()
            }
            ConfigError::IoError(err) => {
                format!("Cannot access configuration file: {}", err)
            }
            ConfigError::SerializationError(_) => {
                "Failed to save configuration settings".to_string()
            }
            ConfigError::DeserializationError(_) => {
                "Configuration file is corrupted or has invalid format".to_string()
            }
        }
    }

    pub fn recovery_suggestions(&self) -> Vec<String> {
        match self {
            ConfigError::ConfigDirNotFound => vec![
                "Check that you have write permissions to your home directory".to_string(),
                "Try creating the directory manually: ~/.config/hires-player/".to_string(),
            ],
            ConfigError::IoError(_) => vec![
                "Check file permissions for the configuration directory".to_string(),
                "Ensure the disk is not full".to_string(),
                "Try deleting and recreating the configuration file".to_string(),
            ],
            ConfigError::SerializationError(_) => vec![
                "Configuration will use default values".to_string(),
                "Try resetting configuration to defaults".to_string(),
            ],
            ConfigError::DeserializationError(_) => vec![
                "Delete the configuration file to reset to defaults".to_string(),
                "Check the configuration file format manually".to_string(),
                "Backup and recreate the configuration file".to_string(),
            ],
        }
    }

    pub fn is_recoverable(&self) -> bool {
        match self {
            ConfigError::ConfigDirNotFound => true, // Can use defaults
            ConfigError::IoError(_) => true, // Can retry or use defaults
            ConfigError::SerializationError(_) => true, // Can use current settings
            ConfigError::DeserializationError(_) => true, // Can use defaults
        }
    }
}

/// Queue management errors
#[derive(Debug, Error)]
pub enum QueueError {
    #[error("File not found: {path}")]
    FileNotFound { path: String },

    #[error("Invalid file format: {path}")]
    InvalidFormat { path: String },

    #[error("Queue is empty")]
    EmptyQueue,

    #[error("Invalid index: {index}")]
    InvalidIndex { index: usize },
}

impl QueueError {
    pub fn user_message(&self) -> String {
        match self {
            QueueError::FileNotFound { path } => {
                format!("Cannot find audio file: {}", path)
            }
            QueueError::InvalidFormat { path } => {
                format!("File '{}' is not a supported audio format", path)
            }
            QueueError::EmptyQueue => {
                "No tracks in queue - add some files first".to_string()
            }
            QueueError::InvalidIndex { index } => {
                format!("Track number {} is not valid for current queue", index + 1)
            }
        }
    }

    pub fn recovery_suggestions(&self) -> Vec<String> {
        match self {
            QueueError::FileNotFound { .. } => vec![
                "Check that the file path is correct".to_string(),
                "Use 'queue add <path>' to add files to the queue".to_string(),
                "Try using absolute paths instead of relative paths".to_string(),
            ],
            QueueError::InvalidFormat { .. } => vec![
                "Supported formats: FLAC, WAV, ALAC, MP3, OGG/Vorbis".to_string(),
                "Convert the file to a supported format".to_string(),
                "Check if the file extension matches the actual format".to_string(),
            ],
            QueueError::EmptyQueue => vec![
                "Use 'queue add <file>' to add individual files".to_string(),
                "Use 'queue add <directory>' to add all files from a folder".to_string(),
                "Use 'playlist load <name>' to load a saved playlist".to_string(),
            ],
            QueueError::InvalidIndex { .. } => vec![
                "Use 'queue list' to see available tracks".to_string(),
                "Track numbers start from 1".to_string(),
            ],
        }
    }

    pub fn is_recoverable(&self) -> bool {
        match self {
            QueueError::FileNotFound { .. } => false, // Requires valid file
            QueueError::InvalidFormat { .. } => false, // Requires supported format
            QueueError::EmptyQueue => true, // Can add files
            QueueError::InvalidIndex { .. } => false, // Requires valid index
        }
    }
}

/// Playlist-related errors
#[derive(Debug, Error)]
pub enum PlaylistError {
    #[error("Playlist not found: {name}")]
    PlaylistNotFound { name: String },

    #[error("Invalid playlist format: {0}")]
    InvalidFormat(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

impl PlaylistError {
    pub fn user_message(&self) -> String {
        match self {
            PlaylistError::PlaylistNotFound { name } => {
                format!("Playlist '{}' does not exist", name)
            }
            PlaylistError::InvalidFormat(msg) => {
                format!("Playlist file has invalid format: {}", msg)
            }
            PlaylistError::IoError(err) => {
                format!("Cannot access playlist file: {}", err)
            }
        }
    }

    pub fn recovery_suggestions(&self) -> Vec<String> {
        match self {
            PlaylistError::PlaylistNotFound { .. } => vec![
                "Use 'playlist list' to see available playlists".to_string(),
                "Create a new playlist with 'playlist save <name>'".to_string(),
                "Check the playlist name spelling".to_string(),
            ],
            PlaylistError::InvalidFormat(_) => vec![
                "Supported playlist formats: M3U, PLS".to_string(),
                "Try recreating the playlist".to_string(),
                "Check the playlist file manually for formatting errors".to_string(),
            ],
            PlaylistError::IoError(_) => vec![
                "Check file permissions for the playlist directory".to_string(),
                "Ensure the disk is not full".to_string(),
                "Try recreating the playlist".to_string(),
            ],
        }
    }

    pub fn is_recoverable(&self) -> bool {
        match self {
            PlaylistError::PlaylistNotFound { .. } => false, // Requires existing playlist
            PlaylistError::InvalidFormat(_) => false, // Requires valid format
            PlaylistError::IoError(_) => true, // Can retry
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;

    #[test]
    fn test_player_error_from_audio_error() {
        let audio_error = AudioError::DeviceNotFound {
            device: "Test Device".to_string(),
        };
        let player_error: PlayerError = audio_error.into();

        match player_error {
            PlayerError::Audio(AudioError::DeviceNotFound { device }) => {
                assert_eq!(device, "Test Device");
            }
            _ => panic!("Expected Audio error variant"),
        }
    }

    #[test]
    fn test_player_error_from_io_error() {
        let io_error = io::Error::new(io::ErrorKind::NotFound, "File not found");
        let player_error: PlayerError = io_error.into();

        match player_error {
            PlayerError::File(_) => {
                // Success - IO error converted to File error
            }
            _ => panic!("Expected File error variant"),
        }
    }

    #[test]
    fn test_player_error_from_decode_error() {
        let decode_error = DecodeError::UnsupportedFormat {
            format: "UNKNOWN".to_string(),
        };
        let player_error: PlayerError = decode_error.into();

        match player_error {
            PlayerError::Decode(DecodeError::UnsupportedFormat { format }) => {
                assert_eq!(format, "UNKNOWN");
            }
            _ => panic!("Expected Decode error variant"),
        }
    }

    #[test]
    fn test_player_error_from_config_error() {
        let config_error = ConfigError::ConfigDirNotFound;
        let player_error: PlayerError = config_error.into();

        match player_error {
            PlayerError::Config(ConfigError::ConfigDirNotFound) => {
                // Success
            }
            _ => panic!("Expected Config error variant"),
        }
    }

    #[test]
    fn test_player_error_from_queue_error() {
        let queue_error = QueueError::EmptyQueue;
        let player_error: PlayerError = queue_error.into();

        match player_error {
            PlayerError::Queue(QueueError::EmptyQueue) => {
                // Success
            }
            _ => panic!("Expected Queue error variant"),
        }
    }

    #[test]
    fn test_audio_error_display() {
        let error = AudioError::DeviceNotFound {
            device: "Test Device".to_string(),
        };
        assert_eq!(format!("{}", error), "Device not found: Test Device");

        let error = AudioError::UnsupportedSampleRate { rate: 192000 };
        assert_eq!(format!("{}", error), "Unsupported sample rate: 192000");

        let error = AudioError::StreamError("Stream failed".to_string());
        assert_eq!(format!("{}", error), "Stream error: Stream failed");

        let error = AudioError::BufferUnderrun;
        assert_eq!(format!("{}", error), "Buffer underrun");

        let error = AudioError::InitializationFailed("Init failed".to_string());
        assert_eq!(format!("{}", error), "Audio initialization failed: Init failed");

        let error = AudioError::InvalidSeekPosition { position: 200.5, duration: 180.0 };
        assert_eq!(format!("{}", error), "Invalid seek position: 200.50s exceeds track duration 180.00s");
    }

    #[test]
    fn test_decode_error_display() {
        let error = DecodeError::UnsupportedFormat {
            format: "UNKNOWN".to_string(),
        };
        assert_eq!(format!("{}", error), "Unsupported format: UNKNOWN");

        let error = DecodeError::CorruptedFile("Bad file".to_string());
        assert_eq!(format!("{}", error), "Corrupted file: Bad file");

        let error = DecodeError::SeekError("Seek failed".to_string());
        assert_eq!(format!("{}", error), "Seek error: Seek failed");

        let error = DecodeError::DecodeFailed("Decode failed".to_string());
        assert_eq!(format!("{}", error), "Decode failed: Decode failed");
    }

    #[test]
    fn test_config_error_display() {
        let error = ConfigError::ConfigDirNotFound;
        assert_eq!(format!("{}", error), "Configuration directory not found");

        let io_error = io::Error::new(io::ErrorKind::PermissionDenied, "Permission denied");
        let error = ConfigError::IoError(io_error);
        assert!(format!("{}", error).contains("IO error"));
    }

    #[test]
    fn test_queue_error_display() {
        let error = QueueError::FileNotFound {
            path: "/test/file.flac".to_string(),
        };
        assert_eq!(format!("{}", error), "File not found: /test/file.flac");

        let error = QueueError::InvalidFormat {
            path: "/test/file.txt".to_string(),
        };
        assert_eq!(format!("{}", error), "Invalid file format: /test/file.txt");

        let error = QueueError::EmptyQueue;
        assert_eq!(format!("{}", error), "Queue is empty");

        let error = QueueError::InvalidIndex { index: 5 };
        assert_eq!(format!("{}", error), "Invalid index: 5");
    }

    #[test]
    fn test_playlist_error_display() {
        let error = PlaylistError::PlaylistNotFound {
            name: "test_playlist".to_string(),
        };
        assert_eq!(format!("{}", error), "Playlist not found: test_playlist");

        let error = PlaylistError::InvalidFormat("Bad format".to_string());
        assert_eq!(format!("{}", error), "Invalid playlist format: Bad format");

        let io_error = io::Error::new(io::ErrorKind::NotFound, "File not found");
        let error = PlaylistError::IoError(io_error);
        assert!(format!("{}", error).contains("IO error"));
    }

    #[test]
    fn test_config_error_from_io_error() {
        let io_error = io::Error::new(io::ErrorKind::PermissionDenied, "Permission denied");
        let config_error: ConfigError = io_error.into();

        match config_error {
            ConfigError::IoError(_) => {
                // Success
            }
            _ => panic!("Expected IoError variant"),
        }
    }

    #[test]
    fn test_playlist_error_from_io_error() {
        let io_error = io::Error::new(io::ErrorKind::NotFound, "File not found");
        let playlist_error: PlaylistError = io_error.into();

        match playlist_error {
            PlaylistError::IoError(_) => {
                // Success
            }
            _ => panic!("Expected IoError variant"),
        }
    }

    #[test]
    fn test_error_chain() {
        // Test that errors can be chained properly
        let io_error = io::Error::new(io::ErrorKind::NotFound, "Config file not found");
        let config_error: ConfigError = io_error.into();
        let player_error: PlayerError = config_error.into();

        // Should be able to display the full error chain
        let error_string = format!("{}", player_error);
        assert!(error_string.contains("Configuration error"));
    }

    #[test]
    fn test_error_debug_format() {
        let error = AudioError::DeviceNotFound {
            device: "Test Device".to_string(),
        };
        let debug_string = format!("{:?}", error);
        assert!(debug_string.contains("DeviceNotFound"));
        assert!(debug_string.contains("Test Device"));
    }

    #[test]
    fn test_error_source_chain() {
        use std::error::Error;

        let io_error = io::Error::new(io::ErrorKind::PermissionDenied, "Permission denied");
        let config_error = ConfigError::IoError(io_error);
        let player_error = PlayerError::Config(config_error);

        // Test that we can walk the error source chain
        let mut current_error: &dyn Error = &player_error;
        let mut error_count = 0;

        while let Some(source) = current_error.source() {
            current_error = source;
            error_count += 1;
        }

        // Should have at least one source error (the IO error)
        assert!(error_count >= 1);
    }
}
