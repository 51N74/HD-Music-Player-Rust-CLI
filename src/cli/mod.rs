use crate::error::PlayerError;
use crate::models::PlayerStatus;
use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub mod status;
pub use status::StatusDisplay;

/// High-Resolution Audio Player CLI
#[derive(Parser)]
#[command(name = "hires-audio-player")]
#[command(about = "A high-performance CLI audio player for high-resolution audio files")]
#[command(version = "0.1.0")]
pub struct CliApp {
    #[command(subcommand)]
    pub command: Option<Commands>,
}

/// Available CLI commands
#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Start playback of current file or queue
    Play {
        /// Optional file or directory path to play
        path: Option<PathBuf>,
    },
    /// Pause playback while preserving position
    Pause,
    /// Resume playback from paused position
    Resume,
    /// Stop playback and reset position
    Stop,
    /// Advance to next track in queue
    Next,
    /// Go back to previous track in queue
    #[command(alias = "previous")]
    Prev,
    /// Seek to specific time position
    Seek {
        /// Time offset (e.g., "1:30", "90", "90s")
        position: String,
    },
    /// Display current player status and track information
    Status,
    /// Continuously update status every 100ms (live view)
    Watch,
    /// Set playback volume (0-100)
    Volume {
        /// Volume level (0-100)
        level: u8,
    },
    /// Queue management commands
    Queue {
        #[command(subcommand)]
        action: QueueAction,
    },
    /// Playlist management commands
    Playlist {
        #[command(subcommand)]
        action: PlaylistAction,
    },
    /// Audio output device management
    Device {
        #[command(subcommand)]
        action: DeviceAction,
    },
}

/// Queue management subcommands
#[derive(Debug, Subcommand)]
pub enum QueueAction {
    /// Add file or directory to queue
    Add {
        /// Path to file or directory
        path: PathBuf,
    },
    /// List all tracks in current queue
    List,
    /// Clear all tracks from queue
    Clear,
    /// Show current queue position
    Position,
}

/// Playlist management subcommands
#[derive(Debug, Subcommand)]
pub enum PlaylistAction {
    /// Save current queue as playlist
    Save {
        /// Playlist name
        name: String,
    },
    /// Load playlist into current queue
    Load {
        /// Playlist name
        name: String,
    },
    /// List available playlists
    List,
    /// Delete a playlist
    Delete {
        /// Playlist name
        name: String,
    },
}

/// Device management subcommands
#[derive(Debug, Subcommand)]
pub enum DeviceAction {
    /// List available audio output devices
    List,
    /// Set audio output device
    Set {
        /// Device name or ID
        device: String,
    },
}

impl CliApp {
    pub fn new() -> Result<Self, PlayerError> {
        Ok(Self { command: None })
    }

    /// Parse command line arguments
    pub fn parse() -> Self {
        <Self as clap::Parser>::parse()
    }

    /// Expand tilde (~) in path to home directory
    pub fn expand_path(path: &str) -> PathBuf {
        if path.starts_with("~/") {
            if let Some(home_dir) = dirs::home_dir() {
                home_dir.join(&path[2..])
            } else {
                PathBuf::from(path)
            }
        } else if path == "~" {
            dirs::home_dir().unwrap_or_else(|| PathBuf::from(path))
        } else {
            PathBuf::from(path)
        }
    }

    /// Parse command from string (for interactive mode)
    pub fn parse_command(input: &str) -> Result<Commands, ParseError> {
        let args: Vec<&str> = input.trim().split_whitespace().collect();
        if args.is_empty() {
            return Err(ParseError::EmptyCommand);
        }

        match args[0] {
            "play" => {
                if args.len() > 1 {
                    let path_str = args[1..].join(" ");
                    let path = Self::expand_path(&path_str);
                    Ok(Commands::Play { path: Some(path) })
                } else {
                    Ok(Commands::Play { path: None })
                }
            }
            "pause" => Ok(Commands::Pause),
            "resume" => Ok(Commands::Resume),
            "stop" => Ok(Commands::Stop),
            "next" => Ok(Commands::Next),
            "prev" | "previous" => Ok(Commands::Prev),
            "seek" => {
                if args.len() > 1 {
                    Ok(Commands::Seek {
                        position: args[1].to_string(),
                    })
                } else {
                    Err(ParseError::MissingArgument {
                        command: "seek".to_string(),
                        argument: "position".to_string(),
                    })
                }
            }
            "status" => Ok(Commands::Status),
            "watch" => Ok(Commands::Watch),
            "volume" => {
                if args.len() > 1 {
                    match args[1].parse::<u8>() {
                        Ok(level) if level <= 100 => Ok(Commands::Volume { level }),
                        Ok(_) => Err(ParseError::InvalidArgument {
                            argument: "volume level".to_string(),
                            value: args[1].to_string(),
                            expected: "0-100".to_string(),
                        }),
                        Err(_) => Err(ParseError::InvalidArgument {
                            argument: "volume level".to_string(),
                            value: args[1].to_string(),
                            expected: "number 0-100".to_string(),
                        }),
                    }
                } else {
                    Err(ParseError::MissingArgument {
                        command: "volume".to_string(),
                        argument: "level".to_string(),
                    })
                }
            }
            "queue" => {
                if args.len() < 2 {
                    return Err(ParseError::MissingArgument {
                        command: "queue".to_string(),
                        argument: "action".to_string(),
                    });
                }
                match args[1] {
                    "add" => {
                        if args.len() > 2 {
                            let path_str = args[2..].join(" ");
                            let path = Self::expand_path(&path_str);
                            Ok(Commands::Queue {
                                action: QueueAction::Add { path },
                            })
                        } else {
                            Err(ParseError::MissingArgument {
                                command: "queue add".to_string(),
                                argument: "path".to_string(),
                            })
                        }
                    }
                    "list" => Ok(Commands::Queue {
                        action: QueueAction::List,
                    }),
                    "clear" => Ok(Commands::Queue {
                        action: QueueAction::Clear,
                    }),
                    "position" => Ok(Commands::Queue {
                        action: QueueAction::Position,
                    }),
                    _ => Err(ParseError::UnknownCommand {
                        command: format!("queue {}", args[1]),
                    }),
                }
            }
            "playlist" => {
                if args.len() < 2 {
                    return Err(ParseError::MissingArgument {
                        command: "playlist".to_string(),
                        argument: "action".to_string(),
                    });
                }
                match args[1] {
                    "save" => {
                        if args.len() > 2 {
                            Ok(Commands::Playlist {
                                action: PlaylistAction::Save {
                                    name: args[2..].join(" "),
                                },
                            })
                        } else {
                            Err(ParseError::MissingArgument {
                                command: "playlist save".to_string(),
                                argument: "name".to_string(),
                            })
                        }
                    }
                    "load" => {
                        if args.len() > 2 {
                            Ok(Commands::Playlist {
                                action: PlaylistAction::Load {
                                    name: args[2..].join(" "),
                                },
                            })
                        } else {
                            Err(ParseError::MissingArgument {
                                command: "playlist load".to_string(),
                                argument: "name".to_string(),
                            })
                        }
                    }
                    "list" => Ok(Commands::Playlist {
                        action: PlaylistAction::List,
                    }),
                    "delete" => {
                        if args.len() > 2 {
                            Ok(Commands::Playlist {
                                action: PlaylistAction::Delete {
                                    name: args[2..].join(" "),
                                },
                            })
                        } else {
                            Err(ParseError::MissingArgument {
                                command: "playlist delete".to_string(),
                                argument: "name".to_string(),
                            })
                        }
                    }
                    _ => Err(ParseError::UnknownCommand {
                        command: format!("playlist {}", args[1]),
                    }),
                }
            }
            "device" => {
                if args.len() < 2 {
                    return Err(ParseError::MissingArgument {
                        command: "device".to_string(),
                        argument: "action".to_string(),
                    });
                }
                match args[1] {
                    "list" => Ok(Commands::Device {
                        action: DeviceAction::List,
                    }),
                    "set" => {
                        if args.len() > 2 {
                            Ok(Commands::Device {
                                action: DeviceAction::Set {
                                    device: args[2..].join(" "),
                                },
                            })
                        } else {
                            Err(ParseError::MissingArgument {
                                command: "device set".to_string(),
                                argument: "device".to_string(),
                            })
                        }
                    }
                    _ => Err(ParseError::UnknownCommand {
                        command: format!("device {}", args[1]),
                    }),
                }
            }
            "help" => Err(ParseError::HelpRequested),
            _ => Err(ParseError::UnknownCommand {
                command: args[0].to_string(),
            }),
        }
    }

    /// Display player status in a formatted way
    pub fn display_status(&self, status: &PlayerStatus) {
        StatusDisplay::display_full_status(status);
    }

    /// Display error message with formatting
    pub fn display_error(&self, error: &PlayerError) {
        StatusDisplay::display_error(error);
    }

    /// Display help information
    pub fn display_help() {
        println!("High-Resolution Audio Player - Available Commands:");
        println!();
        println!("Playback Control:");
        println!("  play [path]     - Start playback (optionally specify file/directory)");
        println!("  pause           - Pause playback");
        println!("  resume          - Resume playback");
        println!("  stop            - Stop playback and reset position");
        println!("  next            - Next track");
        println!("  prev            - Previous track");
        println!("  seek <time>     - Seek to position (e.g., '1:30', '90s')");
        println!();
        println!("Information:");
        println!("  status          - Show current player status");
        println!("  volume <0-100>  - Set volume level");
        println!();
        println!("Queue Management:");
        println!("  queue add <path>    - Add file/directory to queue");
        println!("  queue list          - List queue contents");
        println!("  queue clear         - Clear queue");
        println!("  queue position      - Show current position in queue");
        println!();
        println!("Playlist Management:");
        println!("  playlist save <name>    - Save current queue as playlist");
        println!("  playlist load <name>    - Load playlist");
        println!("  playlist list           - List available playlists");
        println!("  playlist delete <name>  - Delete playlist");
        println!();
        println!("Device Management:");
        println!("  device list         - List available audio devices");
        println!("  device set <name>   - Set audio output device");
        println!();
        println!("General:");
        println!("  help            - Show this help message");
        println!("  exit, quit      - Exit the player");
    }

    /// Truncate string to fit display width
    fn truncate(s: &str, max_len: usize) -> String {
        if s.len() <= max_len {
            s.to_string()
        } else {
            format!("{}...", &s[..max_len.saturating_sub(3)])
        }
    }

    /// Parse time string to Duration with enhanced validation
    pub fn parse_time(time_str: &str) -> Result<Duration, ParseError> {
        let trimmed = time_str.trim();

        if trimmed.is_empty() {
            return Err(ParseError::InvalidTimeFormat {
                input: time_str.to_string(),
            });
        }

        // Handle different time formats: "1:30", "90", "90s", "1:30.5"
        if trimmed.contains(':') {
            // MM:SS or MM:SS.ms format
            let parts: Vec<&str> = trimmed.split(':').collect();
            if parts.len() != 2 {
                return Err(ParseError::InvalidTimeFormat {
                    input: time_str.to_string(),
                });
            }

            let minutes: u64 = parts[0].parse().map_err(|_| ParseError::InvalidTimeFormat {
                input: time_str.to_string(),
            })?;

            // Handle seconds with optional decimal part
            let seconds_f64: f64 = parts[1].parse().map_err(|_| ParseError::InvalidTimeFormat {
                input: time_str.to_string(),
            })?;

            if seconds_f64 < 0.0 || seconds_f64 >= 60.0 {
                return Err(ParseError::InvalidTimeFormat {
                    input: time_str.to_string(),
                });
            }

            let total_seconds = minutes as f64 * 60.0 + seconds_f64;
            Ok(Duration::from_secs_f64(total_seconds))
        } else {
            // Seconds format (with or without 's' suffix), support decimal
            let seconds_str = trimmed.trim_end_matches('s');
            let seconds_f64: f64 = seconds_str.parse().map_err(|_| ParseError::InvalidTimeFormat {
                input: time_str.to_string(),
            })?;

            if seconds_f64 < 0.0 {
                return Err(ParseError::InvalidTimeFormat {
                    input: time_str.to_string(),
                });
            }

            Ok(Duration::from_secs_f64(seconds_f64))
        }
    }

    /// Validate seek position against track duration
    pub fn validate_seek_time(position: Duration, duration: Option<Duration>) -> Result<Duration, ParseError> {
        if let Some(track_duration) = duration {
            if position > track_duration {
                return Err(ParseError::SeekBeyondDuration {
                    position: position.as_secs_f64(),
                    duration: track_duration.as_secs_f64(),
                });
            }
        }
        Ok(position)
    }

    /// Format duration for display
    pub fn format_duration(duration: Duration) -> String {
        let total_seconds = duration.as_secs();
        let minutes = total_seconds / 60;
        let seconds = total_seconds % 60;
        format!("{:02}:{:02}", minutes, seconds)
    }

    pub async fn run(&mut self) -> Result<(), PlayerError> {
        // This will be implemented in later tasks when we have the audio engine
        println!("CLI Audio Player - Structure initialized");
        Ok(())
    }
}

/// Command parsing errors
#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("Empty command")]
    EmptyCommand,

    #[error("Unknown command: {command}")]
    UnknownCommand { command: String },

    #[error("Missing argument for {command}: {argument}")]
    MissingArgument { command: String, argument: String },

    #[error("Invalid argument {argument}: got '{value}', expected {expected}")]
    InvalidArgument {
        argument: String,
        value: String,
        expected: String,
    },

    #[error("Invalid time format: {input}")]
    InvalidTimeFormat { input: String },

    #[error("Seek position {position:.2}s exceeds track duration {duration:.2}s")]
    SeekBeyondDuration { position: f64, duration: f64 },

    #[error("Help requested")]
    HelpRequested,
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod path_tests {
    use super::*;

    #[test]
    fn test_expand_path_tilde_home() {
        let expanded = CliApp::expand_path("~/Documents/Music");

        // Should not start with ~ anymore
        assert!(!expanded.to_string_lossy().starts_with("~"));

        // Should contain Documents/Music
        assert!(expanded.to_string_lossy().contains("Documents/Music"));
    }

    #[test]
    fn test_expand_path_tilde_only() {
        let expanded = CliApp::expand_path("~");

        // Should not be just ~
        assert_ne!(expanded.to_string_lossy(), "~");
    }

    #[test]
    fn test_expand_path_no_tilde() {
        let path = "/absolute/path/to/file";
        let expanded = CliApp::expand_path(path);

        // Should remain unchanged
        assert_eq!(expanded.to_string_lossy(), path);
    }

    #[test]
    fn test_expand_path_relative() {
        let path = "relative/path/to/file";
        let expanded = CliApp::expand_path(path);

        // Should remain unchanged
        assert_eq!(expanded.to_string_lossy(), path);
    }
}
