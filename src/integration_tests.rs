#[cfg(test)]
mod integration_tests {
    use super::*;
    use crate::{AppController, Commands, PlayerError};
    use crate::cli::{QueueAction, PlaylistAction, DeviceAction};
    use crate::models;
    use crate::error;
    use crate::queue::QueueManager;
    use std::path::PathBuf;
    use std::time::Duration;
    use tempfile::TempDir;

    /// Create a test audio file (dummy content for testing)
    fn create_test_audio_file(dir: &std::path::Path, name: &str, extension: &str) -> PathBuf {
        let file_path = dir.join(format!("{}.{}", name, extension));
        std::fs::write(&file_path, b"dummy audio data").unwrap();
        file_path
    }

    /// Create a test directory structure with audio files
    fn create_test_directory_structure() -> TempDir {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path();

        // Create some test audio files
        create_test_audio_file(root, "song1", "flac");
        create_test_audio_file(root, "song2", "mp3");
        create_test_audio_file(root, "song3", "wav");
        
        // Create a subdirectory with more files
        let subdir = root.join("subdir");
        std::fs::create_dir(&subdir).unwrap();
        create_test_audio_file(&subdir, "song4", "ogg");
        create_test_audio_file(&subdir, "song5", "m4a");

        temp_dir
    }

    #[tokio::test]
    async fn test_app_controller_creation() {
        let result = AppController::new();
        assert!(result.is_ok(), "AppController creation should succeed");
        
        let mut app = result.unwrap();
        let init_result = app.initialize();
        assert!(init_result.is_ok(), "AppController initialization should succeed");
    }

    #[tokio::test]
    async fn test_app_controller_initialization() {
        let mut app = AppController::new().expect("Failed to create AppController");
        
        // Test initialization
        let result = app.initialize();
        assert!(result.is_ok(), "Initialization should succeed");
        
        // Verify initial state
        let status = app.get_current_status();
        assert_eq!(status.state, models::PlaybackState::Stopped);
        assert_eq!(status.position, Duration::from_secs(0));
        assert!(status.current_track.is_none());
    }

    #[tokio::test]
    async fn test_volume_command() {
        let mut app = AppController::new().expect("Failed to create AppController");
        app.initialize().expect("Failed to initialize");
        
        // Test volume command
        let command = Commands::Volume { level: 75 };
        let result = app.execute_command(command).await;
        assert!(result.is_ok(), "Volume command should succeed");
        
        // Verify volume was set
        let status = app.get_current_status();
        assert_eq!(status.volume, 0.75);
    }

    #[tokio::test]
    async fn test_queue_operations() {
        let mut app = AppController::new().expect("Failed to create AppController");
        app.initialize().expect("Failed to initialize");
        
        let temp_dir = create_test_directory_structure();
        let test_file = create_test_audio_file(temp_dir.path(), "test", "flac");
        
        // Test adding file to queue
        let command = Commands::Queue {
            action: QueueAction::Add { path: test_file.clone() }
        };
        let result = app.execute_command(command).await;
        assert!(result.is_ok(), "Queue add command should succeed");
        
        // Verify queue has the file
        assert_eq!(app.queue_manager.len(), 1);
        assert!(!app.queue_manager.is_empty());
        
        // Test queue list command
        let command = Commands::Queue {
            action: QueueAction::List
        };
        let result = app.execute_command(command).await;
        assert!(result.is_ok(), "Queue list command should succeed");
        
        // Test queue clear command
        let command = Commands::Queue {
            action: QueueAction::Clear
        };
        let result = app.execute_command(command).await;
        assert!(result.is_ok(), "Queue clear command should succeed");
        
        // Verify queue is empty
        assert_eq!(app.queue_manager.len(), 0);
        assert!(app.queue_manager.is_empty());
    }

    #[tokio::test]
    async fn test_queue_directory_operations() {
        let mut app = AppController::new().expect("Failed to create AppController");
        app.initialize().expect("Failed to initialize");
        
        let temp_dir = create_test_directory_structure();
        
        // Test adding directory to queue
        let command = Commands::Queue {
            action: QueueAction::Add { path: temp_dir.path().to_path_buf() }
        };
        let result = app.execute_command(command).await;
        assert!(result.is_ok(), "Queue add directory command should succeed");
        
        // Verify queue has multiple files (should find 5 audio files)
        assert_eq!(app.queue_manager.len(), 5);
        
        // Test queue position command
        let command = Commands::Queue {
            action: QueueAction::Position
        };
        let result = app.execute_command(command).await;
        assert!(result.is_ok(), "Queue position command should succeed");
    }

    #[tokio::test]
    async fn test_playlist_operations() {
        let mut app = AppController::new().expect("Failed to create AppController");
        app.initialize().expect("Failed to initialize");
        
        let temp_dir = create_test_directory_structure();
        let test_file1 = create_test_audio_file(temp_dir.path(), "test1", "flac");
        let test_file2 = create_test_audio_file(temp_dir.path(), "test2", "mp3");
        
        // Add files to queue
        let command = Commands::Queue {
            action: QueueAction::Add { path: test_file1 }
        };
        app.execute_command(command).await.expect("Failed to add file 1");
        
        let command = Commands::Queue {
            action: QueueAction::Add { path: test_file2 }
        };
        app.execute_command(command).await.expect("Failed to add file 2");
        
        // Test saving playlist
        let command = Commands::Playlist {
            action: PlaylistAction::Save { name: "test_playlist".to_string() }
        };
        let result = app.execute_command(command).await;
        assert!(result.is_ok(), "Playlist save command should succeed");
        
        // Test listing playlists
        let command = Commands::Playlist {
            action: PlaylistAction::List
        };
        let result = app.execute_command(command).await;
        assert!(result.is_ok(), "Playlist list command should succeed");
        
        // Clear queue and load playlist
        app.queue_manager.clear();
        assert!(app.queue_manager.is_empty());
        
        let command = Commands::Playlist {
            action: PlaylistAction::Load { name: "test_playlist".to_string() }
        };
        let result = app.execute_command(command).await;
        assert!(result.is_ok(), "Playlist load command should succeed");
        
        // Verify queue has the loaded tracks
        assert_eq!(app.queue_manager.len(), 2);
        
        // Test deleting playlist
        let command = Commands::Playlist {
            action: PlaylistAction::Delete { name: "test_playlist".to_string() }
        };
        let result = app.execute_command(command).await;
        assert!(result.is_ok(), "Playlist delete command should succeed");
    }

    #[tokio::test]
    async fn test_device_operations() {
        let mut app = AppController::new().expect("Failed to create AppController");
        app.initialize().expect("Failed to initialize");
        
        // Test listing devices
        let command = Commands::Device {
            action: DeviceAction::List
        };
        let result = app.execute_command(command).await;
        assert!(result.is_ok(), "Device list command should succeed");
        
        // Get available devices
        let devices = app.audio_engine.device_manager().list_devices();
        
        if !devices.is_empty() {
            // Test setting device to first available device
            let first_device = devices[0].clone();
            let command = Commands::Device {
                action: DeviceAction::Set { device: first_device.clone() }
            };
            let result = app.execute_command(command).await;
            assert!(result.is_ok(), "Device set command should succeed");
            
            // Verify device was set
            let current_device = app.audio_engine.device_manager().current_device_name()
                .unwrap_or(None);
            assert_eq!(current_device, Some(first_device));
        }
    }

    #[tokio::test]
    async fn test_playback_control_commands() {
        let mut app = AppController::new().expect("Failed to create AppController");
        app.initialize().expect("Failed to initialize");
        
        let temp_dir = create_test_directory_structure();
        let test_file = create_test_audio_file(temp_dir.path(), "test", "flac");
        
        // Add file to queue first
        let command = Commands::Queue {
            action: QueueAction::Add { path: test_file }
        };
        app.execute_command(command).await.expect("Failed to add file to queue");
        
        // Test play command
        let command = Commands::Play { path: None };
        let result = app.execute_command(command).await;
        assert!(result.is_ok(), "Play command should succeed");
        
        // Test pause command
        let command = Commands::Pause;
        let result = app.execute_command(command).await;
        assert!(result.is_ok(), "Pause command should succeed");
        
        // Test resume command
        let command = Commands::Resume;
        let result = app.execute_command(command).await;
        assert!(result.is_ok(), "Resume command should succeed");
        
        // Test stop command
        let command = Commands::Stop;
        let result = app.execute_command(command).await;
        assert!(result.is_ok(), "Stop command should succeed");
    }

    #[tokio::test]
    async fn test_navigation_commands() {
        let mut app = AppController::new().expect("Failed to create AppController");
        app.initialize().expect("Failed to initialize");
        
        let temp_dir = create_test_directory_structure();
        
        // Add multiple files to queue
        let test_file1 = create_test_audio_file(temp_dir.path(), "test1", "flac");
        let test_file2 = create_test_audio_file(temp_dir.path(), "test2", "mp3");
        let test_file3 = create_test_audio_file(temp_dir.path(), "test3", "wav");
        
        for file in [test_file1, test_file2, test_file3] {
            let command = Commands::Queue {
                action: QueueAction::Add { path: file }
            };
            app.execute_command(command).await.expect("Failed to add file to queue");
        }
        
        // Test next command
        let command = Commands::Next;
        let result = app.execute_command(command).await;
        assert!(result.is_ok(), "Next command should succeed");
        
        // Verify we moved to next track
        assert_eq!(app.queue_manager.current_index(), 1);
        
        // Test previous command
        let command = Commands::Prev;
        let result = app.execute_command(command).await;
        assert!(result.is_ok(), "Previous command should succeed");
        
        // Verify we moved back
        assert_eq!(app.queue_manager.current_index(), 0);
    }

    #[tokio::test]
    async fn test_seek_command() {
        let mut app = AppController::new().expect("Failed to create AppController");
        app.initialize().expect("Failed to initialize");
        
        // Test seek command with different time formats
        let test_cases = vec![
            "30",      // 30 seconds
            "1:30",    // 1 minute 30 seconds
            "90s",     // 90 seconds with suffix
        ];
        
        for time_str in test_cases {
            let command = Commands::Seek { position: time_str.to_string() };
            let result = app.execute_command(command).await;
            assert!(result.is_ok(), "Seek command with '{}' should succeed", time_str);
        }
        
        // Test invalid seek format
        let command = Commands::Seek { position: "invalid".to_string() };
        let result = app.execute_command(command).await;
        assert!(result.is_err(), "Seek command with invalid format should fail");
    }

    #[tokio::test]
    async fn test_status_command() {
        let mut app = AppController::new().expect("Failed to create AppController");
        app.initialize().expect("Failed to initialize");
        
        // Test status command
        let command = Commands::Status;
        let result = app.execute_command(command).await;
        assert!(result.is_ok(), "Status command should succeed");
        
        // Verify status is accessible
        let status = app.get_current_status();
        assert_eq!(status.state, models::PlaybackState::Stopped);
    }

    #[tokio::test]
    async fn test_play_with_file_path() {
        let mut app = AppController::new().expect("Failed to create AppController");
        app.initialize().expect("Failed to initialize");
        
        let temp_dir = create_test_directory_structure();
        let test_file = create_test_audio_file(temp_dir.path(), "test", "flac");
        
        // Test play command with file path
        let command = Commands::Play { path: Some(test_file) };
        let result = app.execute_command(command).await;
        assert!(result.is_ok(), "Play command with file path should succeed");
        
        // Verify file was added to queue
        assert_eq!(app.queue_manager.len(), 1);
    }

    #[tokio::test]
    async fn test_play_with_directory_path() {
        let mut app = AppController::new().expect("Failed to create AppController");
        app.initialize().expect("Failed to initialize");
        
        let temp_dir = create_test_directory_structure();
        
        // Test play command with directory path
        let command = Commands::Play { path: Some(temp_dir.path().to_path_buf()) };
        let result = app.execute_command(command).await;
        assert!(result.is_ok(), "Play command with directory path should succeed");
        
        // Verify files were added to queue (should find 5 audio files)
        assert_eq!(app.queue_manager.len(), 5);
    }

    #[tokio::test]
    async fn test_configuration_persistence() {
        let mut app = AppController::new().expect("Failed to create AppController");
        app.initialize().expect("Failed to initialize");
        
        // Change volume
        let command = Commands::Volume { level: 50 };
        app.execute_command(command).await.expect("Failed to set volume");
        
        // Save configuration
        let result = app.save_current_config();
        assert!(result.is_ok(), "Configuration save should succeed");
        
        // Verify configuration was saved by checking the config manager
        let config = app.config_manager.get_config();
        assert_eq!(config.default_volume, 0.5);
    }

    #[tokio::test]
    async fn test_graceful_shutdown() {
        let mut app = AppController::new().expect("Failed to create AppController");
        app.initialize().expect("Failed to initialize");
        
        // Test graceful shutdown
        let result = app.shutdown().await;
        assert!(result.is_ok(), "Graceful shutdown should succeed");
    }

    #[tokio::test]
    async fn test_error_handling_empty_queue() {
        let mut app = AppController::new().expect("Failed to create AppController");
        app.initialize().expect("Failed to initialize");
        
        // Test play command with empty queue
        let command = Commands::Play { path: None };
        let result = app.execute_command(command).await;
        assert!(result.is_err(), "Play command with empty queue should fail");
        
        // Verify it's the correct error type
        match result.unwrap_err() {
            PlayerError::Queue(error::QueueError::EmptyQueue) => {
                // Expected error
            }
            _ => panic!("Expected EmptyQueue error"),
        }
    }

    #[tokio::test]
    async fn test_error_handling_invalid_file() {
        let mut app = AppController::new().expect("Failed to create AppController");
        app.initialize().expect("Failed to initialize");
        
        let nonexistent_file = PathBuf::from("/nonexistent/file.flac");
        
        // Test adding nonexistent file to queue
        let command = Commands::Queue {
            action: QueueAction::Add { path: nonexistent_file }
        };
        let result = app.execute_command(command).await;
        assert!(result.is_err(), "Adding nonexistent file should fail");
    }

    #[tokio::test]
    async fn test_error_handling_invalid_device() {
        let mut app = AppController::new().expect("Failed to create AppController");
        app.initialize().expect("Failed to initialize");
        
        // Test setting invalid device
        let command = Commands::Device {
            action: DeviceAction::Set { device: "NonExistentDevice".to_string() }
        };
        let result = app.execute_command(command).await;
        assert!(result.is_err(), "Setting invalid device should fail");
    }

    #[tokio::test]
    async fn test_complete_workflow() {
        let mut app = AppController::new().expect("Failed to create AppController");
        app.initialize().expect("Failed to initialize");
        
        let temp_dir = create_test_directory_structure();
        
        // Step 1: Add directory to queue
        let command = Commands::Queue {
            action: QueueAction::Add { path: temp_dir.path().to_path_buf() }
        };
        app.execute_command(command).await.expect("Failed to add directory");
        
        // Step 2: Set volume
        let command = Commands::Volume { level: 80 };
        app.execute_command(command).await.expect("Failed to set volume");
        
        // Step 3: Start playback
        let command = Commands::Play { path: None };
        app.execute_command(command).await.expect("Failed to start playback");
        
        // Step 4: Navigate tracks
        let command = Commands::Next;
        app.execute_command(command).await.expect("Failed to go to next track");
        
        let command = Commands::Prev;
        app.execute_command(command).await.expect("Failed to go to previous track");
        
        // Step 5: Save playlist
        let command = Commands::Playlist {
            action: PlaylistAction::Save { name: "workflow_test".to_string() }
        };
        app.execute_command(command).await.expect("Failed to save playlist");
        
        // Step 6: Check status
        let command = Commands::Status;
        app.execute_command(command).await.expect("Failed to get status");
        
        // Step 7: Pause and resume
        let command = Commands::Pause;
        app.execute_command(command).await.expect("Failed to pause");
        
        let command = Commands::Resume;
        app.execute_command(command).await.expect("Failed to resume");
        
        // Step 8: Stop playback
        let command = Commands::Stop;
        app.execute_command(command).await.expect("Failed to stop");
        
        // Step 9: Clean shutdown
        app.shutdown().await.expect("Failed to shutdown");
        
        // Verify final state
        let status = app.get_current_status();
        assert_eq!(status.volume, 0.8);
        assert_eq!(app.queue_manager.len(), 5);
    }

    #[tokio::test]
    async fn test_concurrent_operations() {
        let mut app = AppController::new().expect("Failed to create AppController");
        app.initialize().expect("Failed to initialize");
        
        let temp_dir = create_test_directory_structure();
        let test_file = create_test_audio_file(temp_dir.path(), "test", "flac");
        
        // Add file to queue
        let command = Commands::Queue {
            action: QueueAction::Add { path: test_file }
        };
        app.execute_command(command).await.expect("Failed to add file");
        
        // Perform multiple operations in sequence (simulating rapid user input)
        let commands = vec![
            Commands::Volume { level: 75 },
            Commands::Play { path: None },
            Commands::Volume { level: 50 },
            Commands::Pause,
            Commands::Volume { level: 25 },
            Commands::Resume,
            Commands::Stop,
        ];
        
        for command in commands {
            let result = app.execute_command(command).await;
            assert!(result.is_ok(), "Concurrent operation should succeed");
        }
        
        // Verify final volume setting
        let status = app.get_current_status();
        assert_eq!(status.volume, 0.25);
    }

    #[tokio::test]
    async fn test_queue_navigation_edge_cases() {
        let mut app = AppController::new().expect("Failed to create AppController");
        app.initialize().expect("Failed to initialize");
        
        // Test navigation with empty queue
        let command = Commands::Next;
        let result = app.execute_command(command).await;
        assert!(result.is_ok(), "Next command with empty queue should not crash");
        
        let command = Commands::Prev;
        let result = app.execute_command(command).await;
        assert!(result.is_ok(), "Previous command with empty queue should not crash");
        
        // Add single file and test navigation
        let temp_dir = create_test_directory_structure();
        let test_file = create_test_audio_file(temp_dir.path(), "single", "flac");
        
        let command = Commands::Queue {
            action: QueueAction::Add { path: test_file }
        };
        app.execute_command(command).await.expect("Failed to add file");
        
        // Test navigation with single track (should wrap around)
        let initial_index = app.queue_manager.current_index();
        
        let command = Commands::Next;
        app.execute_command(command).await.expect("Failed to go to next");
        
        let command = Commands::Prev;
        app.execute_command(command).await.expect("Failed to go to previous");
        
        // Should be back at the same position
        assert_eq!(app.queue_manager.current_index(), initial_index);
    }

    #[tokio::test]
    async fn test_volume_edge_cases() {
        let mut app = AppController::new().expect("Failed to create AppController");
        app.initialize().expect("Failed to initialize");
        
        // Test minimum volume
        let command = Commands::Volume { level: 0 };
        let result = app.execute_command(command).await;
        assert!(result.is_ok(), "Setting volume to 0 should succeed");
        
        let status = app.get_current_status();
        assert_eq!(status.volume, 0.0);
        
        // Test maximum volume
        let command = Commands::Volume { level: 100 };
        let result = app.execute_command(command).await;
        assert!(result.is_ok(), "Setting volume to 100 should succeed");
        
        let status = app.get_current_status();
        assert_eq!(status.volume, 1.0);
    }
}