mod cli;
mod audio;
mod queue;
mod config;
mod error;
mod models;
mod logging;
mod error_recovery;

#[cfg(test)]
mod integration_tests;

use cli::{CliApp, Commands, ParseError};
use error::{PlayerError, AudioError};
use models::PlayerStatus;
use queue::QueueManager;
use audio::AudioEngine;
use logging::AudioLogger;
use error_recovery::{ErrorRecoveryManager, RecoveryResult};
use std::io::{self, Write};
use log::{info, warn, error};

/// Main application controller that coordinates all components
pub struct AppController {
    audio_engine: audio::engine::AudioEngineImpl,
    queue_manager: std::sync::Arc<std::sync::Mutex<queue::QueueManagerImpl>>,
    config_manager: config::ConfigManager,
    cli_app: CliApp,
    logger: AudioLogger,
    error_recovery: ErrorRecoveryManager,
}

impl AppController {
    /// Create a new application controller
    pub fn new() -> Result<Self, PlayerError> {
        // Initialize logging first (default to 'warn' if unspecified)
        if std::env::var("HIRES_PLAYER_LOG_LEVEL").is_err() {
            std::env::set_var("HIRES_PLAYER_LOG_LEVEL", "warn");
        }
        if std::env::var("RUST_LOG").is_err() {
            std::env::set_var("RUST_LOG", "warn");
        }
        if let Err(e) = AudioLogger::init() {
            eprintln!("Warning: Failed to initialize logging: {}", e);
        }

        let audio_engine = audio::engine::AudioEngineImpl::new()?;
        let queue_manager = std::sync::Arc::new(std::sync::Mutex::new(queue::QueueManagerImpl::new()));
        let config_manager = config::ConfigManager::new()?;
        let cli_app = CliApp::new()?;
        let logger = AudioLogger::new();
        let error_recovery = ErrorRecoveryManager::new(logger.clone());

        info!("Application controller initialized successfully");

        Ok(Self {
            audio_engine,
            queue_manager,
            config_manager,
            cli_app,
            logger,
            error_recovery,
        })
    }

    /// Initialize the application with saved configuration
    pub fn initialize(&mut self) -> Result<(), PlayerError> {
        let config = self.config_manager.get_config();

        // Set volume from config
        self.audio_engine.set_volume(config.default_volume)?;

        // Set preferred device if specified
        if let Some(device_name) = &config.preferred_device {
            if let Err(e) = self.audio_engine.set_device(device_name) {
                eprintln!("Warning: Could not set preferred device '{}': {}", device_name, e);
                eprintln!("Using default device instead.");
            }
        }

        // Set gapless playback preference
        self.audio_engine.set_gapless_enabled(config.enable_gapless);

        Ok(())
    }

    /// Execute a single command
    pub async fn execute_command(&mut self, command: Commands) -> Result<(), PlayerError> {
        match command {
            Commands::Play { path } => {
                if let Some(path) = path {
                    // Add file/directory to queue and start playback
                    if path.is_dir() {
                        self.queue_manager.lock().unwrap().add_directory(&path)?;
                    } else {
                        self.queue_manager.lock().unwrap().add_file(&path)?;
                    }
                }

                // Start playback of current track
                if let Some(track) = self.queue_manager.lock().unwrap().current_track().cloned() {
                    // Create decoder for the current track
                    use crate::audio::decoders::flac::FlacDecoder;
                    use crate::audio::decoders::wav::WavDecoder;
                    use crate::audio::decoders::mp3::Mp3Decoder;
                    use crate::audio::decoders::ogg::OggDecoder;
                    use crate::audio::decoders::m4a::M4aDecoder;

                    let extension = track.path.extension()
                        .and_then(|ext| ext.to_str())
                        .map(|s| s.to_lowercase())
                        .ok_or_else(|| PlayerError::Audio(AudioError::UnsupportedFormat {
                            format: "No file extension".to_string(),
                        }))?;

                    let decoder: Box<dyn crate::audio::AudioDecoder> = match extension.as_str() {
                        "flac" => {
                            Box::new(FlacDecoder::new(&track.path)
                                .map_err(|e| PlayerError::Audio(AudioError::InitializationFailed(format!("FLAC decoder error: {}", e))))?)
                        }
                        "wav" => {
                            Box::new(WavDecoder::new(&track.path)
                                .map_err(|e| PlayerError::Audio(AudioError::InitializationFailed(format!("WAV decoder error: {}", e))))?)
                        }
                        "mp3" => {
                            Box::new(Mp3Decoder::new(&track.path)
                                .map_err(|e| PlayerError::Audio(AudioError::InitializationFailed(format!("MP3 decoder error: {}", e))))?)
                        }
                        "ogg" | "oga" => {
                            Box::new(OggDecoder::new(&track.path)
                                .map_err(|e| PlayerError::Audio(AudioError::InitializationFailed(format!("OGG decoder error: {}", e))))?)
                        }
                        "m4a" | "mp4" | "m4b" => {
                            Box::new(M4aDecoder::new(&track.path)
                                .map_err(|e| PlayerError::Audio(AudioError::InitializationFailed(format!("M4A/MP4 decoder error: {}", e))))?)
                        }
                        _ => {
                            return Err(PlayerError::Audio(AudioError::UnsupportedFormat {
                                format: format!("Unsupported file extension: {}", extension),
                            }));
                        }
                    };

                    // Start playback with the decoder
                    self.audio_engine.start_playback(decoder)?;

                    // Poll decoder responses to trigger any auto-reconfiguration
                    let _ = self.audio_engine.get_decoder_response();
                    println!("Playing: {} - {}", track.display_name(), track.artist_name());
                } else {
                    return Err(PlayerError::Queue(error::QueueError::EmptyQueue));
                }
            }
            Commands::Pause => {
                self.audio_engine.pause()?;
                println!("OK: Paused");
            }
            Commands::Resume => {
                self.audio_engine.resume()?;
                println!("OK: Resumed");
            }
            Commands::Stop => {
                self.audio_engine.stop()?;
                println!("OK: Stopped");
            }
            Commands::Next => {
                if let Some(track) = self.queue_manager.lock().unwrap().next_track().cloned() {
                    // Load and play the next track without any preloading to avoid lock contention
                    self.audio_engine.load_file(track.path.clone())?;
                    println!("OK: Next - {}", track.display_name());
                } else {
                    println!("Queue finished");
                }
            }
            Commands::Prev => {
                if let Some(track) = self.queue_manager.lock().unwrap().previous_track().cloned() {
                    self.audio_engine.load_file(track.path.clone())?;
                    let _ = self.audio_engine.get_decoder_response();
                    println!("OK: Previous - {}", track.display_name());
                } else {
                    println!("No previous track available");
                }
            }
            Commands::Seek { position } => {
                let duration = CliApp::parse_time(&position)?;
                let validated_duration = self.audio_engine.validate_seek_position(duration)?;
                self.audio_engine.seek(validated_duration)?;
                println!("Seeked to: {}", CliApp::format_duration(validated_duration));
            }
            Commands::Status => {
                use crate::cli::status::StatusDisplay;
                // One-shot snapshot
                let status = self.get_current_status();
                self.cli_app.display_status(&status);
            }
            Commands::Watch => {
                use crate::cli::status::StatusDisplay;
                println!("Watching status (updates every 100ms). Press Ctrl-C to stop.");
                loop {
                    // Poll decoder responses and render snapshot
                    let _ = self.audio_engine.get_decoder_response();
                    let status = self.get_current_status();
                    // Clear screen and print snapshot
                    print!("\x1B[2J\x1B[H");
                    self.cli_app.display_status(&status);
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                }
            }

            Commands::Volume { level } => {
                let volume = (level as f32) / 100.0;
                self.audio_engine.set_volume(volume)?;

                // Save volume to config
                self.config_manager.set_volume(volume)?;

                println!("OK: Volume {}%", level);
            }
            Commands::Queue { action } => {
                use cli::QueueAction;
                match action {
                    QueueAction::Add { path } => {
                        if path.is_dir() {
                            self.queue_manager.lock().unwrap().add_directory(&path)?;
                            println!("OK: Added directory {}", path.display());
                        } else {
                            self.queue_manager.lock().unwrap().add_file(&path)?;
                            println!("OK: Added file {}", path.display());
                        }
                    }
                    QueueAction::List => {
                        let qm = self.queue_manager.lock().unwrap();
                        let queue = qm.list();
                        if queue.is_empty() {
                            println!("Queue is empty");
                        } else {
                            println!("Queue ({} tracks):", queue.len());
                            for (i, track) in queue.iter().enumerate() {
                                let marker = if i == qm.current_index() { ">" } else { " " };
                                println!("{} {}: {} - {}",
                                    marker,
                                    i + 1,
                                    track.artist_name(),
                                    track.display_name()
                                );
                            }
                        }
                    }
                    QueueAction::Clear => {
                        self.queue_manager.lock().unwrap().clear();
                        println!("OK: Queue cleared");
                    }
                    QueueAction::Position => {
                        let qm = self.queue_manager.lock().unwrap();
                        if let Some(track) = qm.current_track() {
                            println!("Current position: {} of {} - {} - {}",
                                qm.current_index() + 1,
                                qm.len(),
                                track.artist_name(),
                                track.display_name()
                            );
                        } else {
                            println!("No current track");
                        }
                    }
                }
            }
            Commands::Playlist { action } => {
                use cli::PlaylistAction;
                match action {
                    PlaylistAction::Save { name } => {
                        self.queue_manager.lock().unwrap().save_playlist(&name, queue::playlist::PlaylistFormat::M3u)?;
                        println!("Playlist saved: {}", name);
                    }
                    PlaylistAction::Load { name } => {
                        self.queue_manager.lock().unwrap().load_playlist(&name)?;
                        println!("Playlist loaded: {}", name);
                    }
                    PlaylistAction::List => {
                        let playlists = self.queue_manager.lock().unwrap().list_playlists()?;
                        if playlists.is_empty() {
                            println!("No playlists found");
                        } else {
                            println!("Available playlists:");
                            for playlist in playlists {
                                println!("  {}", playlist);
                            }
                        }
                    }
                    PlaylistAction::Delete { name } => {
                        self.queue_manager.lock().unwrap().delete_playlist(&name)?;
                        println!("Playlist deleted: {}", name);
                    }
                }
            }
            Commands::Device { action } => {
                use cli::DeviceAction;
                match action {
                    DeviceAction::List => {
                        let devices = self.audio_engine.device_manager().list_devices();
                        if devices.is_empty() {
                            println!("No audio devices found");
                        } else {
                            println!("Available audio devices:");
                            let current_device = self.audio_engine.device_manager().current_device_name()
                                .unwrap_or(None);

                            for device in devices {
                                let marker = if Some(&device) == current_device.as_ref() { "*" } else { " " };
                                println!("{} {}", marker, device);
                            }
                        }
                    }
                    DeviceAction::Set { device } => {
                        self.audio_engine.set_device(&device)?;

                        // Save device preference to config
                        self.config_manager.set_preferred_device(Some(device.clone()))?;

                        println!("Audio device set to: {}", device);
                    }
                }
            }
        }

        Ok(())
    }

    /// Get current player status
    fn get_current_status(&self) -> PlayerStatus {
        let mut status = PlayerStatus::new();

        // Get playback state from audio engine and convert to models::PlaybackState
        let engine_state = self.audio_engine.playback_state();
        status.state = match engine_state {
            audio::engine::PlaybackState::Stopped => models::PlaybackState::Stopped,
            audio::engine::PlaybackState::Playing => models::PlaybackState::Playing,
            audio::engine::PlaybackState::Paused => models::PlaybackState::Paused,
        };

        status.position = self.audio_engine.current_position();
        status.volume = self.audio_engine.volume();

        // Only show track info if playing or paused; otherwise show basic device/volume only
        match status.state {
            models::PlaybackState::Playing | models::PlaybackState::Paused => {
                status.current_track = self.queue_manager.lock().unwrap().current_track().cloned();
                if let Some(_track) = &status.current_track {
                    status.audio_format = Some(models::AudioFormat::new(
                        self.audio_engine.sample_rate(),
                        self.audio_engine.bit_depth(),
                        self.audio_engine.channels(),
                        models::AudioCodec::Flac, // Default for now
                    ));
                }
            }
            models::PlaybackState::Stopped => {
                status.current_track = None;
                status.audio_format = None;
            }
        }

        // Get current device name
        status.output_device = self.audio_engine.device_manager().current_device_name()
            .unwrap_or(None);

        status
    }

    /// Run interactive mode
    pub async fn run_interactive_mode(&mut self) -> Result<(), PlayerError> {
        println!("High-Resolution Audio Player v0.1.0");
        println!("Type 'help' for available commands, 'exit' or 'quit' to quit.");
        println!();

        // Set up graceful shutdown handling
        let shutdown_flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let shutdown_flag_clone = shutdown_flag.clone();

        ctrlc::set_handler(move || {
            println!("\nReceived interrupt signal. Shutting down gracefully...");
            shutdown_flag_clone.store(true, std::sync::atomic::Ordering::Relaxed);
        }).expect("Error setting Ctrl-C handler");

        // Non-blocking input with 100ms polling using a dedicated stdin thread
        let mut interval = tokio::time::interval(std::time::Duration::from_millis(100));
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
        std::thread::spawn(move || {
            let stdin = std::io::stdin();
            let mut line = String::new();
            loop {
                line.clear();
                match stdin.read_line(&mut line) {
                    Ok(0) => {
                        // EOF
                        let _ = tx.send(String::new());
                        break;
                    }
                    Ok(_) => {
                        let s = line.trim().to_string();
                        let _ = tx.send(s);
                    }
                    Err(_) => {
                        let _ = tx.send(String::new());
                        break;
                    }
                }
            }
        });
        let mut awaiting_input = false;
        let mut announced_queue_finished = false;

        loop {
            // Check for shutdown signal
            if shutdown_flag.load(std::sync::atomic::Ordering::Relaxed) {
                break;
            }


            // Print prompt only when not already awaiting input
            if !awaiting_input {
                print!("> ");
                let _ = std::io::Write::flush(&mut std::io::stdout());
                awaiting_input = true;
            }

            tokio::select! {
                biased;

                // Handle user input when it arrives (from stdin thread)
                line = rx.recv() => {
                    awaiting_input = false;
                    match line {
                        Some(line) => {
                            let line = line.trim().to_string();
                            if line.is_empty() {
                                continue;
                            }
                            if line == "exit" || line == "quit" {
                                println!("Goodbye!");
                                break;
                            }
                            match CliApp::parse_command(&line) {
                                Ok(command) => {
                                    if let Err(e) = self.execute_command(command).await {
                                        self.handle_error_with_recovery(&e).await;
                                    }
                                }
                                Err(ParseError::HelpRequested) => {
                                    CliApp::display_help();
                                }
                                Err(e) => {
                                    eprintln!("Error: {}", e);
                                    println!("Type 'help' for available commands.");
                                }
                            }
                        }
                        None => {
                            // Channel closed / EOF
                            println!();
                            break;
                        }
                    }
                }

                // 100ms poll: process engine events and keep prompt responsive
                _ = interval.tick() => {
                    // Poll decoder responses to trigger any auto-reconfiguration and keep next track preloaded
                    if let Some(resp) = self.audio_engine.get_decoder_response() {
                        use crate::audio::engine::DecoderResponse;
                        match resp {
                            DecoderResponse::FileLoaded { .. } | DecoderResponse::TrackTransitioned => {
                                // Announce the new track title and reset completion flag
                                if let Some(track) = self.queue_manager.lock().unwrap().current_track() {
                                    println!("Now playing: {} - {}", track.display_name(), track.artist_name());
                                }
                                announced_queue_finished = false;
                            }
                            DecoderResponse::EndOfFile => {
                                if !announced_queue_finished {
                                    println!("\nQueue finished");
                                    announced_queue_finished = true;
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }

        }

        // Perform graceful shutdown
        self.shutdown().await?;

        Ok(())
    }

    /// Perform graceful shutdown with resource cleanup and configuration saving
    pub async fn shutdown(&mut self) -> Result<(), PlayerError> {
        println!("Shutting down...");

        // Stop audio playback
        if let Err(e) = self.audio_engine.stop() {
            eprintln!("Warning: Error stopping audio engine: {}", e);
        }

        // Save current configuration
        if let Err(e) = self.save_current_config() {
            eprintln!("Warning: Error saving configuration: {}", e);
        }

        println!("Shutdown complete.");
        Ok(())
    }

    /// Handle error with automatic recovery attempts
    async fn handle_error_with_recovery(&mut self, error: &PlayerError) {
        error!("Error occurred: {}", error);

        // Log the error with appropriate severity
        let severity = error.severity();
        match severity {
            error::ErrorSeverity::Info => info!("Info: {}", error),
            error::ErrorSeverity::Warning => warn!("Warning: {}", error),
            error::ErrorSeverity::Error | error::ErrorSeverity::Critical => {
                error!("Error: {}", error);
            }
        }

        // Attempt automatic recovery if the error is recoverable
        if error.is_recoverable() {
            match self.error_recovery.attempt_recovery(error).await {
                RecoveryResult::Success(msg) => {
                    info!("Recovery successful: {}", msg);
                    println!("✓ Recovered: {}", msg);
                    return;
                }
                RecoveryResult::Retry(msg) => {
                    info!("Recovery suggests retry: {}", msg);
                    println!("🔄 Recovery suggestion: {}", msg);
                }
                RecoveryResult::Failed(msg) => {
                    warn!("Recovery failed: {}", msg);
                    // Fall through to display full error
                }
            }
        }

        // Display error with recovery information
        use cli::status::StatusDisplay;
        StatusDisplay::display_error_with_recovery(error, error.is_recoverable());
    }

    /// Save current state to configuration
    fn save_current_config(&mut self) -> Result<(), PlayerError> {
        // Update config with current settings
        self.config_manager.update_config(|config| {
            config.default_volume = self.audio_engine.volume();
            config.enable_gapless = self.audio_engine.is_gapless_enabled();

            // Save current device if available
            if let Ok(Some(device_name)) = self.audio_engine.device_manager().current_device_name() {
                config.preferred_device = Some(device_name);
            }
        })?;

        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<(), PlayerError> {
    // Create and initialize application controller
    let mut app = match AppController::new() {
        Ok(app) => app,
        Err(e) => {
            eprintln!("Failed to initialize application: {}", e);
            // Try to display error with basic formatting if CLI is not available
            use cli::status::StatusDisplay;
            StatusDisplay::display_simple_error(&e);
            std::process::exit(1);
        }
    };

    // Install engine-level next-track provider that pulls from the queue manager (thread-safe).
    struct QueueNextTrackProvider {
        qm: std::sync::Arc<std::sync::Mutex<crate::queue::QueueManagerImpl>>,
    }
    impl crate::audio::engine::NextTrackProvider for QueueNextTrackProvider {
        fn request_next(&self) -> Option<std::path::PathBuf> {
            let mut qm = self.qm.lock().unwrap();
            let len = qm.len();
            if len == 0 {
                return None;
            }
            let cur = qm.current_index();
            // Advance to the next track without wrapping. If at the end, signal completion.
            if cur + 1 < len {
                let _ = qm.jump_to(cur + 1);
                if let Some(track) = qm.current_track() {
                    return Some(track.path.clone());
                }
            }
            None
        }
    }
    let provider = std::sync::Arc::new(QueueNextTrackProvider {
        qm: app.queue_manager.clone(),
    });
    app.audio_engine.set_next_track_provider(provider);

    if let Err(e) = app.initialize() {
        error!("Failed to initialize application: {}", e);
        app.handle_error_with_recovery(&e).await;
        std::process::exit(1);
    }

    // Parse command line arguments
    let cli = CliApp::parse();

    match cli.command {
        Some(command) => {
            // Single command mode
            if let Err(e) = app.execute_command(command).await {
                app.handle_error_with_recovery(&e).await;
                std::process::exit(1);
            }
        }
        None => {
            // Interactive mode
            if let Err(e) = app.run_interactive_mode().await {
                app.handle_error_with_recovery(&e).await;
                std::process::exit(1);
            }
        }
    }

    info!("Application shutdown complete");
    Ok(())
}
