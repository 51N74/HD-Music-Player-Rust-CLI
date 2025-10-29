pub mod engine;
pub mod decoders;
pub mod device;
pub mod buffer;
pub mod metadata;
pub mod position;
pub mod gapless;
pub mod performance;
pub mod memory;
pub mod resampler;

#[cfg(test)]
pub mod tests;

use std::time::Duration;
use crate::error::{AudioError, DecodeError};

// Re-export device management types
pub use device::{DeviceManager, DeviceCapabilities};

// Re-export decoder types
pub use decoders::{FlacDecoder, WavDecoder, AlacDecoder, Mp3Decoder, OggDecoder, M4aDecoder};

// Re-export buffer management types
pub use buffer::{RingBuffer, BufferManager, BufferStatus};

// Re-export models for convenience
pub use crate::models::{AudioBuffer, AudioMetadata, AudioFormat, AudioCodec};

// Re-export metadata extraction
pub use metadata::MetadataExtractor;

// Re-export position tracking
pub use position::{PositionTracker, PositionUpdate};

// Re-export gapless playback
pub use gapless::GaplessManager;

// Re-export performance monitoring
pub use performance::{AudioPerformanceProfiler, PerformanceReport, PerformanceStats};

// Re-export memory management
pub use memory::{AudioMemoryManager, HighResBufferAllocator, ManagedAudioBuffer, MemoryStats};
pub use resampler::LinearResampler;

/// Core trait for audio decoding functionality
pub trait AudioDecoder: Send {
    /// Decode the next chunk of audio data
    fn decode_next(&mut self) -> Result<Option<AudioBuffer>, DecodeError>;

    /// Seek to a specific position in the audio stream
    fn seek(&mut self, position: Duration) -> Result<(), DecodeError>;

    /// Get metadata information about the audio file
    fn metadata(&self) -> &AudioMetadata;

    /// Get the total duration of the audio file
    fn duration(&self) -> Duration;

    /// Get the sample rate of the audio file
    fn sample_rate(&self) -> u32;

    /// Get the bit depth of the audio file
    fn bit_depth(&self) -> u16;

    /// Get the number of audio channels
    fn channels(&self) -> u16;
}

/// Core trait for audio engine functionality
pub trait AudioEngine {
    /// Start playback with the given decoder
    fn start_playback(&mut self, decoder: Box<dyn AudioDecoder>) -> Result<(), AudioError>;

    /// Pause current playback
    fn pause(&mut self) -> Result<(), AudioError>;

    /// Resume paused playback
    fn resume(&mut self) -> Result<(), AudioError>;

    /// Stop playback completely
    fn stop(&mut self) -> Result<(), AudioError>;

    /// Set the output volume (0.0 to 1.0)
    fn set_volume(&mut self, volume: f32) -> Result<(), AudioError>;

    /// Set the output device
    fn set_device(&mut self, device_name: &str) -> Result<(), AudioError>;
}
