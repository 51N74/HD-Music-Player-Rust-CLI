use std::path::PathBuf;
use serde::{Deserialize, Serialize};
use crate::error::ConfigError;

/// Player configuration structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerConfig {
    pub default_volume: f32,
    pub preferred_device: Option<String>,
    pub buffer_size: usize,
    pub enable_gapless: bool,
    pub playlist_directory: PathBuf,
}

impl Default for PlayerConfig {
    fn default() -> Self {
        Self {
            default_volume: 0.8,
            preferred_device: None,
            buffer_size: 4096,
            enable_gapless: true,
            playlist_directory: dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".config")
                .join("hires-player")
                .join("playlists"),
        }
    }
}

/// Configuration manager for loading and saving settings
pub struct ConfigManager {
    config: PlayerConfig,
    config_path: PathBuf,
}

impl ConfigManager {
    pub fn new() -> Result<Self, ConfigError> {
        let config_path = Self::get_config_path()?;
        let config = Self::load_config(&config_path).unwrap_or_default();
        
        Ok(Self {
            config,
            config_path,
        })
    }

    pub fn get_config(&self) -> &PlayerConfig {
        &self.config
    }

    pub fn update_config<F>(&mut self, updater: F) -> Result<(), ConfigError>
    where
        F: FnOnce(&mut PlayerConfig),
    {
        updater(&mut self.config);
        self.save_config()
    }

    pub fn set_volume(&mut self, volume: f32) -> Result<(), ConfigError> {
        self.config.default_volume = volume.clamp(0.0, 1.0);
        self.save_config()
    }

    pub fn set_preferred_device(&mut self, device: Option<String>) -> Result<(), ConfigError> {
        self.config.preferred_device = device;
        self.save_config()
    }

    pub fn set_buffer_size(&mut self, buffer_size: usize) -> Result<(), ConfigError> {
        self.config.buffer_size = buffer_size;
        self.save_config()
    }

    pub fn set_gapless_enabled(&mut self, enabled: bool) -> Result<(), ConfigError> {
        self.config.enable_gapless = enabled;
        self.save_config()
    }

    pub fn set_playlist_directory(&mut self, directory: PathBuf) -> Result<(), ConfigError> {
        self.config.playlist_directory = directory;
        self.save_config()
    }

    pub fn reset_to_defaults(&mut self) -> Result<(), ConfigError> {
        self.config = PlayerConfig::default();
        self.save_config()
    }

    fn get_config_path() -> Result<PathBuf, ConfigError> {
        let config_dir = dirs::home_dir()
            .ok_or(ConfigError::ConfigDirNotFound)?
            .join(".config")
            .join("hires-player");
        
        std::fs::create_dir_all(&config_dir)
            .map_err(|e| ConfigError::IoError(e))?;
        
        Ok(config_dir.join("config.toml"))
    }

    fn load_config(path: &Path) -> Result<PlayerConfig, ConfigError> {
        if !path.exists() {
            return Ok(PlayerConfig::default());
        }

        let config_content = std::fs::read_to_string(path)
            .map_err(ConfigError::IoError)?;
        
        let config: PlayerConfig = toml::from_str(&config_content)
            .map_err(ConfigError::DeserializationError)?;
        
        Ok(config)
    }

    fn save_config(&self) -> Result<(), ConfigError> {
        // Ensure the parent directory exists
        if let Some(parent) = self.config_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(ConfigError::IoError)?;
        }

        let config_content = toml::to_string_pretty(&self.config)
            .map_err(ConfigError::SerializationError)?;
        
        std::fs::write(&self.config_path, config_content)
            .map_err(ConfigError::IoError)?;
        
        Ok(())
    }
}

use std::path::Path;

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn create_test_config_manager() -> (ConfigManager, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config.toml");
        
        let config_manager = ConfigManager {
            config: PlayerConfig::default(),
            config_path,
        };
        
        (config_manager, temp_dir)
    }

    #[test]
    fn test_player_config_default() {
        let config = PlayerConfig::default();
        
        assert_eq!(config.default_volume, 0.8);
        assert_eq!(config.preferred_device, None);
        assert_eq!(config.buffer_size, 4096);
        assert_eq!(config.enable_gapless, true);
        assert!(config.playlist_directory.to_string_lossy().contains("hires-player"));
    }

    #[test]
    fn test_config_serialization() {
        let config = PlayerConfig {
            default_volume: 0.5,
            preferred_device: Some("Test Device".to_string()),
            buffer_size: 8192,
            enable_gapless: false,
            playlist_directory: PathBuf::from("/test/playlists"),
        };

        let serialized = toml::to_string(&config).unwrap();
        let deserialized: PlayerConfig = toml::from_str(&serialized).unwrap();

        assert_eq!(config.default_volume, deserialized.default_volume);
        assert_eq!(config.preferred_device, deserialized.preferred_device);
        assert_eq!(config.buffer_size, deserialized.buffer_size);
        assert_eq!(config.enable_gapless, deserialized.enable_gapless);
        assert_eq!(config.playlist_directory, deserialized.playlist_directory);
    }

    #[test]
    fn test_save_and_load_config() {
        let (mut config_manager, _temp_dir) = create_test_config_manager();
        
        // Modify config
        config_manager.config.default_volume = 0.6;
        config_manager.config.preferred_device = Some("Test Device".to_string());
        config_manager.config.buffer_size = 2048;
        
        // Save config
        config_manager.save_config().unwrap();
        
        // Load config from file
        let loaded_config = ConfigManager::load_config(&config_manager.config_path).unwrap();
        
        assert_eq!(loaded_config.default_volume, 0.6);
        assert_eq!(loaded_config.preferred_device, Some("Test Device".to_string()));
        assert_eq!(loaded_config.buffer_size, 2048);
    }

    #[test]
    fn test_load_nonexistent_config() {
        let temp_dir = TempDir::new().unwrap();
        let nonexistent_path = temp_dir.path().join("nonexistent.toml");
        
        let config = ConfigManager::load_config(&nonexistent_path).unwrap();
        
        // Should return default config
        assert_eq!(config.default_volume, PlayerConfig::default().default_volume);
        assert_eq!(config.preferred_device, PlayerConfig::default().preferred_device);
    }

    #[test]
    fn test_load_invalid_config() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("invalid.toml");
        
        // Write invalid TOML
        fs::write(&config_path, "invalid toml content [[[").unwrap();
        
        let result = ConfigManager::load_config(&config_path);
        assert!(result.is_err());
        
        match result.unwrap_err() {
            ConfigError::DeserializationError(_) => {
                // Expected error type
            }
            _ => panic!("Expected DeserializationError"),
        }
    }

    #[test]
    fn test_update_config() {
        let (mut config_manager, _temp_dir) = create_test_config_manager();
        
        config_manager.update_config(|config| {
            config.default_volume = 0.9;
            config.buffer_size = 1024;
        }).unwrap();
        
        assert_eq!(config_manager.config.default_volume, 0.9);
        assert_eq!(config_manager.config.buffer_size, 1024);
        
        // Verify it was saved
        let loaded_config = ConfigManager::load_config(&config_manager.config_path).unwrap();
        assert_eq!(loaded_config.default_volume, 0.9);
        assert_eq!(loaded_config.buffer_size, 1024);
    }

    #[test]
    fn test_set_volume() {
        let (mut config_manager, _temp_dir) = create_test_config_manager();
        
        // Test normal volume
        config_manager.set_volume(0.7).unwrap();
        assert_eq!(config_manager.config.default_volume, 0.7);
        
        // Test volume clamping - too high
        config_manager.set_volume(1.5).unwrap();
        assert_eq!(config_manager.config.default_volume, 1.0);
        
        // Test volume clamping - too low
        config_manager.set_volume(-0.5).unwrap();
        assert_eq!(config_manager.config.default_volume, 0.0);
    }

    #[test]
    fn test_set_preferred_device() {
        let (mut config_manager, _temp_dir) = create_test_config_manager();
        
        config_manager.set_preferred_device(Some("My DAC".to_string())).unwrap();
        assert_eq!(config_manager.config.preferred_device, Some("My DAC".to_string()));
        
        config_manager.set_preferred_device(None).unwrap();
        assert_eq!(config_manager.config.preferred_device, None);
    }

    #[test]
    fn test_set_buffer_size() {
        let (mut config_manager, _temp_dir) = create_test_config_manager();
        
        config_manager.set_buffer_size(8192).unwrap();
        assert_eq!(config_manager.config.buffer_size, 8192);
    }

    #[test]
    fn test_set_gapless_enabled() {
        let (mut config_manager, _temp_dir) = create_test_config_manager();
        
        config_manager.set_gapless_enabled(false).unwrap();
        assert_eq!(config_manager.config.enable_gapless, false);
        
        config_manager.set_gapless_enabled(true).unwrap();
        assert_eq!(config_manager.config.enable_gapless, true);
    }

    #[test]
    fn test_set_playlist_directory() {
        let (mut config_manager, _temp_dir) = create_test_config_manager();
        
        let new_dir = PathBuf::from("/custom/playlist/dir");
        config_manager.set_playlist_directory(new_dir.clone()).unwrap();
        assert_eq!(config_manager.config.playlist_directory, new_dir);
    }

    #[test]
    fn test_reset_to_defaults() {
        let (mut config_manager, _temp_dir) = create_test_config_manager();
        
        // Modify config
        config_manager.config.default_volume = 0.1;
        config_manager.config.preferred_device = Some("Custom Device".to_string());
        config_manager.config.buffer_size = 512;
        
        // Reset to defaults
        config_manager.reset_to_defaults().unwrap();
        
        let default_config = PlayerConfig::default();
        assert_eq!(config_manager.config.default_volume, default_config.default_volume);
        assert_eq!(config_manager.config.preferred_device, default_config.preferred_device);
        assert_eq!(config_manager.config.buffer_size, default_config.buffer_size);
    }

    #[test]
    fn test_config_path_creation() {
        let temp_dir = TempDir::new().unwrap();
        let nested_path = temp_dir.path().join("nested").join("config").join("config.toml");
        
        let config_manager = ConfigManager {
            config: PlayerConfig::default(),
            config_path: nested_path.clone(),
        };
        
        // Save should create the directory structure
        config_manager.save_config().unwrap();
        
        assert!(nested_path.exists());
        assert!(nested_path.parent().unwrap().exists());
    }

    #[test]
    fn test_config_manager_new() {
        // This test might fail in some environments due to home directory access
        // but it's important to test the actual constructor
        match ConfigManager::new() {
            Ok(config_manager) => {
                assert!(config_manager.config_path.to_string_lossy().contains("hires-player"));
                assert!(config_manager.config_path.to_string_lossy().contains("config.toml"));
            }
            Err(ConfigError::ConfigDirNotFound) => {
                // This is acceptable in test environments without home directories
            }
            Err(e) => panic!("Unexpected error: {}", e),
        }
    }

    #[test]
    fn test_toml_format() {
        let config = PlayerConfig {
            default_volume: 0.75,
            preferred_device: Some("AudioQuest DragonFly".to_string()),
            buffer_size: 4096,
            enable_gapless: true,
            playlist_directory: PathBuf::from("/Users/test/.config/hires-player/playlists"),
        };

        let toml_string = toml::to_string_pretty(&config).unwrap();
        
        // Verify TOML format contains expected fields
        assert!(toml_string.contains("default_volume"));
        assert!(toml_string.contains("preferred_device"));
        assert!(toml_string.contains("buffer_size"));
        assert!(toml_string.contains("enable_gapless"));
        assert!(toml_string.contains("playlist_directory"));
        
        // Verify values
        assert!(toml_string.contains("0.75"));
        assert!(toml_string.contains("AudioQuest DragonFly"));
        assert!(toml_string.contains("4096"));
        assert!(toml_string.contains("true"));
    }

    #[test]
    fn test_config_persistence_across_instances() {
        let (mut config_manager1, temp_dir) = create_test_config_manager();
        let config_path = config_manager1.config_path.clone();
        
        // Modify and save config with first instance
        config_manager1.set_volume(0.3).unwrap();
        config_manager1.set_preferred_device(Some("Test Device".to_string())).unwrap();
        
        // Create second instance with same path
        let config_manager2 = ConfigManager {
            config: ConfigManager::load_config(&config_path).unwrap(),
            config_path: config_path.clone(),
        };
        
        // Verify second instance has the same config
        assert_eq!(config_manager2.config.default_volume, 0.3);
        assert_eq!(config_manager2.config.preferred_device, Some("Test Device".to_string()));
        
        // Keep temp_dir alive
        drop(temp_dir);
    }
}