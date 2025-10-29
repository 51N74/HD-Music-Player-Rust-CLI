#[cfg(test)]
mod tests {
    use crate::cli::{CliApp, Commands, QueueAction, PlaylistAction, DeviceAction, ParseError};
    use crate::models::{AudioFormat, AudioCodec, AudioMetadata, TrackInfo, PlayerStatus};
    use crate::error::PlayerError;
    use std::path::PathBuf;
    use std::time::Duration;

    #[test]
    fn test_parse_command_play() {
        // Test play without path
        let result = CliApp::parse_command("play");
        assert!(result.is_ok());
        match result.unwrap() {
            Commands::Play { path } => assert!(path.is_none()),
            _ => panic!("Expected Play command"),
        }

        // Test play with path
        let result = CliApp::parse_command("play /path/to/song.flac");
        assert!(result.is_ok());
        match result.unwrap() {
            Commands::Play { path } => {
                assert_eq!(path, Some(PathBuf::from("/path/to/song.flac")));
            }
            _ => panic!("Expected Play command"),
        }

        // Test play with path containing spaces
        let result = CliApp::parse_command("play /path/to/my song.flac");
        assert!(result.is_ok());
        match result.unwrap() {
            Commands::Play { path } => {
                assert_eq!(path, Some(PathBuf::from("/path/to/my song.flac")));
            }
            _ => panic!("Expected Play command"),
        }
    }

    #[test]
    fn test_parse_command_basic_controls() {
        // Test pause
        let result = CliApp::parse_command("pause");
        assert!(result.is_ok());
        assert!(matches!(result.unwrap(), Commands::Pause));

        // Test resume
        let result = CliApp::parse_command("resume");
        assert!(result.is_ok());
        assert!(matches!(result.unwrap(), Commands::Resume));

        // Test stop
        let result = CliApp::parse_command("stop");
        assert!(result.is_ok());
        assert!(matches!(result.unwrap(), Commands::Stop));

        // Test next
        let result = CliApp::parse_command("next");
        assert!(result.is_ok());
        assert!(matches!(result.unwrap(), Commands::Next));

        // Test prev
        let result = CliApp::parse_command("prev");
        assert!(result.is_ok());
        assert!(matches!(result.unwrap(), Commands::Prev));

        // Test previous (alias)
        let result = CliApp::parse_command("previous");
        assert!(result.is_ok());
        assert!(matches!(result.unwrap(), Commands::Prev));

        // Test status
        let result = CliApp::parse_command("status");
        assert!(result.is_ok());
        assert!(matches!(result.unwrap(), Commands::Status));
    }

    #[test]
    fn test_parse_command_seek() {
        // Test seek with position
        let result = CliApp::parse_command("seek 1:30");
        assert!(result.is_ok());
        match result.unwrap() {
            Commands::Seek { position } => assert_eq!(position, "1:30"),
            _ => panic!("Expected Seek command"),
        }

        // Test seek without position (should fail)
        let result = CliApp::parse_command("seek");
        assert!(result.is_err());
        match result.unwrap_err() {
            ParseError::MissingArgument { command, argument } => {
                assert_eq!(command, "seek");
                assert_eq!(argument, "position");
            }
            _ => panic!("Expected MissingArgument error"),
        }
    }

    #[test]
    fn test_parse_command_volume() {
        // Test valid volume
        let result = CliApp::parse_command("volume 50");
        assert!(result.is_ok());
        match result.unwrap() {
            Commands::Volume { level } => assert_eq!(level, 50),
            _ => panic!("Expected Volume command"),
        }

        // Test volume at boundaries
        let result = CliApp::parse_command("volume 0");
        assert!(result.is_ok());
        match result.unwrap() {
            Commands::Volume { level } => assert_eq!(level, 0),
            _ => panic!("Expected Volume command"),
        }

        let result = CliApp::parse_command("volume 100");
        assert!(result.is_ok());
        match result.unwrap() {
            Commands::Volume { level } => assert_eq!(level, 100),
            _ => panic!("Expected Volume command"),
        }

        // Test invalid volume (too high)
        let result = CliApp::parse_command("volume 101");
        assert!(result.is_err());
        match result.unwrap_err() {
            ParseError::InvalidArgument { argument, value, expected } => {
                assert_eq!(argument, "volume level");
                assert_eq!(value, "101");
                assert_eq!(expected, "0-100");
            }
            _ => panic!("Expected InvalidArgument error"),
        }

        // Test invalid volume (not a number)
        let result = CliApp::parse_command("volume abc");
        assert!(result.is_err());
        match result.unwrap_err() {
            ParseError::InvalidArgument { argument, value, expected } => {
                assert_eq!(argument, "volume level");
                assert_eq!(value, "abc");
                assert_eq!(expected, "number 0-100");
            }
            _ => panic!("Expected InvalidArgument error"),
        }

        // Test volume without argument
        let result = CliApp::parse_command("volume");
        assert!(result.is_err());
        match result.unwrap_err() {
            ParseError::MissingArgument { command, argument } => {
                assert_eq!(command, "volume");
                assert_eq!(argument, "level");
            }
            _ => panic!("Expected MissingArgument error"),
        }
    }

    #[test]
    fn test_parse_command_queue() {
        // Test queue add
        let result = CliApp::parse_command("queue add /path/to/music");
        assert!(result.is_ok());
        match result.unwrap() {
            Commands::Queue { action: QueueAction::Add { path } } => {
                assert_eq!(path, PathBuf::from("/path/to/music"));
            }
            _ => panic!("Expected Queue Add command"),
        }

        // Test queue list
        let result = CliApp::parse_command("queue list");
        assert!(result.is_ok());
        match result.unwrap() {
            Commands::Queue { action: QueueAction::List } => {}
            _ => panic!("Expected Queue List command"),
        }

        // Test queue clear
        let result = CliApp::parse_command("queue clear");
        assert!(result.is_ok());
        match result.unwrap() {
            Commands::Queue { action: QueueAction::Clear } => {}
            _ => panic!("Expected Queue Clear command"),
        }

        // Test queue position
        let result = CliApp::parse_command("queue position");
        assert!(result.is_ok());
        match result.unwrap() {
            Commands::Queue { action: QueueAction::Position } => {}
            _ => panic!("Expected Queue Position command"),
        }

        // Test queue without action
        let result = CliApp::parse_command("queue");
        assert!(result.is_err());
        match result.unwrap_err() {
            ParseError::MissingArgument { command, argument } => {
                assert_eq!(command, "queue");
                assert_eq!(argument, "action");
            }
            _ => panic!("Expected MissingArgument error"),
        }

        // Test queue add without path
        let result = CliApp::parse_command("queue add");
        assert!(result.is_err());
        match result.unwrap_err() {
            ParseError::MissingArgument { command, argument } => {
                assert_eq!(command, "queue add");
                assert_eq!(argument, "path");
            }
            _ => panic!("Expected MissingArgument error"),
        }

        // Test unknown queue action
        let result = CliApp::parse_command("queue unknown");
        assert!(result.is_err());
        match result.unwrap_err() {
            ParseError::UnknownCommand { command } => {
                assert_eq!(command, "queue unknown");
            }
            _ => panic!("Expected UnknownCommand error"),
        }
    }

    #[test]
    fn test_parse_command_playlist() {
        // Test playlist save
        let result = CliApp::parse_command("playlist save my_playlist");
        assert!(result.is_ok());
        match result.unwrap() {
            Commands::Playlist { action: PlaylistAction::Save { name } } => {
                assert_eq!(name, "my_playlist");
            }
            _ => panic!("Expected Playlist Save command"),
        }

        // Test playlist load
        let result = CliApp::parse_command("playlist load my_playlist");
        assert!(result.is_ok());
        match result.unwrap() {
            Commands::Playlist { action: PlaylistAction::Load { name } } => {
                assert_eq!(name, "my_playlist");
            }
            _ => panic!("Expected Playlist Load command"),
        }

        // Test playlist list
        let result = CliApp::parse_command("playlist list");
        assert!(result.is_ok());
        match result.unwrap() {
            Commands::Playlist { action: PlaylistAction::List } => {}
            _ => panic!("Expected Playlist List command"),
        }

        // Test playlist delete
        let result = CliApp::parse_command("playlist delete my_playlist");
        assert!(result.is_ok());
        match result.unwrap() {
            Commands::Playlist { action: PlaylistAction::Delete { name } } => {
                assert_eq!(name, "my_playlist");
            }
            _ => panic!("Expected Playlist Delete command"),
        }

        // Test playlist with name containing spaces
        let result = CliApp::parse_command("playlist save my favorite songs");
        assert!(result.is_ok());
        match result.unwrap() {
            Commands::Playlist { action: PlaylistAction::Save { name } } => {
                assert_eq!(name, "my favorite songs");
            }
            _ => panic!("Expected Playlist Save command"),
        }
    }

    #[test]
    fn test_parse_command_device() {
        // Test device list
        let result = CliApp::parse_command("device list");
        assert!(result.is_ok());
        match result.unwrap() {
            Commands::Device { action: DeviceAction::List } => {}
            _ => panic!("Expected Device List command"),
        }

        // Test device set
        let result = CliApp::parse_command("device set Built-in Output");
        assert!(result.is_ok());
        match result.unwrap() {
            Commands::Device { action: DeviceAction::Set { device } } => {
                assert_eq!(device, "Built-in Output");
            }
            _ => panic!("Expected Device Set command"),
        }
    }

    #[test]
    fn test_parse_command_errors() {
        // Test empty command
        let result = CliApp::parse_command("");
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ParseError::EmptyCommand));

        // Test whitespace only
        let result = CliApp::parse_command("   ");
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ParseError::EmptyCommand));

        // Test unknown command
        let result = CliApp::parse_command("unknown_command");
        assert!(result.is_err());
        match result.unwrap_err() {
            ParseError::UnknownCommand { command } => {
                assert_eq!(command, "unknown_command");
            }
            _ => panic!("Expected UnknownCommand error"),
        }

        // Test help request
        let result = CliApp::parse_command("help");
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ParseError::HelpRequested));
    }

    #[test]
    fn test_parse_time() {
        // Test MM:SS format
        let result = CliApp::parse_time("1:30");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Duration::from_secs(90));

        let result = CliApp::parse_time("0:05");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Duration::from_secs(5));

        let result = CliApp::parse_time("10:00");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Duration::from_secs(600));

        // Test seconds format
        let result = CliApp::parse_time("90");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Duration::from_secs(90));

        let result = CliApp::parse_time("5");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Duration::from_secs(5));

        // Test seconds with 's' suffix
        let result = CliApp::parse_time("90s");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Duration::from_secs(90));

        // Test decimal seconds
        let result = CliApp::parse_time("90.5");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Duration::from_millis(90500));

        let result = CliApp::parse_time("1:30.25");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Duration::from_millis(90250));

        // Test decimal with 's' suffix
        let result = CliApp::parse_time("5.5s");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Duration::from_millis(5500));

        // Test invalid formats
        let result = CliApp::parse_time("1:60"); // Invalid seconds
        assert!(result.is_err());

        let result = CliApp::parse_time("1:2:3"); // Too many parts
        assert!(result.is_err());

        let result = CliApp::parse_time("abc");
        assert!(result.is_err());

        let result = CliApp::parse_time("1:abc");
        assert!(result.is_err());

        // Test negative values
        let result = CliApp::parse_time("-30");
        assert!(result.is_err());

        let result = CliApp::parse_time("1:-30");
        assert!(result.is_err());

        // Test empty and whitespace
        let result = CliApp::parse_time("");
        assert!(result.is_err());

        let result = CliApp::parse_time("   ");
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_seek_time() {
        let track_duration = Duration::from_secs(180); // 3 minutes

        // Test valid seek positions
        let result = CliApp::validate_seek_time(Duration::from_secs(60), Some(track_duration));
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Duration::from_secs(60));

        let result = CliApp::validate_seek_time(Duration::from_secs(0), Some(track_duration));
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Duration::from_secs(0));

        let result = CliApp::validate_seek_time(Duration::from_secs(180), Some(track_duration));
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Duration::from_secs(180));

        // Test seek beyond duration
        let result = CliApp::validate_seek_time(Duration::from_secs(200), Some(track_duration));
        assert!(result.is_err());
        match result.unwrap_err() {
            ParseError::SeekBeyondDuration { position, duration } => {
                assert_eq!(position, 200.0);
                assert_eq!(duration, 180.0);
            }
            _ => panic!("Expected SeekBeyondDuration error"),
        }

        // Test with no duration (should always pass)
        let result = CliApp::validate_seek_time(Duration::from_secs(1000), None);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Duration::from_secs(1000));
    }

    #[test]
    fn test_seek_command_parsing_comprehensive() {
        // Test various seek formats
        let test_cases = vec![
            ("seek 30", "30"),
            ("seek 1:30", "1:30"),
            ("seek 90s", "90s"),
            ("seek 1:30.5", "1:30.5"),
            ("seek 0:00", "0:00"),
        ];

        for (input, expected_position) in test_cases {
            let result = CliApp::parse_command(input);
            assert!(result.is_ok(), "Failed to parse: {}", input);
            match result.unwrap() {
                Commands::Seek { position } => {
                    assert_eq!(position, expected_position);
                }
                _ => panic!("Expected Seek command for: {}", input),
            }
        }
    }

    #[test]
    fn test_parse_time_precision() {
        // Test high precision parsing
        let result = CliApp::parse_time("1.001");
        assert!(result.is_ok());
        let duration = result.unwrap();
        assert_eq!(duration.as_millis(), 1001);

        let result = CliApp::parse_time("0:01.500");
        assert!(result.is_ok());
        let duration = result.unwrap();
        assert_eq!(duration.as_millis(), 1500);

        // Test very small values
        let result = CliApp::parse_time("0.1");
        assert!(result.is_ok());
        let duration = result.unwrap();
        assert_eq!(duration.as_millis(), 100);
    }

    #[test]
    fn test_parse_time_edge_cases() {
        // Test boundary values
        let result = CliApp::parse_time("0:59.999");
        assert!(result.is_ok());
        let duration = result.unwrap();
        assert_eq!(duration.as_millis(), 59999);

        // Test exactly 60 seconds (should be invalid in MM:SS format)
        let result = CliApp::parse_time("0:60");
        assert!(result.is_err());

        // Test large values
        let result = CliApp::parse_time("999:59");
        assert!(result.is_ok());
        let duration = result.unwrap();
        assert_eq!(duration.as_secs(), 999 * 60 + 59);

        // Test zero values
        let result = CliApp::parse_time("0");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Duration::from_secs(0));

        let result = CliApp::parse_time("0:00");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Duration::from_secs(0));

        let result = CliApp::parse_time("0.0");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Duration::from_secs(0));
    }

    #[test]
    fn test_format_duration() {
        assert_eq!(CliApp::format_duration(Duration::from_secs(0)), "00:00");
        assert_eq!(CliApp::format_duration(Duration::from_secs(5)), "00:05");
        assert_eq!(CliApp::format_duration(Duration::from_secs(60)), "01:00");
        assert_eq!(CliApp::format_duration(Duration::from_secs(90)), "01:30");
        assert_eq!(CliApp::format_duration(Duration::from_secs(3661)), "61:01");
    }

    #[test]
    fn test_truncate() {
        assert_eq!(CliApp::truncate("short", 10), "short");
        assert_eq!(CliApp::truncate("exactly_ten", 10), "exactly...");
        assert_eq!(CliApp::truncate("this is a very long string", 10), "this is...");
        assert_eq!(CliApp::truncate("abc", 3), "abc");
        assert_eq!(CliApp::truncate("abcd", 3), "...");
    }

    #[test]
    fn test_display_status_empty() {
        let app = CliApp::new().unwrap();
        let status = PlayerStatus::new();
        
        // This test just ensures display_status doesn't panic with empty status
        app.display_status(&status);
    }

    #[test]
    fn test_display_status_with_track() {
        let app = CliApp::new().unwrap();
        
        let metadata = AudioMetadata::with_title_artist(
            "Test Song".to_string(),
            "Test Artist".to_string(),
        );
        let mut metadata = metadata;
        metadata.album = Some("Test Album".to_string());
        
        let track = TrackInfo::new(
            PathBuf::from("/test/song.flac"),
            metadata,
            Duration::from_secs(180),
            1024 * 1024,
        );
        
        let mut status = PlayerStatus::playing(track, Duration::from_secs(60), 0.8);
        status.audio_format = Some(AudioFormat::new(44100, 16, 2, AudioCodec::Flac));
        status.output_device = Some("Built-in Output".to_string());
        
        // This test just ensures display_status doesn't panic with full status
        app.display_status(&status);
    }

    #[test]
    fn test_display_error() {
        let app = CliApp::new().unwrap();
        let error = PlayerError::Parse(ParseError::EmptyCommand);
        
        // This test just ensures display_error doesn't panic
        app.display_error(&error);
    }

    #[test]
    fn test_parse_error_display() {
        let error = ParseError::EmptyCommand;
        assert_eq!(format!("{}", error), "Empty command");

        let error = ParseError::UnknownCommand {
            command: "test".to_string(),
        };
        assert_eq!(format!("{}", error), "Unknown command: test");

        let error = ParseError::MissingArgument {
            command: "volume".to_string(),
            argument: "level".to_string(),
        };
        assert_eq!(format!("{}", error), "Missing argument for volume: level");

        let error = ParseError::InvalidArgument {
            argument: "volume".to_string(),
            value: "abc".to_string(),
            expected: "0-100".to_string(),
        };
        assert_eq!(format!("{}", error), "Invalid argument volume: got 'abc', expected 0-100");

        let error = ParseError::InvalidTimeFormat {
            input: "1:60".to_string(),
        };
        assert_eq!(format!("{}", error), "Invalid time format: 1:60");

        let error = ParseError::HelpRequested;
        assert_eq!(format!("{}", error), "Help requested");
    }

    #[test]
    fn test_cli_app_creation() {
        let app = CliApp::new();
        assert!(app.is_ok());
    }

    #[test]
    fn test_command_validation_edge_cases() {
        // Test commands with extra whitespace
        let result = CliApp::parse_command("  play  /path/to/song.flac  ");
        assert!(result.is_ok());

        // Test case sensitivity
        let result = CliApp::parse_command("PLAY");
        assert!(result.is_err()); // Commands are case-sensitive

        // Test partial commands
        let result = CliApp::parse_command("pla");
        assert!(result.is_err());

        // Test commands with special characters in paths
        let result = CliApp::parse_command("play /path/with spaces/song (1).flac");
        assert!(result.is_ok());
        match result.unwrap() {
            Commands::Play { path } => {
                assert_eq!(path, Some(PathBuf::from("/path/with spaces/song (1).flac")));
            }
            _ => panic!("Expected Play command"),
        }
    }

    #[test]
    fn test_queue_commands_comprehensive() {
        // Test all queue subcommands
        let commands = vec![
            ("queue add /music", QueueAction::Add { path: PathBuf::from("/music") }),
            ("queue list", QueueAction::List),
            ("queue clear", QueueAction::Clear),
            ("queue position", QueueAction::Position),
        ];

        for (input, expected_action) in commands {
            let result = CliApp::parse_command(input);
            assert!(result.is_ok(), "Failed to parse: {}", input);
            
            match result.unwrap() {
                Commands::Queue { action } => {
                    match (&action, &expected_action) {
                        (QueueAction::Add { path: p1 }, QueueAction::Add { path: p2 }) => {
                            assert_eq!(p1, p2);
                        }
                        (QueueAction::List, QueueAction::List) => {}
                        (QueueAction::Clear, QueueAction::Clear) => {}
                        (QueueAction::Position, QueueAction::Position) => {}
                        _ => panic!("Action mismatch for: {}", input),
                    }
                }
                _ => panic!("Expected Queue command for: {}", input),
            }
        }
    }

    #[test]
    fn test_playlist_commands_comprehensive() {
        // Test all playlist subcommands
        let result = CliApp::parse_command("playlist save test");
        assert!(result.is_ok());
        
        let result = CliApp::parse_command("playlist load test");
        assert!(result.is_ok());
        
        let result = CliApp::parse_command("playlist list");
        assert!(result.is_ok());
        
        let result = CliApp::parse_command("playlist delete test");
        assert!(result.is_ok());

        // Test error cases
        let result = CliApp::parse_command("playlist");
        assert!(result.is_err());
        
        let result = CliApp::parse_command("playlist save");
        assert!(result.is_err());
        
        let result = CliApp::parse_command("playlist invalid");
        assert!(result.is_err());
    }

    #[test]
    fn test_device_commands_comprehensive() {
        // Test all device subcommands
        let result = CliApp::parse_command("device list");
        assert!(result.is_ok());
        
        let result = CliApp::parse_command("device set Built-in Output");
        assert!(result.is_ok());

        // Test error cases
        let result = CliApp::parse_command("device");
        assert!(result.is_err());
        
        let result = CliApp::parse_command("device set");
        assert!(result.is_err());
        
        let result = CliApp::parse_command("device invalid");
        assert!(result.is_err());
    }

    #[test]
    fn test_time_parsing_edge_cases() {
        // Test zero values
        assert_eq!(CliApp::parse_time("0").unwrap(), Duration::from_secs(0));
        assert_eq!(CliApp::parse_time("0:00").unwrap(), Duration::from_secs(0));
        assert_eq!(CliApp::parse_time("0s").unwrap(), Duration::from_secs(0));

        // Test large values
        assert_eq!(CliApp::parse_time("3600").unwrap(), Duration::from_secs(3600)); // 1 hour
        assert_eq!(CliApp::parse_time("60:00").unwrap(), Duration::from_secs(3600)); // 1 hour

        // Test boundary conditions
        assert_eq!(CliApp::parse_time("0:59").unwrap(), Duration::from_secs(59));
        assert!(CliApp::parse_time("0:60").is_err()); // Invalid seconds

        // Test malformed inputs
        assert!(CliApp::parse_time("").is_err());
        assert!(CliApp::parse_time(":30").is_err());
        assert!(CliApp::parse_time("30:").is_err());
        assert!(CliApp::parse_time("1::30").is_err());
        assert!(CliApp::parse_time("-30").is_err());
        assert!(CliApp::parse_time("1:-30").is_err());
    }
}