use std::time::Duration;
use crate::models::{PlayerStatus, TrackInfo, PlaybackState};

/// Status display formatter for the CLI
pub struct StatusDisplay;

impl StatusDisplay {
    /// Display comprehensive player status with track information and technical specs
    pub fn display_full_status(status: &PlayerStatus) {
        println!("┌─ Player Status ─────────────────────────────────────────┐");
        
        match &status.current_track {
            Some(track) => {
                Self::display_track_info(track);
                Self::display_playback_info(status);
                Self::display_technical_info(status);
            }
            None => {
                println!("│ No track loaded");
                println!("│ Status: {}", status.state.as_str());
            }
        }
        
        Self::display_system_info(status);
        println!("└─────────────────────────────────────────────────────────┘");
    }

    /// Display compact status information
    pub fn display_compact_status(status: &PlayerStatus) {
        match &status.current_track {
            Some(track) => {
                let title = Self::truncate(&track.display_name(), 30);
                let artist = Self::truncate(&track.artist_name(), 25);
                let position = Self::format_duration(status.position);
                let duration = Self::format_duration(track.duration);
                let progress_percent = (status.progress() * 100.0) as u8;
                
                println!("{} | {} - {} | {}/{} ({}%) | {}",
                    status.state.as_str(),
                    artist,
                    title,
                    position,
                    duration,
                    progress_percent,
                    if let Some(format) = &status.audio_format {
                        format.format_description()
                    } else {
                        "Unknown format".to_string()
                    }
                );
            }
            None => {
                println!("{} | No track loaded", status.state.as_str());
            }
        }
    }

    /// Display only track metadata information
    pub fn display_track_metadata(track: &TrackInfo) {
        println!("┌─ Track Information ─────────────────────────────────────┐");
        println!("│ Title: {}", Self::truncate(&track.display_name(), 50));
        println!("│ Artist: {}", Self::truncate(&track.artist_name(), 49));
        println!("│ Album: {}", Self::truncate(&track.album_name(), 50));
        
        if let Some(track_num) = track.metadata.track_number {
            println!("│ Track: {}", track_num);
        }
        
        if let Some(year) = track.metadata.year {
            println!("│ Year: {}", year);
        }
        
        if let Some(genre) = &track.metadata.genre {
            println!("│ Genre: {}", Self::truncate(genre, 50));
        }
        
        println!("│ Duration: {}", Self::format_duration(track.duration));
        println!("│ File Size: {}", Self::format_file_size(track.file_size));
        println!("│ Path: {}", Self::truncate(&track.path.display().to_string(), 45));
        println!("└─────────────────────────────────────────────────────────┘");
    }

    /// Display technical audio format information
    pub fn display_technical_info(status: &PlayerStatus) {
        if let Some(format) = &status.audio_format {
            println!("│");
            println!("│ ┌─ Technical Information ─────────────────────────────┐");
            println!("│ │ Format: {}", format.codec.name());
            println!("│ │ Sample Rate: {} Hz", format.sample_rate);
            println!("│ │ Bit Depth: {}-bit", format.bit_depth);
            println!("│ │ Channels: {} ({})", 
                format.channels, 
                Self::channel_description(format.channels)
            );
            
            if format.is_high_resolution() {
                println!("│ │ Quality: High Resolution Audio");
            } else {
                println!("│ │ Quality: Standard Resolution");
            }
            
            if let Some(bitrate) = format.bitrate() {
                println!("│ │ Bitrate: {} kbps", bitrate / 1000);
            }
            
            if format.codec.is_lossless() {
                println!("│ │ Compression: Lossless");
            } else {
                println!("│ │ Compression: Lossy");
            }
            
            println!("│ └─────────────────────────────────────────────────────┘");
        }
    }

    /// Display track information section
    fn display_track_info(track: &TrackInfo) {
        println!("│ Track: {}", Self::truncate(&track.display_name(), 50));
        println!("│ Artist: {}", Self::truncate(&track.artist_name(), 49));
        println!("│ Album: {}", Self::truncate(&track.album_name(), 50));
        
        // Display additional metadata if available
        if let Some(track_num) = track.metadata.track_number {
            print!("│ Track #: {}", track_num);
            if let Some(year) = track.metadata.year {
                println!(" | Year: {}", year);
            } else {
                println!();
            }
        } else if let Some(year) = track.metadata.year {
            println!("│ Year: {}", year);
        }
        
        if let Some(genre) = &track.metadata.genre {
            println!("│ Genre: {}", Self::truncate(genre, 50));
        }
    }

    /// Display playback information section
    fn display_playback_info(status: &PlayerStatus) {
        println!("│");
        println!("│ Status: {}", status.state.as_str());
        
        if let Some(track) = &status.current_track {
            println!("│ Position: {} / {}", 
                Self::format_duration(status.position), 
                Self::format_duration(track.duration)
            );
            
            // Progress bar
            let progress = status.progress();
            let bar_width = 40;
            let filled = (progress * bar_width as f32) as usize;
            let empty = bar_width - filled;
            let progress_bar = format!("{}{}",
                "█".repeat(filled),
                "░".repeat(empty)
            );
            println!("│ Progress: [{}] {:.1}%", progress_bar, progress * 100.0);
            
            // Time remaining
            let remaining = track.duration.saturating_sub(status.position);
            println!("│ Remaining: {}", Self::format_duration(remaining));
        }
    }

    /// Display system information section
    fn display_system_info(status: &PlayerStatus) {
        println!("│");
        println!("│ Volume: {}%", (status.volume * 100.0) as u8);
        
        if let Some(device) = &status.output_device {
            println!("│ Device: {}", Self::truncate(device, 49));
        } else {
            println!("│ Device: Default");
        }
    }

    /// Display real-time position update (single line)
    pub fn display_position_update(status: &PlayerStatus) {
        if let Some(track) = &status.current_track {
            let progress = status.progress();
            let bar_width = 30;
            let filled = (progress * bar_width as f32) as usize;
            let empty = bar_width - filled;
            let progress_bar = format!("{}{}",
                "█".repeat(filled),
                "░".repeat(empty)
            );
            
            print!("\r{} [{}] {}/{} ({:.1}%)",
                status.state.as_str(),
                progress_bar,
                Self::format_duration(status.position),
                Self::format_duration(track.duration),
                progress * 100.0
            );
            
            // Flush stdout to ensure immediate display
            use std::io::{self, Write};
            let _ = io::stdout().flush();
        }
    }

    /// Display error message with formatting and recovery suggestions
    pub fn display_error(error: &crate::error::PlayerError) {
        use crate::error::ErrorSeverity;
        
        let severity = error.severity();
        let severity_icon = match severity {
            ErrorSeverity::Info => "ℹ",
            ErrorSeverity::Warning => "⚠",
            ErrorSeverity::Error => "✗",
            ErrorSeverity::Critical => "🔥",
        };
        
        eprintln!("┌─ {} {} ─────────────────────────────────────────────────┐", 
            severity_icon, severity.as_str());
        
        // Display user-friendly error message
        let user_message = error.user_message();
        for line in Self::wrap_text(&user_message, 55) {
            eprintln!("│ {}", line);
        }
        
        // Display recovery suggestions
        let suggestions = error.recovery_suggestions();
        if !suggestions.is_empty() {
            eprintln!("│");
            eprintln!("│ Suggestions:");
            for suggestion in suggestions.iter().take(3) { // Limit to 3 suggestions
                for line in Self::wrap_text(&format!("• {}", suggestion), 53) {
                    eprintln!("│   {}", line);
                }
            }
        }
        
        // Display additional context based on error type
        Self::display_error_context(error);
        
        eprintln!("└─────────────────────────────────────────────────────────┘");
    }

    /// Display additional context for specific error types
    fn display_error_context(error: &crate::error::PlayerError) {
        match error {
            crate::error::PlayerError::Audio(audio_err) => {
                match audio_err {
                    crate::error::AudioError::DeviceNotFound { .. } => {
                        eprintln!("│");
                        eprintln!("│ Use 'device list' to see available devices");
                    }
                    crate::error::AudioError::UnsupportedSampleRate { .. } => {
                        eprintln!("│");
                        eprintln!("│ Common rates: 44.1kHz, 48kHz, 96kHz, 192kHz");
                    }
                    crate::error::AudioError::BufferUnderrun => {
                        eprintln!("│");
                        eprintln!("│ This usually recovers automatically");
                    }
                    _ => {}
                }
            }
            crate::error::PlayerError::Decode(decode_err) => {
                match decode_err {
                    crate::error::DecodeError::UnsupportedFormat { .. } => {
                        eprintln!("│");
                        eprintln!("│ Supported: FLAC, WAV, ALAC, MP3, OGG/Vorbis");
                    }
                    crate::error::DecodeError::CorruptedFile(_) => {
                        eprintln!("│");
                        eprintln!("│ File may need to be re-downloaded or re-encoded");
                    }
                    _ => {}
                }
            }
            crate::error::PlayerError::Queue(queue_err) => {
                match queue_err {
                    crate::error::QueueError::EmptyQueue => {
                        eprintln!("│");
                        eprintln!("│ Add files: 'queue add <path>' or 'playlist load <name>'");
                    }
                    _ => {}
                }
            }
            crate::error::PlayerError::Config(_) => {
                eprintln!("│");
                eprintln!("│ Configuration will use default values");
            }
            _ => {}
        }
    }

    /// Wrap text to fit within specified width
    fn wrap_text(text: &str, width: usize) -> Vec<String> {
        let mut lines = Vec::new();
        let mut current_line = String::new();
        
        for word in text.split_whitespace() {
            if current_line.is_empty() {
                current_line = word.to_string();
            } else if current_line.len() + word.len() + 1 <= width {
                current_line.push(' ');
                current_line.push_str(word);
            } else {
                lines.push(current_line);
                current_line = word.to_string();
            }
        }
        
        if !current_line.is_empty() {
            lines.push(current_line);
        }
        
        // Pad lines to consistent width
        lines.into_iter()
            .map(|line| format!("{:<width$}", line, width = width))
            .collect()
    }

    /// Display error with recovery options for interactive mode
    pub fn display_error_with_recovery(error: &crate::error::PlayerError, recovery_available: bool) {
        Self::display_error(error);
        
        if recovery_available && error.is_recoverable() {
            eprintln!();
            eprintln!("💡 Automatic recovery is available for this error.");
            eprintln!("   The system will attempt to recover automatically.");
        } else if !error.is_recoverable() {
            eprintln!();
            eprintln!("⚠  This error requires manual intervention to resolve.");
        }
    }

    /// Display a simple error message for non-interactive contexts
    pub fn display_simple_error(error: &crate::error::PlayerError) {
        let severity = error.severity();
        eprintln!("[{}] {}", severity.as_str(), error.user_message());
        
        let suggestions = error.recovery_suggestions();
        if !suggestions.is_empty() {
            eprintln!("Suggestion: {}", suggestions[0]);
        }
    }

    /// Display help information for status commands
    pub fn display_status_help() {
        println!("Status Display Commands:");
        println!();
        println!("  status          - Show full player status with all information");
        println!("  status compact  - Show compact one-line status");
        println!("  status track    - Show detailed track metadata");
        println!("  status tech     - Show technical audio format information");
        println!();
        println!("Status Information Includes:");
        println!("  • Track metadata (title, artist, album, year, genre)");
        println!("  • Playback state and position");
        println!("  • Technical specs (sample rate, bit depth, channels)");
        println!("  • Audio quality indicators (high-res, lossless/lossy)");
        println!("  • Progress bar and time remaining");
        println!("  • Volume level and output device");
    }

    /// Format duration as MM:SS or HH:MM:SS for longer tracks
    pub fn format_duration(duration: Duration) -> String {
        let total_seconds = duration.as_secs();
        let hours = total_seconds / 3600;
        let minutes = (total_seconds % 3600) / 60;
        let seconds = total_seconds % 60;
        
        if hours > 0 {
            format!("{:02}:{:02}:{:02}", hours, minutes, seconds)
        } else {
            format!("{:02}:{:02}", minutes, seconds)
        }
    }

    /// Format file size in human-readable format
    pub fn format_file_size(size: u64) -> String {
        const UNITS: &[&str] = &["B", "KB", "MB", "GB"];
        let mut size_f = size as f64;
        let mut unit_index = 0;
        
        while size_f >= 1024.0 && unit_index < UNITS.len() - 1 {
            size_f /= 1024.0;
            unit_index += 1;
        }
        
        if unit_index == 0 {
            format!("{} {}", size, UNITS[unit_index])
        } else {
            format!("{:.1} {}", size_f, UNITS[unit_index])
        }
    }

    /// Get channel description from channel count
    pub fn channel_description(channels: u16) -> &'static str {
        match channels {
            1 => "Mono",
            2 => "Stereo",
            3 => "2.1",
            4 => "Quad",
            5 => "4.1",
            6 => "5.1 Surround",
            7 => "6.1 Surround",
            8 => "7.1 Surround",
            _ => "Multi-channel",
        }
    }

    /// Truncate string to fit display width
    pub fn truncate(s: &str, max_len: usize) -> String {
        if s.len() <= max_len {
            s.to_string()
        } else if max_len <= 3 {
            // If max_len is too small for ellipsis, return original string
            s.to_string()
        } else {
            format!("{}...", &s[..max_len.saturating_sub(3)])
        }
    }

    /// Create a progress bar string
    pub fn create_progress_bar(progress: f32, width: usize) -> String {
        let filled = (progress * width as f32) as usize;
        let empty = width - filled;
        format!("{}{}",
            "█".repeat(filled),
            "░".repeat(empty)
        )
    }

    /// Format playback state with color indicators (if terminal supports it)
    pub fn format_playback_state(state: PlaybackState) -> String {
        match state {
            PlaybackState::Playing => "▶ Playing".to_string(),
            PlaybackState::Paused => "⏸ Paused".to_string(),
            PlaybackState::Stopped => "⏹ Stopped".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{AudioMetadata, AudioFormat, AudioCodec, TrackInfo};
    use std::path::PathBuf;

    fn create_test_track() -> TrackInfo {
        let metadata = AudioMetadata {
            title: Some("Test Song".to_string()),
            artist: Some("Test Artist".to_string()),
            album: Some("Test Album".to_string()),
            track_number: Some(1),
            year: Some(2023),
            genre: Some("Test Genre".to_string()),
        };
        
        TrackInfo::new(
            PathBuf::from("/test/path/song.flac"),
            metadata,
            Duration::from_secs(180), // 3 minutes
            1024 * 1024 // 1MB
        )
    }

    fn create_test_status() -> PlayerStatus {
        let track = create_test_track();
        let mut status = PlayerStatus::playing(track, Duration::from_secs(60), 0.8);
        
        status.audio_format = Some(AudioFormat::new(44100, 16, 2, AudioCodec::Flac));
        status.output_device = Some("Test Device".to_string());
        
        status
    }

    #[test]
    fn test_format_duration() {
        // Test seconds only
        assert_eq!(StatusDisplay::format_duration(Duration::from_secs(30)), "00:30");
        assert_eq!(StatusDisplay::format_duration(Duration::from_secs(90)), "01:30");
        
        // Test minutes and seconds
        assert_eq!(StatusDisplay::format_duration(Duration::from_secs(180)), "03:00");
        assert_eq!(StatusDisplay::format_duration(Duration::from_secs(185)), "03:05");
        
        // Test hours, minutes, and seconds
        assert_eq!(StatusDisplay::format_duration(Duration::from_secs(3661)), "01:01:01");
        assert_eq!(StatusDisplay::format_duration(Duration::from_secs(7200)), "02:00:00");
    }

    #[test]
    fn test_format_file_size() {
        assert_eq!(StatusDisplay::format_file_size(512), "512 B");
        assert_eq!(StatusDisplay::format_file_size(1024), "1.0 KB");
        assert_eq!(StatusDisplay::format_file_size(1536), "1.5 KB");
        assert_eq!(StatusDisplay::format_file_size(1024 * 1024), "1.0 MB");
        assert_eq!(StatusDisplay::format_file_size(1024 * 1024 * 1024), "1.0 GB");
        assert_eq!(StatusDisplay::format_file_size(1536 * 1024 * 1024), "1.5 GB");
    }

    #[test]
    fn test_channel_description() {
        assert_eq!(StatusDisplay::channel_description(1), "Mono");
        assert_eq!(StatusDisplay::channel_description(2), "Stereo");
        assert_eq!(StatusDisplay::channel_description(3), "2.1");
        assert_eq!(StatusDisplay::channel_description(4), "Quad");
        assert_eq!(StatusDisplay::channel_description(5), "4.1");
        assert_eq!(StatusDisplay::channel_description(6), "5.1 Surround");
        assert_eq!(StatusDisplay::channel_description(7), "6.1 Surround");
        assert_eq!(StatusDisplay::channel_description(8), "7.1 Surround");
        assert_eq!(StatusDisplay::channel_description(16), "Multi-channel");
    }

    #[test]
    fn test_truncate() {
        assert_eq!(StatusDisplay::truncate("short", 10), "short");
        assert_eq!(StatusDisplay::truncate("exactly ten", 11), "exactly ten");
        assert_eq!(StatusDisplay::truncate("this is a very long string", 10), "this is...");
        assert_eq!(StatusDisplay::truncate("", 5), "");
        assert_eq!(StatusDisplay::truncate("abc", 2), "abc");  // Return original if too short for ellipsis
    }

    #[test]
    fn test_create_progress_bar() {
        assert_eq!(StatusDisplay::create_progress_bar(0.0, 10), "░░░░░░░░░░");
        assert_eq!(StatusDisplay::create_progress_bar(1.0, 10), "██████████");
        assert_eq!(StatusDisplay::create_progress_bar(0.5, 10), "█████░░░░░");
        assert_eq!(StatusDisplay::create_progress_bar(0.3, 10), "███░░░░░░░");
    }

    #[test]
    fn test_format_playback_state() {
        assert_eq!(StatusDisplay::format_playback_state(PlaybackState::Playing), "▶ Playing");
        assert_eq!(StatusDisplay::format_playback_state(PlaybackState::Paused), "⏸ Paused");
        assert_eq!(StatusDisplay::format_playback_state(PlaybackState::Stopped), "⏹ Stopped");
    }

    #[test]
    fn test_display_functions_dont_panic() {
        let status = create_test_status();
        let track = create_test_track();
        
        // These functions should not panic when called
        // We can't easily test their output without capturing stdout,
        // but we can ensure they don't crash
        
        // Test with valid status
        StatusDisplay::display_compact_status(&status);
        StatusDisplay::display_track_metadata(&track);
        StatusDisplay::display_position_update(&status);
        
        // Test with empty status
        let empty_status = PlayerStatus::new();
        StatusDisplay::display_compact_status(&empty_status);
        StatusDisplay::display_position_update(&empty_status);
        
        // Test help display
        StatusDisplay::display_status_help();
    }

    #[test]
    fn test_display_with_missing_metadata() {
        let mut metadata = AudioMetadata::new();
        metadata.title = Some("Only Title".to_string());
        
        let track = TrackInfo::new(
            PathBuf::from("/test/song.flac"),
            metadata,
            Duration::from_secs(120),
            1024
        );
        
        // Should handle missing metadata gracefully
        StatusDisplay::display_track_metadata(&track);
        
        let status = PlayerStatus::playing(track, Duration::from_secs(30), 1.0);
        StatusDisplay::display_compact_status(&status);
    }

    #[test]
    fn test_display_with_long_strings() {
        let metadata = AudioMetadata {
            title: Some("This is a very long song title that should be truncated".to_string()),
            artist: Some("This is a very long artist name that should also be truncated".to_string()),
            album: Some("This is a very long album name that should be truncated as well".to_string()),
            track_number: Some(1),
            year: Some(2023),
            genre: Some("This is a very long genre name".to_string()),
        };
        
        let track = TrackInfo::new(
            PathBuf::from("/very/long/path/to/a/file/with/a/very/long/name/song.flac"),
            metadata,
            Duration::from_secs(300),
            1024 * 1024 * 50 // 50MB
        );
        
        // Should handle long strings by truncating them
        StatusDisplay::display_track_metadata(&track);
        
        let status = PlayerStatus::playing(track, Duration::from_secs(150), 0.75);
        StatusDisplay::display_compact_status(&status);
    }

    #[test]
    fn test_progress_calculation() {
        let track = create_test_track(); // 180 seconds duration
        
        // Test various positions
        let test_cases = [
            (0, 0.0),      // Start
            (90, 0.5),     // Middle
            (180, 1.0),    // End
            (45, 0.25),    // Quarter
            (135, 0.75),   // Three quarters
        ];
        
        for (position_secs, expected_progress) in test_cases {
            let status = PlayerStatus::playing(
                track.clone(), 
                Duration::from_secs(position_secs), 
                1.0
            );
            
            let actual_progress = status.progress();
            assert!((actual_progress - expected_progress).abs() < 0.01, 
                "Progress calculation failed for {}s position", position_secs);
        }
    }

    #[test]
    fn test_zero_duration_handling() {
        let mut track = create_test_track();
        track.duration = Duration::from_secs(0);
        
        let status = PlayerStatus::playing(track, Duration::from_secs(0), 1.0);
        
        // Should handle zero duration gracefully
        assert_eq!(status.progress(), 0.0);
        StatusDisplay::display_compact_status(&status);
    }

    #[test]
    fn test_high_resolution_format_display() {
        let track = create_test_track();
        let mut status = PlayerStatus::playing(track, Duration::from_secs(60), 1.0);
        
        // Test with high-resolution format
        status.audio_format = Some(AudioFormat::new(96000, 24, 2, AudioCodec::Flac));
        StatusDisplay::display_technical_info(&status);
        
        // Test with standard resolution format
        status.audio_format = Some(AudioFormat::new(44100, 16, 2, AudioCodec::Mp3));
        StatusDisplay::display_technical_info(&status);
    }

    #[test]
    fn test_various_audio_formats() {
        let formats = [
            (AudioCodec::Flac, 44100, 16, 2),
            (AudioCodec::Wav, 48000, 24, 2),
            (AudioCodec::Alac, 96000, 24, 2),
            (AudioCodec::Mp3, 44100, 16, 2),
            (AudioCodec::OggVorbis, 44100, 16, 2),
        ];
        
        for (codec, sample_rate, bit_depth, channels) in formats {
            let format = AudioFormat::new(sample_rate, bit_depth, channels, codec);
            let track = create_test_track();
            let mut status = PlayerStatus::playing(track, Duration::from_secs(60), 1.0);
            status.audio_format = Some(format);
            
            // Should handle all format types without panicking
            StatusDisplay::display_technical_info(&status);
            StatusDisplay::display_compact_status(&status);
        }
    }
}