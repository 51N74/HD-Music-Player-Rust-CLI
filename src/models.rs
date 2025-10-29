use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;

/// Information about an audio track
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TrackInfo {
    pub path: PathBuf,
    pub metadata: AudioMetadata,
    pub duration: Duration,
    pub file_size: u64,
}

impl TrackInfo {
    pub fn new(path: PathBuf, metadata: AudioMetadata, duration: Duration, file_size: u64) -> Self {
        Self {
            path,
            metadata,
            duration,
            file_size,
        }
    }

    /// Get the display name for this track (title or filename)
    pub fn display_name(&self) -> String {
        self.metadata
            .title
            .clone()
            .unwrap_or_else(|| {
                self.path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("Unknown")
                    .to_string()
            })
    }

    /// Get the artist name or "Unknown Artist"
    pub fn artist_name(&self) -> String {
        self.metadata
            .artist
            .clone()
            .unwrap_or_else(|| "Unknown Artist".to_string())
    }

    /// Get the album name or "Unknown Album"
    pub fn album_name(&self) -> String {
        self.metadata
            .album
            .clone()
            .unwrap_or_else(|| "Unknown Album".to_string())
    }
}

/// Audio metadata extracted from files
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct AudioMetadata {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub track_number: Option<u32>,
    pub year: Option<u32>,
    pub genre: Option<String>,
}

impl AudioMetadata {
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if metadata has any information
    pub fn is_empty(&self) -> bool {
        self.title.is_none()
            && self.artist.is_none()
            && self.album.is_none()
            && self.track_number.is_none()
            && self.year.is_none()
            && self.genre.is_none()
    }

    /// Create metadata with basic information
    pub fn with_title_artist(title: String, artist: String) -> Self {
        Self {
            title: Some(title),
            artist: Some(artist),
            ..Default::default()
        }
    }
}

/// Audio format information
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AudioFormat {
    pub sample_rate: u32,
    pub bit_depth: u16,
    pub channels: u16,
    pub codec: AudioCodec,
}

impl AudioFormat {
    pub fn new(sample_rate: u32, bit_depth: u16, channels: u16, codec: AudioCodec) -> Self {
        Self {
            sample_rate,
            bit_depth,
            channels,
            codec,
        }
    }

    /// Get a human-readable format description
    pub fn format_description(&self) -> String {
        format!(
            "{} - {}-bit/{} Hz - {} channel{}",
            self.codec.name(),
            self.bit_depth,
            self.sample_rate,
            self.channels,
            if self.channels == 1 { "" } else { "s" }
        )
    }

    /// Check if this is a high-resolution format (>= 24-bit or >= 96kHz)
    pub fn is_high_resolution(&self) -> bool {
        self.bit_depth >= 24 || self.sample_rate >= 96000
    }

    /// Calculate approximate bitrate for uncompressed formats
    pub fn bitrate(&self) -> Option<u32> {
        match self.codec {
            AudioCodec::Wav | AudioCodec::Alac => {
                Some(self.sample_rate * self.bit_depth as u32 * self.channels as u32)
            }
            _ => None, // Compressed formats have variable bitrates
        }
    }
}

/// Supported audio codecs
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AudioCodec {
    Flac,
    Wav,
    Alac,
    Mp3,
    OggVorbis,
}

impl AudioCodec {
    /// Get the human-readable name of the codec
    pub fn name(&self) -> &'static str {
        match self {
            AudioCodec::Flac => "FLAC",
            AudioCodec::Wav => "WAV",
            AudioCodec::Alac => "ALAC",
            AudioCodec::Mp3 => "MP3",
            AudioCodec::OggVorbis => "OGG Vorbis",
        }
    }

    /// Check if the codec is lossless
    pub fn is_lossless(&self) -> bool {
        matches!(self, AudioCodec::Flac | AudioCodec::Wav | AudioCodec::Alac)
    }

    /// Get file extensions associated with this codec
    pub fn extensions(&self) -> &'static [&'static str] {
        match self {
            AudioCodec::Flac => &["flac"],
            AudioCodec::Wav => &["wav", "wave"],
            AudioCodec::Alac => &["m4a", "alac"],
            AudioCodec::Mp3 => &["mp3"],
            AudioCodec::OggVorbis => &["ogg", "oga"],
        }
    }
}

/// Current player status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerStatus {
    pub state: PlaybackState,
    pub current_track: Option<TrackInfo>,
    pub position: Duration,
    pub volume: f32,
    pub audio_format: Option<AudioFormat>,
    pub output_device: Option<String>,
}

impl PlayerStatus {
    pub fn new() -> Self {
        Self {
            state: PlaybackState::Stopped,
            current_track: None,
            position: Duration::from_secs(0),
            volume: 1.0,
            audio_format: None,
            output_device: None,
        }
    }

    /// Create a stopped status
    pub fn stopped() -> Self {
        Self::new()
    }

    /// Create a playing status with track info
    pub fn playing(track: TrackInfo, position: Duration, volume: f32) -> Self {
        Self {
            state: PlaybackState::Playing,
            current_track: Some(track),
            position,
            volume,
            audio_format: None,
            output_device: None,
        }
    }

    /// Create a paused status with track info
    pub fn paused(track: TrackInfo, position: Duration, volume: f32) -> Self {
        Self {
            state: PlaybackState::Paused,
            current_track: Some(track),
            position,
            volume,
            audio_format: None,
            output_device: None,
        }
    }

    /// Check if currently playing
    pub fn is_playing(&self) -> bool {
        matches!(self.state, PlaybackState::Playing)
    }

    /// Check if currently paused
    pub fn is_paused(&self) -> bool {
        matches!(self.state, PlaybackState::Paused)
    }

    /// Check if stopped
    pub fn is_stopped(&self) -> bool {
        matches!(self.state, PlaybackState::Stopped)
    }

    /// Get progress as a percentage (0.0 to 1.0)
    pub fn progress(&self) -> f32 {
        if let Some(track) = &self.current_track {
            if track.duration.as_secs() > 0 {
                self.position.as_secs_f32() / track.duration.as_secs_f32()
            } else {
                0.0
            }
        } else {
            0.0
        }
    }

    /// Format position as MM:SS
    pub fn position_formatted(&self) -> String {
        let total_seconds = self.position.as_secs();
        let minutes = total_seconds / 60;
        let seconds = total_seconds % 60;
        format!("{:02}:{:02}", minutes, seconds)
    }

    /// Format duration as MM:SS
    pub fn duration_formatted(&self) -> String {
        if let Some(track) = &self.current_track {
            let total_seconds = track.duration.as_secs();
            let minutes = total_seconds / 60;
            let seconds = total_seconds % 60;
            format!("{:02}:{:02}", minutes, seconds)
        } else {
            "00:00".to_string()
        }
    }
}

impl Default for PlayerStatus {
    fn default() -> Self {
        Self::new()
    }
}

/// Playback state enumeration
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum PlaybackState {
    Stopped,
    Playing,
    Paused,
}

impl PlaybackState {
    /// Get a human-readable string representation
    pub fn as_str(&self) -> &'static str {
        match self {
            PlaybackState::Stopped => "Stopped",
            PlaybackState::Playing => "Playing",
            PlaybackState::Paused => "Paused",
        }
    }
}

impl std::fmt::Display for PlaybackState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use std::path::PathBuf;

    #[test]
    fn test_track_info_creation() {
        let metadata = AudioMetadata::with_title_artist("Test Song".to_string(), "Test Artist".to_string());
        let path = PathBuf::from("/test/path/song.flac");
        let duration = Duration::from_secs(180);
        let file_size = 1024 * 1024; // 1MB

        let track = TrackInfo::new(path.clone(), metadata.clone(), duration, file_size);

        assert_eq!(track.path, path);
        assert_eq!(track.metadata, metadata);
        assert_eq!(track.duration, duration);
        assert_eq!(track.file_size, file_size);
    }

    #[test]
    fn test_track_info_display_name() {
        let metadata = AudioMetadata::with_title_artist("Test Song".to_string(), "Test Artist".to_string());
        let path = PathBuf::from("/test/path/song.flac");
        let track = TrackInfo::new(path, metadata, Duration::from_secs(180), 1024);

        assert_eq!(track.display_name(), "Test Song");

        // Test with no title
        let empty_metadata = AudioMetadata::new();
        let path = PathBuf::from("/test/path/song.flac");
        let track = TrackInfo::new(path, empty_metadata, Duration::from_secs(180), 1024);

        assert_eq!(track.display_name(), "song");
    }

    #[test]
    fn test_track_info_artist_name() {
        let metadata = AudioMetadata::with_title_artist("Test Song".to_string(), "Test Artist".to_string());
        let path = PathBuf::from("/test/path/song.flac");
        let track = TrackInfo::new(path, metadata, Duration::from_secs(180), 1024);

        assert_eq!(track.artist_name(), "Test Artist");

        // Test with no artist
        let empty_metadata = AudioMetadata::new();
        let path = PathBuf::from("/test/path/song.flac");
        let track = TrackInfo::new(path, empty_metadata, Duration::from_secs(180), 1024);

        assert_eq!(track.artist_name(), "Unknown Artist");
    }

    #[test]
    fn test_track_info_album_name() {
        let mut metadata = AudioMetadata::new();
        metadata.album = Some("Test Album".to_string());
        let path = PathBuf::from("/test/path/song.flac");
        let track = TrackInfo::new(path, metadata, Duration::from_secs(180), 1024);

        assert_eq!(track.album_name(), "Test Album");

        // Test with no album
        let empty_metadata = AudioMetadata::new();
        let path = PathBuf::from("/test/path/song.flac");
        let track = TrackInfo::new(path, empty_metadata, Duration::from_secs(180), 1024);

        assert_eq!(track.album_name(), "Unknown Album");
    }

    #[test]
    fn test_audio_metadata_creation() {
        let metadata = AudioMetadata::new();
        assert!(metadata.is_empty());

        let metadata = AudioMetadata::with_title_artist("Title".to_string(), "Artist".to_string());
        assert!(!metadata.is_empty());
        assert_eq!(metadata.title, Some("Title".to_string()));
        assert_eq!(metadata.artist, Some("Artist".to_string()));
    }

    #[test]
    fn test_audio_metadata_is_empty() {
        let empty_metadata = AudioMetadata::new();
        assert!(empty_metadata.is_empty());

        let mut metadata = AudioMetadata::new();
        metadata.title = Some("Title".to_string());
        assert!(!metadata.is_empty());
    }

    #[test]
    fn test_audio_format_creation() {
        let format = AudioFormat::new(44100, 16, 2, AudioCodec::Flac);
        
        assert_eq!(format.sample_rate, 44100);
        assert_eq!(format.bit_depth, 16);
        assert_eq!(format.channels, 2);
        assert_eq!(format.codec, AudioCodec::Flac);
    }

    #[test]
    fn test_audio_format_description() {
        let format = AudioFormat::new(44100, 16, 2, AudioCodec::Flac);
        assert_eq!(format.format_description(), "FLAC - 16-bit/44100 Hz - 2 channels");

        let mono_format = AudioFormat::new(48000, 24, 1, AudioCodec::Wav);
        assert_eq!(mono_format.format_description(), "WAV - 24-bit/48000 Hz - 1 channel");
    }

    #[test]
    fn test_audio_format_is_high_resolution() {
        let cd_quality = AudioFormat::new(44100, 16, 2, AudioCodec::Flac);
        assert!(!cd_quality.is_high_resolution());

        let high_res_bit_depth = AudioFormat::new(44100, 24, 2, AudioCodec::Flac);
        assert!(high_res_bit_depth.is_high_resolution());

        let high_res_sample_rate = AudioFormat::new(96000, 16, 2, AudioCodec::Flac);
        assert!(high_res_sample_rate.is_high_resolution());

        let high_res_both = AudioFormat::new(192000, 32, 2, AudioCodec::Wav);
        assert!(high_res_both.is_high_resolution());
    }

    #[test]
    fn test_audio_format_bitrate() {
        let wav_format = AudioFormat::new(44100, 16, 2, AudioCodec::Wav);
        assert_eq!(wav_format.bitrate(), Some(44100 * 16 * 2));

        let alac_format = AudioFormat::new(48000, 24, 2, AudioCodec::Alac);
        assert_eq!(alac_format.bitrate(), Some(48000 * 24 * 2));

        let flac_format = AudioFormat::new(44100, 16, 2, AudioCodec::Flac);
        assert_eq!(flac_format.bitrate(), None); // Compressed format
    }

    #[test]
    fn test_audio_codec_properties() {
        assert_eq!(AudioCodec::Flac.name(), "FLAC");
        assert!(AudioCodec::Flac.is_lossless());
        assert_eq!(AudioCodec::Flac.extensions(), &["flac"]);

        assert_eq!(AudioCodec::Mp3.name(), "MP3");
        assert!(!AudioCodec::Mp3.is_lossless());
        assert_eq!(AudioCodec::Mp3.extensions(), &["mp3"]);

        assert_eq!(AudioCodec::Wav.name(), "WAV");
        assert!(AudioCodec::Wav.is_lossless());
        assert_eq!(AudioCodec::Wav.extensions(), &["wav", "wave"]);

        assert_eq!(AudioCodec::Alac.name(), "ALAC");
        assert!(AudioCodec::Alac.is_lossless());
        assert_eq!(AudioCodec::Alac.extensions(), &["m4a", "alac"]);

        assert_eq!(AudioCodec::OggVorbis.name(), "OGG Vorbis");
        assert!(!AudioCodec::OggVorbis.is_lossless());
        assert_eq!(AudioCodec::OggVorbis.extensions(), &["ogg", "oga"]);
    }

    #[test]
    fn test_player_status_creation() {
        let status = PlayerStatus::new();
        assert_eq!(status.state, PlaybackState::Stopped);
        assert!(status.current_track.is_none());
        assert_eq!(status.position, Duration::from_secs(0));
        assert_eq!(status.volume, 1.0);

        let status = PlayerStatus::stopped();
        assert!(status.is_stopped());
        assert!(!status.is_playing());
        assert!(!status.is_paused());
    }

    #[test]
    fn test_player_status_playing() {
        let metadata = AudioMetadata::with_title_artist("Test Song".to_string(), "Test Artist".to_string());
        let track = TrackInfo::new(
            PathBuf::from("/test/song.flac"),
            metadata,
            Duration::from_secs(180),
            1024
        );
        let position = Duration::from_secs(60);
        let volume = 0.8;

        let status = PlayerStatus::playing(track.clone(), position, volume);
        
        assert!(status.is_playing());
        assert!(!status.is_stopped());
        assert!(!status.is_paused());
        assert_eq!(status.current_track, Some(track));
        assert_eq!(status.position, position);
        assert_eq!(status.volume, volume);
    }

    #[test]
    fn test_player_status_paused() {
        let metadata = AudioMetadata::with_title_artist("Test Song".to_string(), "Test Artist".to_string());
        let track = TrackInfo::new(
            PathBuf::from("/test/song.flac"),
            metadata,
            Duration::from_secs(180),
            1024
        );
        let position = Duration::from_secs(60);
        let volume = 0.8;

        let status = PlayerStatus::paused(track.clone(), position, volume);
        
        assert!(status.is_paused());
        assert!(!status.is_stopped());
        assert!(!status.is_playing());
        assert_eq!(status.current_track, Some(track));
        assert_eq!(status.position, position);
        assert_eq!(status.volume, volume);
    }

    #[test]
    fn test_player_status_progress() {
        let metadata = AudioMetadata::with_title_artist("Test Song".to_string(), "Test Artist".to_string());
        let track = TrackInfo::new(
            PathBuf::from("/test/song.flac"),
            metadata,
            Duration::from_secs(180), // 3 minutes
            1024
        );
        let position = Duration::from_secs(60); // 1 minute

        let status = PlayerStatus::playing(track, position, 1.0);
        
        // Should be 1/3 = 0.333...
        let progress = status.progress();
        assert!((progress - 0.333333).abs() < 0.001);

        // Test with no track
        let empty_status = PlayerStatus::new();
        assert_eq!(empty_status.progress(), 0.0);
    }

    #[test]
    fn test_player_status_formatting() {
        let metadata = AudioMetadata::with_title_artist("Test Song".to_string(), "Test Artist".to_string());
        let track = TrackInfo::new(
            PathBuf::from("/test/song.flac"),
            metadata,
            Duration::from_secs(185), // 3:05
            1024
        );
        let position = Duration::from_secs(65); // 1:05

        let status = PlayerStatus::playing(track, position, 1.0);
        
        assert_eq!(status.position_formatted(), "01:05");
        assert_eq!(status.duration_formatted(), "03:05");

        // Test with no track
        let empty_status = PlayerStatus::new();
        assert_eq!(empty_status.duration_formatted(), "00:00");
    }

    #[test]
    fn test_playback_state_display() {
        assert_eq!(PlaybackState::Stopped.as_str(), "Stopped");
        assert_eq!(PlaybackState::Playing.as_str(), "Playing");
        assert_eq!(PlaybackState::Paused.as_str(), "Paused");

        assert_eq!(format!("{}", PlaybackState::Playing), "Playing");
    }

    #[test]
    fn test_audio_buffer_creation() {
        let buffer = AudioBuffer::new(2, 44100, 1024);
        
        assert_eq!(buffer.channels, 2);
        assert_eq!(buffer.sample_rate, 44100);
        assert_eq!(buffer.frames, 1024);
        assert_eq!(buffer.total_samples(), 2048); // 2 channels * 1024 frames
        assert!(!buffer.is_empty());

        let empty_buffer = AudioBuffer::empty();
        assert!(empty_buffer.is_empty());
        assert_eq!(empty_buffer.total_samples(), 0);
    }

    #[test]
    fn test_audio_buffer_duration() {
        let buffer = AudioBuffer::new(2, 44100, 44100); // 1 second of audio
        let duration = buffer.duration();
        
        // Should be approximately 1 second
        assert!((duration.as_secs_f64() - 1.0).abs() < 0.001);

        let empty_buffer = AudioBuffer::empty();
        assert_eq!(empty_buffer.duration(), Duration::from_secs(0));
    }

    #[test]
    fn test_serialization_deserialization() {
        // Test TrackInfo serialization
        let metadata = AudioMetadata::with_title_artist("Test Song".to_string(), "Test Artist".to_string());
        let track = TrackInfo::new(
            PathBuf::from("/test/song.flac"),
            metadata,
            Duration::from_secs(180),
            1024
        );

        let serialized = serde_json::to_string(&track).expect("Failed to serialize TrackInfo");
        let deserialized: TrackInfo = serde_json::from_str(&serialized).expect("Failed to deserialize TrackInfo");
        assert_eq!(track, deserialized);

        // Test AudioFormat serialization
        let format = AudioFormat::new(44100, 16, 2, AudioCodec::Flac);
        let serialized = serde_json::to_string(&format).expect("Failed to serialize AudioFormat");
        let deserialized: AudioFormat = serde_json::from_str(&serialized).expect("Failed to deserialize AudioFormat");
        assert_eq!(format, deserialized);

        // Test PlayerStatus serialization
        let status = PlayerStatus::playing(track.clone(), Duration::from_secs(60), 0.8);
        let serialized = serde_json::to_string(&status).expect("Failed to serialize PlayerStatus");
        let deserialized: PlayerStatus = serde_json::from_str(&serialized).expect("Failed to deserialize PlayerStatus");
        assert_eq!(status.state, deserialized.state);
        assert_eq!(status.position, deserialized.position);
        assert_eq!(status.volume, deserialized.volume);
    }
}

/// Audio buffer for sample data
#[derive(Debug, Clone)]
pub struct AudioBuffer {
    pub samples: Vec<f32>,
    pub channels: u16,
    pub sample_rate: u32,
    pub frames: usize,
}

impl AudioBuffer {
    pub fn new(channels: u16, sample_rate: u32, frames: usize) -> Self {
        let samples = vec![0.0; frames * channels as usize];
        Self {
            samples,
            channels,
            sample_rate,
            frames,
        }
    }

    /// Create an empty buffer
    pub fn empty() -> Self {
        Self {
            samples: Vec::new(),
            channels: 0,
            sample_rate: 0,
            frames: 0,
        }
    }

    /// Get the number of samples per channel
    pub fn frames(&self) -> usize {
        self.frames
    }

    /// Get the total number of samples
    pub fn total_samples(&self) -> usize {
        self.samples.len()
    }

    /// Check if the buffer is empty
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// Get duration of this buffer
    pub fn duration(&self) -> Duration {
        if self.sample_rate > 0 {
            Duration::from_secs_f64(self.frames as f64 / self.sample_rate as f64)
        } else {
            Duration::from_secs(0)
        }
    }
}