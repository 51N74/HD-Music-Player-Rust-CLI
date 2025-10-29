use hires_audio_player::config::{ConfigManager, PlayerConfig};
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Configuration Management System Demo");
    println!("====================================");

    // Create a new config manager
    let mut config_manager = ConfigManager::new()?;
    
    println!("Default configuration:");
    let config = config_manager.get_config();
    println!("  Volume: {}", config.default_volume);
    println!("  Preferred Device: {:?}", config.preferred_device);
    println!("  Buffer Size: {}", config.buffer_size);
    println!("  Gapless Enabled: {}", config.enable_gapless);
    println!("  Playlist Directory: {}", config.playlist_directory.display());
    
    // Modify some settings
    println!("\nModifying configuration...");
    config_manager.set_volume(0.6)?;
    config_manager.set_preferred_device(Some("AudioQuest DragonFly".to_string()))?;
    config_manager.set_buffer_size(8192)?;
    
    println!("Updated configuration:");
    let config = config_manager.get_config();
    println!("  Volume: {}", config.default_volume);
    println!("  Preferred Device: {:?}", config.preferred_device);
    println!("  Buffer Size: {}", config.buffer_size);
    
    // Demonstrate TOML serialization
    println!("\nTOML representation:");
    let toml_string = toml::to_string_pretty(config)?;
    println!("{}", toml_string);
    
    println!("Configuration saved to: ~/.config/hires-player/config.toml");
    
    Ok(())
}