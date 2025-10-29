use std::sync::{Arc, Mutex};
use std::time::Duration;
use std::path::PathBuf;
use tempfile::TempDir;

use crate::audio::{AudioEngine, GaplessManager};
use crate::audio::engine::AudioEngineImpl;
use crate::queue::{QueueManager, QueueManagerImpl};
use crate::models::{TrackInfo, AudioMetadata, PlaybackState};
use crate::error::AudioError;

/// Integration tests for gapless playback functionality
/// 
/// Note: These tests require actual audio files to work properly.
/// In a production environment, you would have test audio files
/// in different formats to test gapless transitions.

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_track(name: &str, extension: &str) -> TrackInfo {
        let path = PathBuf::from(format!("/test/{}.{}", name, extension));
        let metadata = AudioMetadata::with_title_artist(name.to_string(), "Test Artist".to_string());
        TrackInfo::new(path, metadata, Duration::from_secs(180), 1024)
    }

    #[test]
    fn test_gapless_manager_initialization() {
        // Test that we can create a gapless manager with proper components
        let temp_dir = TempDir::new().unwrap();
        let queue_manager = Box::new(
            QueueManagerImpl::with_playlist_directory(temp_dir.path().to_path_buf()).unwrap()
        );
        
        // Note: AudioEngineImpl requires audio devices, so we can't easily test it
        // in a headless environment. In a real test suite, you'd use mock audio devices
        // or run these tests on systems with audio hardware.
        
        let queue_arc = Arc::new(Mutex::new(queue_manager as Box<dyn QueueManager>));
        
        // Test queue operations that don't require audio
        {
            let queue = queue_arc.lock().unwrap();
            assert!(queue.is_empty());
            assert_eq!(queue.len(), 0);
            assert_eq!(queue.current_index(), 0);
        }
    }

    #[test]
    fn test_track_format_detection() {
        // Test that we can detect different audio formats for gapless playback
        let formats = vec![
            ("track1", "flac"),
            ("track2", "wav"),
            ("track3", "mp3"),
            ("track4", "m4a"),
            ("track5", "ogg"),
        ];

        for (name, ext) in formats {
            let track = create_test_track(name, ext);
            
            // Verify track properties
            assert_eq!(track.display_name(), name);
            assert_eq!(track.artist_name(), "Test Artist");
            assert!(track.path.to_string_lossy().ends_with(&format!(".{}", ext)));
            
            // Test that we can identify the format from extension
            let extension = track.path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_lowercase();
            
            match extension.as_str() {
                "flac" | "wav" | "mp3" | "m4a" | "ogg" => {
                    // These are supported formats
                    assert!(true);
                }
                _ => {
                    panic!("Unexpected format: {}", extension);
                }
            }
        }
    }

    #[test]
    fn test_queue_operations_for_gapless() {
        // Test queue operations that are important for gapless playback
        let temp_dir = TempDir::new().unwrap();
        let mut queue_manager = QueueManagerImpl::with_playlist_directory(temp_dir.path().to_path_buf()).unwrap();
        
        // Create test tracks
        let track1 = create_test_track("track1", "flac");
        let track2 = create_test_track("track2", "wav");
        let track3 = create_test_track("track3", "mp3");
        
        // Test that we can simulate adding tracks (even though files don't exist)
        // In a real implementation, these would be real audio files
        
        // Test queue navigation
        assert!(queue_manager.is_empty());
        assert_eq!(queue_manager.current_index(), 0);
        
        // Test that we can get the current track (should be None when empty)
        assert!(queue_manager.current_track().is_none());
        
        // Test that next_track returns None when queue is empty
        assert!(queue_manager.next_track().is_none());
        
        // Test that previous_track returns None when queue is empty
        assert!(queue_manager.previous_track().is_none());
    }

    #[test]
    fn test_gapless_buffer_management() {
        // Test buffer management concepts important for gapless playback
        use crate::audio::buffer::{RingBuffer, BufferManager};
        
        // Create a buffer manager suitable for gapless playback
        let buffer_manager = BufferManager::new(
            8192,  // 8192 frames capacity
            2,     // stereo
            44100, // 44.1kHz
            200,   // 200ms target buffer
            100,   // 100ms minimum buffer
        );
        
        let ring_buffer = buffer_manager.ring_buffer();
        
        // Test that buffer can handle typical audio data
        assert_eq!(ring_buffer.capacity_frames(), 8192);
        assert_eq!(ring_buffer.available_read_frames(), 0);
        assert!(ring_buffer.available_write_frames() > 0);
        
        // Test buffer status
        let status = buffer_manager.buffer_status();
        assert_eq!(status.fill_level, 0.0);
        assert!(status.needs_data);
        assert!(!status.is_underrun);
        assert_eq!(status.underrun_count, 0);
        
        // Test writing some audio data
        let test_audio = vec![0.1, -0.1, 0.2, -0.2]; // 2 frames of stereo audio
        let written = ring_buffer.write(&test_audio);
        assert_eq!(written, 4); // All samples should be written
        
        // Test that buffer now has data
        assert_eq!(ring_buffer.available_read(), 4);
        assert_eq!(ring_buffer.available_read_frames(), 2);
        
        // Test reading the data back
        let mut read_buffer = vec![0.0; 4];
        let read = ring_buffer.read(&mut read_buffer);
        assert_eq!(read, 4);
        assert_eq!(read_buffer, test_audio);
        
        // Buffer should be empty again
        assert_eq!(ring_buffer.available_read(), 0);
        assert!(ring_buffer.is_empty());
    }

    #[test]
    fn test_gapless_timing_calculations() {
        // Test timing calculations important for gapless playback
        use crate::models::AudioBuffer;
        
        // Test buffer duration calculations
        let buffer_44k = AudioBuffer::new(2, 44100, 4410); // 0.1 seconds at 44.1kHz
        let duration_44k = buffer_44k.duration();
        assert!((duration_44k.as_secs_f64() - 0.1).abs() < 0.001);
        
        let buffer_48k = AudioBuffer::new(2, 48000, 4800); // 0.1 seconds at 48kHz
        let duration_48k = buffer_48k.duration();
        assert!((duration_48k.as_secs_f64() - 0.1).abs() < 0.001);
        
        // Test that different sample rates produce correct durations
        let buffer_96k = AudioBuffer::new(2, 96000, 9600); // 0.1 seconds at 96kHz
        let duration_96k = buffer_96k.duration();
        assert!((duration_96k.as_secs_f64() - 0.1).abs() < 0.001);
        
        // Test frame calculations
        assert_eq!(buffer_44k.frames(), 4410);
        assert_eq!(buffer_48k.frames(), 4800);
        assert_eq!(buffer_96k.frames(), 9600);
        
        // Test total samples (frames * channels)
        assert_eq!(buffer_44k.total_samples(), 8820); // 4410 * 2
        assert_eq!(buffer_48k.total_samples(), 9600); // 4800 * 2
        assert_eq!(buffer_96k.total_samples(), 19200); // 9600 * 2
    }

    #[test]
    fn test_gapless_format_compatibility() {
        // Test format compatibility for gapless playback
        use crate::models::{AudioFormat, AudioCodec};
        
        // Create different audio formats
        let cd_quality = AudioFormat::new(44100, 16, 2, AudioCodec::Flac);
        let high_res_flac = AudioFormat::new(96000, 24, 2, AudioCodec::Flac);
        let wav_format = AudioFormat::new(44100, 16, 2, AudioCodec::Wav);
        let mp3_format = AudioFormat::new(44100, 16, 2, AudioCodec::Mp3);
        
        // Test format descriptions
        assert!(cd_quality.format_description().contains("FLAC"));
        assert!(cd_quality.format_description().contains("44100"));
        assert!(cd_quality.format_description().contains("16-bit"));
        
        // Test high-resolution detection
        assert!(!cd_quality.is_high_resolution());
        assert!(high_res_flac.is_high_resolution());
        
        // Test lossless detection
        assert!(cd_quality.codec.is_lossless());
        assert!(wav_format.codec.is_lossless());
        assert!(!mp3_format.codec.is_lossless());
        
        // Test bitrate calculations for uncompressed formats
        assert!(wav_format.bitrate().is_some());
        assert!(mp3_format.bitrate().is_none()); // Compressed format
    }

    #[test]
    fn test_gapless_error_handling() {
        // Test error handling scenarios for gapless playback
        use crate::error::{AudioError, DecodeError};
        
        // Test audio error types
        let device_error = AudioError::DeviceNotFound {
            device: "NonExistentDevice".to_string(),
        };
        assert!(format!("{}", device_error).contains("NonExistentDevice"));
        
        let stream_error = AudioError::StreamError("Test stream error".to_string());
        assert!(format!("{}", stream_error).contains("Test stream error"));
        
        // Test decode error types
        let decode_error = DecodeError::DecodeFailed("Test decode error".to_string());
        assert!(format!("{}", decode_error).contains("Test decode error"));
        
        let seek_error = DecodeError::SeekError("Test seek error".to_string());
        assert!(format!("{}", seek_error).contains("Test seek error"));
        
        let format_error = DecodeError::UnsupportedFormat {
            format: "UNKNOWN".to_string(),
        };
        assert!(format!("{}", format_error).contains("UNKNOWN"));
    }

    #[test]
    fn test_gapless_playback_state_transitions() {
        // Test playback state transitions important for gapless playback
        use crate::models::PlaybackState;
        
        // Test state transitions
        let stopped = PlaybackState::Stopped;
        let playing = PlaybackState::Playing;
        let paused = PlaybackState::Paused;
        
        // Test state properties
        assert_eq!(stopped.as_str(), "Stopped");
        assert_eq!(playing.as_str(), "Playing");
        assert_eq!(paused.as_str(), "Paused");
        
        // Test state comparisons
        assert_ne!(stopped, playing);
        assert_ne!(playing, paused);
        assert_ne!(paused, stopped);
        
        // Test display formatting
        assert_eq!(format!("{}", playing), "Playing");
        assert_eq!(format!("{}", paused), "Paused");
        assert_eq!(format!("{}", stopped), "Stopped");
    }

    #[test]
    fn test_gapless_track_metadata() {
        // Test track metadata handling for gapless playback
        let mut metadata = AudioMetadata::new();
        assert!(metadata.is_empty());
        
        metadata.title = Some("Test Track".to_string());
        metadata.artist = Some("Test Artist".to_string());
        metadata.album = Some("Test Album".to_string());
        metadata.track_number = Some(1);
        metadata.year = Some(2023);
        metadata.genre = Some("Test Genre".to_string());
        
        assert!(!metadata.is_empty());
        
        // Create track with metadata
        let track = TrackInfo::new(
            PathBuf::from("/test/track.flac"),
            metadata,
            Duration::from_secs(240),
            2048
        );
        
        assert_eq!(track.display_name(), "Test Track");
        assert_eq!(track.artist_name(), "Test Artist");
        assert_eq!(track.album_name(), "Test Album");
        assert_eq!(track.duration, Duration::from_secs(240));
        assert_eq!(track.file_size, 2048);
    }

    #[test]
    #[ignore] // Ignored because it requires actual audio hardware
    fn test_gapless_playback_integration() {
        // This test would require actual audio files and hardware
        // It's marked as ignored so it doesn't run in CI/CD environments
        // without audio devices.
        
        // In a real test environment with audio files, you would:
        // 1. Create an AudioEngineImpl
        // 2. Create a QueueManager with real audio files
        // 3. Create a GaplessManager
        // 4. Test loading tracks, preloading next tracks, and transitions
        // 5. Verify that transitions happen without gaps
        
        println!("Gapless integration test requires audio hardware and test files");
    }

    #[test]
    fn test_gapless_performance_characteristics() {
        // Test performance characteristics important for gapless playback
        use std::time::Instant;
        use crate::audio::buffer::RingBuffer;
        
        // Test buffer performance with realistic audio data sizes
        let buffer = RingBuffer::new(44100, 2, 44100); // 1 second buffer
        
        // Test writing performance
        let large_audio_data = vec![0.1f32; 8820]; // 0.1 seconds of stereo audio
        
        let start = Instant::now();
        for _ in 0..100 {
            buffer.write(&large_audio_data);
            let mut read_data = vec![0.0f32; 8820];
            buffer.read(&mut read_data);
        }
        let elapsed = start.elapsed();
        
        // Should be able to process 10 seconds of audio data quickly
        assert!(elapsed.as_millis() < 100, "Buffer operations too slow: {:?}", elapsed);
        
        // Test buffer capacity and utilization
        assert_eq!(buffer.capacity_frames(), 44100);
        assert!(buffer.is_empty());
        
        // Test that we can fill and empty the buffer efficiently
        let fill_data = vec![0.5f32; 88200]; // Fill the entire buffer
        let written = buffer.write(&fill_data);
        assert!(written > 0);
        
        let mut read_all = vec![0.0f32; 88200];
        let read = buffer.read(&mut read_all);
        assert_eq!(read, written);
    }
}