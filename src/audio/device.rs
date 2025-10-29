use cpal::{Device, Host, SupportedStreamConfig, SampleFormat};
use cpal::traits::{DeviceTrait, HostTrait};
use std::collections::HashMap;
use crate::error::AudioError;

/// Information about an audio device's capabilities
#[derive(Debug, Clone)]
pub struct DeviceCapabilities {
    pub name: String,
    pub supported_sample_rates: Vec<u32>,
    pub supported_bit_depths: Vec<u16>,
    pub max_channels: u16,
    pub default_config: SupportedStreamConfig,
}

/// Manages audio device enumeration and selection
pub struct DeviceManager {
    host: Host,
    devices: HashMap<String, Device>,
    current_device: Option<Device>,
    device_capabilities: HashMap<String, DeviceCapabilities>,
}

impl DeviceManager {
    /// Create a new DeviceManager instance
    pub fn new() -> Result<Self, AudioError> {
        let host = cpal::default_host();
        let mut manager = DeviceManager {
            host,
            devices: HashMap::new(),
            current_device: None,
            device_capabilities: HashMap::new(),
        };
        
        manager.refresh_devices()?;
        Ok(manager)
    }

    /// Refresh the list of available audio devices
    pub fn refresh_devices(&mut self) -> Result<(), AudioError> {
        self.devices.clear();
        self.device_capabilities.clear();

        // Get output devices
        let devices = self.host.output_devices()
            .map_err(|e| AudioError::InitializationFailed(format!("Failed to enumerate devices: {}", e)))?;

        for device in devices {
            let device_name = device.name()
                .map_err(|e| AudioError::InitializationFailed(format!("Failed to get device name: {}", e)))?;
            
            // Get device capabilities
            let capabilities = self.get_device_capabilities(&device)?;
            
            self.devices.insert(device_name.clone(), device);
            self.device_capabilities.insert(device_name, capabilities);
        }

        Ok(())
    }

    /// Get a list of all available device names
    pub fn list_devices(&self) -> Vec<String> {
        self.devices.keys().cloned().collect()
    }

    /// Get capabilities for a specific device
    pub fn get_capabilities(&self, device_name: &str) -> Option<&DeviceCapabilities> {
        self.device_capabilities.get(device_name)
    }

    /// Select a device by name, with fallback to default device
    pub fn select_device(&mut self, device_name: Option<&str>) -> Result<(), AudioError> {
        match device_name {
            Some(name) => {
                if let Some(device) = self.devices.get(name).cloned() {
                    self.current_device = Some(device);
                    Ok(())
                } else {
                    // Device not found, return error
                    Err(AudioError::DeviceNotFound { 
                        device: name.to_string() 
                    })
                }
            }
            None => self.select_default_device(),
        }
    }

    /// Select a device by name with fallback to default device
    pub fn select_device_with_fallback(&mut self, device_name: Option<&str>) -> Result<(), AudioError> {
        match device_name {
            Some(name) => {
                if let Some(device) = self.devices.get(name).cloned() {
                    self.current_device = Some(device);
                    Ok(())
                } else {
                    // Device not found, try fallback to default
                    self.select_default_device()
                        .map_err(|_| AudioError::DeviceNotFound { 
                            device: name.to_string() 
                        })
                }
            }
            None => self.select_default_device(),
        }
    }

    /// Select the default audio device
    pub fn select_default_device(&mut self) -> Result<(), AudioError> {
        let default_device = self.host.default_output_device()
            .ok_or_else(|| AudioError::InitializationFailed("No default output device available".to_string()))?;
        
        self.current_device = Some(default_device);
        Ok(())
    }

    /// Get the currently selected device
    pub fn current_device(&self) -> Option<&Device> {
        self.current_device.as_ref()
    }

    /// Get the name of the currently selected device
    pub fn current_device_name(&self) -> Result<Option<String>, AudioError> {
        match &self.current_device {
            Some(device) => {
                let name = device.name()
                    .map_err(|e| AudioError::InitializationFailed(format!("Failed to get device name: {}", e)))?;
                Ok(Some(name))
            }
            None => Ok(None),
        }
    }

    /// Check if a device supports a specific sample rate and bit depth
    pub fn supports_format(&self, device_name: &str, sample_rate: u32, bit_depth: u16) -> bool {
        if let Some(capabilities) = self.device_capabilities.get(device_name) {
            capabilities.supported_sample_rates.contains(&sample_rate) &&
            capabilities.supported_bit_depths.contains(&bit_depth)
        } else {
            false
        }
    }

    /// Get the best supported configuration for a device given preferred settings
    pub fn get_best_config(&self, device_name: &str, preferred_sample_rate: Option<u32>, _preferred_bit_depth: Option<u16>) -> Option<SupportedStreamConfig> {
        let capabilities = self.device_capabilities.get(device_name)?;
        
        // For now, return the default config
        // In a more advanced implementation, we would create a custom config
        // that matches the preferred settings if supported
        let best_config = capabilities.default_config.clone();
        
        // Check if preferred sample rate is supported
        if let Some(preferred_rate) = preferred_sample_rate {
            if capabilities.supported_sample_rates.contains(&preferred_rate) {
                // The SupportedStreamConfig doesn't have a with_sample_rate method
                // We would need to use the device's supported_output_configs to find
                // a matching configuration, but for now we'll return the default
            }
        }
        
        Some(best_config)
    }

    /// Get device capabilities by analyzing supported configurations
    fn get_device_capabilities(&self, device: &Device) -> Result<DeviceCapabilities, AudioError> {
        let device_name = device.name()
            .map_err(|e| AudioError::InitializationFailed(format!("Failed to get device name: {}", e)))?;

        // Get default configuration
        let default_config = device.default_output_config()
            .map_err(|e| AudioError::InitializationFailed(format!("Failed to get default config for {}: {}", device_name, e)))?;

        // Get supported configurations to determine capabilities
        let supported_configs = device.supported_output_configs()
            .map_err(|e| AudioError::InitializationFailed(format!("Failed to get supported configs for {}: {}", device_name, e)))?;

        let mut sample_rates = Vec::new();
        let mut bit_depths = Vec::new();
        let mut max_channels = 0;

        for config_range in supported_configs {
            // Collect sample rates
            let min_rate = config_range.min_sample_rate().0;
            let max_rate = config_range.max_sample_rate().0;
            
            // Add common sample rates within the supported range
            for &rate in &[44100, 48000, 88200, 96000, 176400, 192000, 352800, 384000] {
                if rate >= min_rate && rate <= max_rate && !sample_rates.contains(&rate) {
                    sample_rates.push(rate);
                }
            }

            // Collect bit depths based on sample format
            let sample_format = config_range.sample_format();
            let bit_depth = match sample_format {
                SampleFormat::I8 => 8,
                SampleFormat::I16 => 16,
                SampleFormat::I32 => 32,
                SampleFormat::I64 => 64,
                SampleFormat::U8 => 8,
                SampleFormat::U16 => 16,
                SampleFormat::U32 => 32,
                SampleFormat::U64 => 64,
                SampleFormat::F32 => 32,
                SampleFormat::F64 => 64,
                _ => continue, // Skip unknown formats
            };

            if !bit_depths.contains(&bit_depth) {
                bit_depths.push(bit_depth);
            }

            // Track maximum channels
            let channels = config_range.channels();
            if channels > max_channels {
                max_channels = channels;
            }
        }

        // Sort for consistent ordering
        sample_rates.sort();
        bit_depths.sort();

        // Ensure we have at least the default values
        if sample_rates.is_empty() {
            sample_rates.push(default_config.sample_rate().0);
        }
        if bit_depths.is_empty() {
            let default_bit_depth = match default_config.sample_format() {
                SampleFormat::I8 | SampleFormat::U8 => 8,
                SampleFormat::I16 | SampleFormat::U16 => 16,
                SampleFormat::I32 | SampleFormat::U32 | SampleFormat::F32 => 32,
                SampleFormat::I64 | SampleFormat::U64 | SampleFormat::F64 => 64,
                _ => 16, // Default fallback
            };
            bit_depths.push(default_bit_depth);
        }
        if max_channels == 0 {
            max_channels = default_config.channels();
        }

        Ok(DeviceCapabilities {
            name: device_name,
            supported_sample_rates: sample_rates,
            supported_bit_depths: bit_depths,
            max_channels,
            default_config,
        })
    }
}

impl Default for DeviceManager {
    fn default() -> Self {
        Self::new().expect("Failed to create default DeviceManager")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_device_manager_creation() {
        let result = DeviceManager::new();
        assert!(result.is_ok(), "DeviceManager creation should succeed");
    }

    #[test]
    fn test_device_enumeration() {
        let manager = DeviceManager::new().expect("Failed to create DeviceManager");
        let devices = manager.list_devices();
        
        // On macOS, there should be at least one device (built-in output)
        assert!(!devices.is_empty(), "Should have at least one audio device");
        
        // Check that device names are not empty
        for device_name in &devices {
            assert!(!device_name.is_empty(), "Device name should not be empty");
        }
    }

    #[test]
    fn test_default_device_selection() {
        let mut manager = DeviceManager::new().expect("Failed to create DeviceManager");
        let result = manager.select_default_device();
        
        assert!(result.is_ok(), "Default device selection should succeed");
        assert!(manager.current_device().is_some(), "Should have a current device after selection");
        
        let device_name = manager.current_device_name().expect("Should get device name");
        assert!(device_name.is_some(), "Device name should be available");
    }

    #[test]
    fn test_device_selection_by_name() {
        let mut manager = DeviceManager::new().expect("Failed to create DeviceManager");
        let devices = manager.list_devices();
        
        if !devices.is_empty() {
            let first_device = &devices[0];
            let result = manager.select_device(Some(first_device));
            
            assert!(result.is_ok(), "Device selection by name should succeed");
            
            let current_name = manager.current_device_name().expect("Should get device name");
            assert_eq!(current_name.as_ref(), Some(first_device), "Selected device should match requested device");
        }
    }

    #[test]
    fn test_invalid_device_selection() {
        let mut manager = DeviceManager::new().expect("Failed to create DeviceManager");
        let result = manager.select_device(Some("NonExistentDevice"));
        
        assert!(result.is_err(), "Invalid device selection should fail");
        
        match result {
            Err(AudioError::DeviceNotFound { device }) => {
                assert_eq!(device, "NonExistentDevice");
            }
            _ => panic!("Expected DeviceNotFound error"),
        }
    }

    #[test]
    fn test_device_capabilities() {
        let manager = DeviceManager::new().expect("Failed to create DeviceManager");
        let devices = manager.list_devices();
        
        if !devices.is_empty() {
            let first_device = &devices[0];
            let capabilities = manager.get_capabilities(first_device);
            
            assert!(capabilities.is_some(), "Should have capabilities for existing device");
            
            let caps = capabilities.unwrap();
            assert_eq!(caps.name, *first_device, "Capability name should match device name");
            assert!(!caps.supported_sample_rates.is_empty(), "Should have supported sample rates");
            assert!(!caps.supported_bit_depths.is_empty(), "Should have supported bit depths");
            assert!(caps.max_channels > 0, "Should have at least one channel");
        }
    }

    #[test]
    fn test_format_support_check() {
        let manager = DeviceManager::new().expect("Failed to create DeviceManager");
        let devices = manager.list_devices();
        
        if !devices.is_empty() {
            let first_device = &devices[0];
            
            // Get the capabilities to see what's actually supported
            let capabilities = manager.get_capabilities(first_device).expect("Should have capabilities");
            
            // Test with the device's own supported formats
            if !capabilities.supported_sample_rates.is_empty() && !capabilities.supported_bit_depths.is_empty() {
                let first_rate = capabilities.supported_sample_rates[0];
                let first_depth = capabilities.supported_bit_depths[0];
                
                let supports_own_format = manager.supports_format(first_device, first_rate, first_depth);
                assert!(supports_own_format, "Should support its own reported format");
            }
            
            // Test with an unlikely format
            let supports_unusual = manager.supports_format(first_device, 999999, 128);
            assert!(!supports_unusual, "Should not support unusual format");
        }
    }

    #[test]
    fn test_best_config_selection() {
        let manager = DeviceManager::new().expect("Failed to create DeviceManager");
        let devices = manager.list_devices();
        
        if !devices.is_empty() {
            let first_device = &devices[0];
            
            // Test getting best config with preferences
            let config = manager.get_best_config(first_device, Some(48000), Some(16));
            assert!(config.is_some(), "Should get a best config");
            
            // Test getting best config without preferences
            let config_no_prefs = manager.get_best_config(first_device, None, None);
            assert!(config_no_prefs.is_some(), "Should get a best config without preferences");
        }
    }

    #[test]
    fn test_device_refresh() {
        let mut manager = DeviceManager::new().expect("Failed to create DeviceManager");
        let initial_devices = manager.list_devices();
        
        let result = manager.refresh_devices();
        assert!(result.is_ok(), "Device refresh should succeed");
        
        let refreshed_devices = manager.list_devices();
        assert_eq!(initial_devices.len(), refreshed_devices.len(), "Device count should remain the same after refresh");
    }

    #[test]
    fn test_capabilities_contain_default_values() {
        let manager = DeviceManager::new().expect("Failed to create DeviceManager");
        let devices = manager.list_devices();
        
        if !devices.is_empty() {
            let first_device = &devices[0];
            let capabilities = manager.get_capabilities(first_device).expect("Should have capabilities");
            
            // The default config's sample rate should be in supported sample rates
            let default_rate = capabilities.default_config.sample_rate().0;
            assert!(capabilities.supported_sample_rates.contains(&default_rate), 
                   "Default sample rate should be in supported rates");
        }
    }

    #[test]
    fn test_no_current_device_initially() {
        let manager = DeviceManager::new().expect("Failed to create DeviceManager");
        assert!(manager.current_device().is_none(), "Should have no current device initially");
        
        let name = manager.current_device_name().expect("Should get device name result");
        assert!(name.is_none(), "Should have no current device name initially");
    }

    #[test]
    fn test_select_device_none() {
        let mut manager = DeviceManager::new().expect("Failed to create DeviceManager");
        let result = manager.select_device(None);
        
        assert!(result.is_ok(), "Selecting None should default to default device");
        assert!(manager.current_device().is_some(), "Should have a current device after selecting None");
    }
}