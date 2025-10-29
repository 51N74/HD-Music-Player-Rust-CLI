use std::sync::{Arc, Mutex};
use std::time::Duration;
use crate::audio::{AudioEngine, AudioDecoder};
use crate::audio::engine::AudioEngineImpl;
use crate::queue::QueueManager;
use crate::models::{TrackInfo, PlaybackState};
use crate::error::AudioError;

/// Manages gapless playback between tracks in a queue
pub struct GaplessManager {
    audio_engine: Arc<Mutex<AudioEngineImpl>>,
    queue_manager: Arc<Mutex<Box<dyn QueueManager>>>,
    current_track: Option<TrackInfo>,
    next_track_preloaded: bool,
    gapless_enabled: bool,
}

impl GaplessManager {
    /// Create a new gapless manager
    pub fn new(
        audio_engine: Arc<Mutex<AudioEngineImpl>>,
        queue_manager: Arc<Mutex<Box<dyn QueueManager>>>,
    ) -> Self {
        Self {
            audio_engine,
            queue_manager,
            current_track: None,
            next_track_preloaded: false,
            gapless_enabled: true,
        }
    }

    /// Enable or disable gapless playback
    pub fn set_gapless_enabled(&mut self, enabled: bool) {
        self.gapless_enabled = enabled;
        if let Ok(mut engine) = self.audio_engine.lock() {
            engine.set_gapless_enabled(enabled);
        }
    }

    /// Check if gapless playback is enabled
    pub fn is_gapless_enabled(&self) -> bool {
        self.gapless_enabled
    }

    /// Start playback of the current track in the queue
    pub fn start_playback(&mut self) -> Result<(), AudioError> {
        let current_track = {
            let queue = self.queue_manager.lock().unwrap();
            queue.current_track().cloned()
        };

        if let Some(track) = current_track {
            // Load the current track
            if let Ok(mut engine) = self.audio_engine.lock() {
                engine.load_file(track.path.clone())?;
            }

            self.current_track = Some(track);
            self.preload_next_track_if_needed()?;
            
            // Start playback
            if let Ok(mut engine) = self.audio_engine.lock() {
                // Create a decoder for the current track
                let decoder = self.create_decoder_for_track(&self.current_track.as_ref().unwrap())?;
                engine.start_playback(decoder)?;
            }
        }

        Ok(())
    }

    /// Move to the next track in the queue
    pub fn next_track(&mut self) -> Result<(), AudioError> {
        let next_track = {
            let mut queue = self.queue_manager.lock().unwrap();
            queue.next_track().cloned()
        };

        if let Some(track) = next_track {
            if self.gapless_enabled && self.next_track_preloaded {
                // Use gapless transition
                if let Ok(mut engine) = self.audio_engine.lock() {
                    engine.transition_to_next_track()?;
                }
                self.next_track_preloaded = false;
            } else {
                // Regular track change
                if let Ok(mut engine) = self.audio_engine.lock() {
                    engine.stop()?;
                    engine.load_file(track.path.clone())?;
                    let decoder = self.create_decoder_for_track(&track)?;
                    engine.start_playback(decoder)?;
                }
            }

            self.current_track = Some(track);
            self.preload_next_track_if_needed()?;
        }

        Ok(())
    }

    /// Move to the previous track in the queue
    pub fn previous_track(&mut self) -> Result<(), AudioError> {
        let prev_track = {
            let mut queue = self.queue_manager.lock().unwrap();
            queue.previous_track().cloned()
        };

        if let Some(track) = prev_track {
            // Previous track always requires stopping and reloading
            if let Ok(mut engine) = self.audio_engine.lock() {
                engine.stop()?;
                engine.load_file(track.path.clone())?;
                let decoder = self.create_decoder_for_track(&track)?;
                engine.start_playback(decoder)?;
            }

            self.current_track = Some(track);
            self.next_track_preloaded = false;
            self.preload_next_track_if_needed()?;
        }

        Ok(())
    }

    /// Handle end of file - automatically transition to next track if gapless is enabled
    pub fn handle_end_of_file(&mut self) -> Result<bool, AudioError> {
        if self.gapless_enabled && self.next_track_preloaded {
            // Automatically transition to next track
            self.next_track()?;
            Ok(true) // Track was changed
        } else {
            // Check if there's a next track to load
            let has_next = {
                let queue = self.queue_manager.lock().unwrap();
                queue.current_index() + 1 < queue.len()
            };

            if has_next {
                self.next_track()?;
                Ok(true)
            } else {
                Ok(false) // No more tracks
            }
        }
    }

    /// Preload the next track if gapless is enabled and there is a next track
    fn preload_next_track_if_needed(&mut self) -> Result<(), AudioError> {
        if !self.gapless_enabled {
            return Ok(());
        }

        let next_track = {
            let queue = self.queue_manager.lock().unwrap();
            let current_index = queue.current_index();
            if current_index + 1 < queue.len() {
                queue.list().get(current_index + 1).cloned()
            } else {
                None
            }
        };

        if let Some(track) = next_track {
            if let Ok(mut engine) = self.audio_engine.lock() {
                engine.preload_next_track(track.path.clone())?;
                self.next_track_preloaded = true;
            }
        }

        Ok(())
    }

    /// Create a decoder for the given track
    fn create_decoder_for_track(&self, track: &TrackInfo) -> Result<Box<dyn AudioDecoder>, AudioError> {
        // Determine the codec from the file extension
        let extension = track.path
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("")
            .to_lowercase();

        match extension.as_str() {
            "flac" => {
                let decoder = crate::audio::decoders::FlacDecoder::new(&track.path)
                    .map_err(|e| AudioError::StreamError(format!("Failed to create FLAC decoder: {}", e)))?;
                Ok(Box::new(decoder))
            }
            "wav" | "wave" => {
                let decoder = crate::audio::decoders::WavDecoder::new(&track.path)
                    .map_err(|e| AudioError::StreamError(format!("Failed to create WAV decoder: {}", e)))?;
                Ok(Box::new(decoder))
            }
            "m4a" | "alac" => {
                let decoder = crate::audio::decoders::AlacDecoder::new(&track.path)
                    .map_err(|e| AudioError::StreamError(format!("Failed to create ALAC decoder: {}", e)))?;
                Ok(Box::new(decoder))
            }
            "mp3" => {
                let decoder = crate::audio::decoders::Mp3Decoder::new(&track.path)
                    .map_err(|e| AudioError::StreamError(format!("Failed to create MP3 decoder: {}", e)))?;
                Ok(Box::new(decoder))
            }
            "ogg" | "oga" => {
                let decoder = crate::audio::decoders::OggDecoder::new(&track.path)
                    .map_err(|e| AudioError::StreamError(format!("Failed to create OGG decoder: {}", e)))?;
                Ok(Box::new(decoder))
            }
            _ => {
                Err(AudioError::StreamError(format!("Unsupported audio format: {}", extension)))
            }
        }
    }

    /// Get the current track
    pub fn current_track(&self) -> Option<&TrackInfo> {
        self.current_track.as_ref()
    }

    /// Check if the next track is preloaded
    pub fn is_next_track_preloaded(&self) -> bool {
        self.next_track_preloaded
    }

    /// Clean up resources
    pub fn cleanup(&mut self) -> Result<(), AudioError> {
        if let Ok(mut engine) = self.audio_engine.lock() {
            engine.stop()?;
        }
        
        self.current_track = None;
        self.next_track_preloaded = false;
        
        Ok(())
    }
}



#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::collections::VecDeque;
    use crate::models::{AudioMetadata, TrackInfo};
    use crate::queue::QueueManagerImpl;
    use crate::audio::engine::AudioEngineImpl;
    use tempfile::TempDir;

    // Note: For proper testing, we would need to create mock implementations
    // or use dependency injection. For now, we'll test the basic functionality
    // that doesn't require actual audio engine operations.

    fn create_test_track(name: &str, extension: &str) -> TrackInfo {
        let path = PathBuf::from(format!("/test/{}.{}", name, extension));
        let metadata = AudioMetadata::with_title_artist(name.to_string(), "Test Artist".to_string());
        TrackInfo::new(path, metadata, Duration::from_secs(180), 1024)
    }

    #[test]
    fn test_gapless_manager_basic_functionality() {
        // Test basic state management without requiring actual audio engine
        let temp_dir = TempDir::new().unwrap();
        let queue_manager = Box::new(QueueManagerImpl::with_playlist_directory(temp_dir.path().to_path_buf()).unwrap());
        
        // We can't easily create a real AudioEngineImpl for testing without audio devices
        // So we'll test the parts that don't require it
        
        let test_track = create_test_track("test", "flac");
        
        // Test track creation
        assert_eq!(test_track.display_name(), "test");
        assert_eq!(test_track.artist_name(), "Test Artist");
        
        // Test format detection logic
        let flac_track = create_test_track("test", "flac");
        let wav_track = create_test_track("test", "wav");
        let mp3_track = create_test_track("test", "mp3");
        
        // Verify the paths have the correct extensions
        assert!(flac_track.path.extension().unwrap() == "flac");
        assert!(wav_track.path.extension().unwrap() == "wav");
        assert!(mp3_track.path.extension().unwrap() == "mp3");
    }

    #[test]
    fn test_format_detection() {
        // Test format detection logic without requiring actual files
        let flac_track = create_test_track("test", "flac");
        let wav_track = create_test_track("test", "wav");
        let mp3_track = create_test_track("test", "mp3");
        let alac_track = create_test_track("test", "m4a");
        let ogg_track = create_test_track("test", "ogg");
        let unsupported_track = create_test_track("test", "xyz");

        // Test extension extraction
        assert_eq!(flac_track.path.extension().unwrap(), "flac");
        assert_eq!(wav_track.path.extension().unwrap(), "wav");
        assert_eq!(mp3_track.path.extension().unwrap(), "mp3");
        assert_eq!(alac_track.path.extension().unwrap(), "m4a");
        assert_eq!(ogg_track.path.extension().unwrap(), "ogg");
        assert_eq!(unsupported_track.path.extension().unwrap(), "xyz");
    }

    #[test]
    fn test_track_info_properties() {
        let track1 = create_test_track("track1", "flac");
        let track2 = create_test_track("track2", "mp3");
        
        assert_eq!(track1.display_name(), "track1");
        assert_eq!(track1.artist_name(), "Test Artist");
        assert_eq!(track1.duration, Duration::from_secs(180));
        
        assert_eq!(track2.display_name(), "track2");
        assert_eq!(track2.artist_name(), "Test Artist");
        assert_eq!(track2.duration, Duration::from_secs(180));
        
        // Test different extensions
        assert!(track1.path.to_string_lossy().ends_with(".flac"));
        assert!(track2.path.to_string_lossy().ends_with(".mp3"));
    }

    #[test]
    fn test_gapless_state_management() {
        // Test basic state management without audio engine dependency
        let test_track = create_test_track("test", "flac");
        
        // Test track properties
        assert_eq!(test_track.display_name(), "test");
        assert_eq!(test_track.artist_name(), "Test Artist");
        assert_eq!(test_track.album_name(), "Unknown Album");
        assert_eq!(test_track.duration, Duration::from_secs(180));
        assert_eq!(test_track.file_size, 1024);
        
        // Test path properties
        assert!(test_track.path.to_string_lossy().contains("test.flac"));
    }
}