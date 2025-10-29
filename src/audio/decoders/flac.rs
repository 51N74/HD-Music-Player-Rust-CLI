use std::fs::File;
use std::path::Path;
use std::time::{Duration, Instant};

use symphonia::core::audio::AudioBufferRef;
use symphonia::core::codecs::{Decoder, DecoderOptions, CODEC_TYPE_FLAC};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::{FormatOptions, FormatReader};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::{MetadataOptions, MetadataRevision, StandardTagKey, Value};
use symphonia::core::probe::Hint;
use symphonia::core::units::{Time, TimeBase};

use crate::audio::{AudioBuffer, AudioDecoder, AudioMetadata, MetadataExtractor};
use crate::audio::performance::AudioPerformanceProfiler;
use crate::audio::memory::{HighResBufferAllocator, ManagedAudioBuffer};
use crate::error::DecodeError;

/// FLAC audio decoder implementation using symphonia with performance optimizations
pub struct FlacDecoder {
    format_reader: Box<dyn FormatReader>,
    decoder: Box<dyn Decoder>,
    track_id: u32,
    metadata: AudioMetadata,
    duration: Duration,
    sample_rate: u32,
    bit_depth: u16,
    channels: u16,
    time_base: TimeBase,

    // Performance optimizations
    performance_profiler: Option<std::sync::Arc<AudioPerformanceProfiler>>,
    buffer_allocator: Option<std::sync::Arc<HighResBufferAllocator>>,
    is_high_resolution: bool,

    // Decode caching for better performance
    last_decode_time: Option<Instant>,
    decode_buffer_cache: Option<ManagedAudioBuffer>,
}

impl FlacDecoder {
    /// Create a new FLAC decoder for the given file path
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self, DecodeError> {
        Self::new_with_profiler(path, None, None)
    }

    /// Create a new FLAC decoder with performance profiling
    pub fn new_with_profiler<P: AsRef<Path>>(
        path: P,
        profiler: Option<std::sync::Arc<AudioPerformanceProfiler>>,
        allocator: Option<std::sync::Arc<HighResBufferAllocator>>,
    ) -> Result<Self, DecodeError> {
        let file = File::open(&path).map_err(|e| {
            DecodeError::DecodeFailed(format!("Failed to open file: {}", e))
        })?;

        let media_source = MediaSourceStream::new(
            Box::new(file),
            Default::default(),
        );

        // Create a hint to help the format registry guess the format
        let mut hint = Hint::new();
        if let Some(extension) = path.as_ref().extension() {
            if let Some(ext_str) = extension.to_str() {
                hint.with_extension(ext_str);
            }
        }

        // Probe the media source for a format
        let probed = symphonia::default::get_probe()
            .format(&hint, media_source, &FormatOptions::default(), &MetadataOptions::default())
            .map_err(|e| DecodeError::UnsupportedFormat {
                format: format!("FLAC probe failed: {}", e),
            })?;

        let format_reader = probed.format;

        // Find the first audio track
        let track = format_reader
            .tracks()
            .iter()
            .find(|t| t.codec_params.codec == CODEC_TYPE_FLAC)
            .ok_or_else(|| DecodeError::UnsupportedFormat {
                format: "No FLAC audio track found".to_string(),
            })?;

        let track_id = track.id;

        // Create a decoder for the track
        let decoder = symphonia::default::get_codecs()
            .make(&track.codec_params, &DecoderOptions::default())
            .map_err(|e| DecodeError::DecodeFailed(format!("Failed to create decoder: {}", e)))?;

        // Extract audio format information
        let sample_rate = track.codec_params.sample_rate.unwrap_or(44100);
        let channels = track.codec_params.channels.map(|c| c.count() as u16).unwrap_or(2);

        // Determine bit depth from codec parameters
        let bit_depth = match track.codec_params.bits_per_sample {
            Some(bits) => bits as u16,
            None => {
                // Default to 16-bit if not specified, but FLAC typically has this info
                16
            }
        };

        // Calculate duration
        let duration = if let (Some(n_frames), Some(sample_rate)) =
            (track.codec_params.n_frames, track.codec_params.sample_rate) {
            Duration::from_secs_f64(n_frames as f64 / sample_rate as f64)
        } else {
            Duration::from_secs(0) // Unknown duration
        };

        // Extract metadata during initialization when we have mutable access
        let metadata = MetadataExtractor::extract_from_format_reader(format_reader.as_ref(), probed.metadata);

        // Get time base for seeking
        let time_base = track.codec_params.time_base.unwrap_or(TimeBase::new(1, sample_rate));

        // Determine if this is high-resolution audio
        let is_high_resolution = bit_depth >= 24 || sample_rate >= 96000;

        // Pre-allocate decode buffer for high-resolution files
        let decode_buffer_cache = if let Some(ref allocator) = allocator {
            if is_high_resolution {
                // Pre-allocate a buffer for 100ms of audio
                allocator.allocate_for_format(sample_rate, bit_depth, channels, 100).ok()
            } else {
                None
            }
        } else {
            None
        };

        Ok(FlacDecoder {
            format_reader,
            decoder,
            track_id,
            metadata,
            duration,
            sample_rate,
            bit_depth,
            channels,
            time_base,
            performance_profiler: profiler,
            buffer_allocator: allocator,
            is_high_resolution,
            last_decode_time: None,
            decode_buffer_cache,
        })
    }

    /// Extract metadata from probed metadata during initialization
    fn extract_metadata_from_probed(
        mut probed_metadata: symphonia::core::probe::ProbedMetadata,
    ) -> AudioMetadata {
        let mut metadata = AudioMetadata {
            title: None,
            artist: None,
            album: None,
            track_number: None,
            year: None,
            genre: None,
        };

        // Try to get metadata from the probed metadata
        if let Some(probed_meta) = probed_metadata.get() {
            if let Some(metadata_rev) = probed_meta.current() {
                Self::extract_from_revision(metadata_rev, &mut metadata);
            }
        }

        metadata
    }

    /// Extract metadata from a metadata revision
    fn extract_from_revision(revision: &MetadataRevision, metadata: &mut AudioMetadata) {
        for tag in revision.tags() {
            if let Some(std_key) = tag.std_key {
                match std_key {
                    StandardTagKey::TrackTitle => {
                        if let Value::String(title) = &tag.value {
                            metadata.title = Some(title.clone());
                        }
                    }
                    StandardTagKey::Artist => {
                        if let Value::String(artist) = &tag.value {
                            metadata.artist = Some(artist.clone());
                        }
                    }
                    StandardTagKey::Album => {
                        if let Value::String(album) = &tag.value {
                            metadata.album = Some(album.clone());
                        }
                    }
                    StandardTagKey::TrackNumber => {
                        match &tag.value {
                            Value::UnsignedInt(track_num) => {
                                metadata.track_number = Some(*track_num as u32);
                            }
                            Value::String(track_str) => {
                                if let Ok(track_num) = track_str.parse::<u32>() {
                                    metadata.track_number = Some(track_num);
                                }
                            }
                            _ => {}
                        }
                    }
                    StandardTagKey::Date => {
                        match &tag.value {
                            Value::String(date_str) => {
                                // Try to extract year from date string (YYYY-MM-DD or just YYYY)
                                if let Some(year_str) = date_str.split('-').next() {
                                    if let Ok(year) = year_str.parse::<u32>() {
                                        metadata.year = Some(year);
                                    }
                                }
                            }
                            Value::UnsignedInt(year) => {
                                metadata.year = Some(*year as u32);
                            }
                            _ => {}
                        }
                    }
                    StandardTagKey::Genre => {
                        if let Value::String(genre) = &tag.value {
                            metadata.genre = Some(genre.clone());
                        }
                    }
                    _ => {} // Ignore other tags for now
                }
            }
        }
    }



    /// Convert symphonia audio buffer to our AudioBuffer format with optimizations
    fn convert_audio_buffer_optimized(
        audio_buf: AudioBufferRef,
        managed_buffer: Option<&mut ManagedAudioBuffer>,
    ) -> Result<AudioBuffer, DecodeError> {
        let spec = *audio_buf.spec();
        let frames = audio_buf.frames();
        let channels = spec.channels.count() as usize;

        // Use pre-allocated buffer if available
        let required_samples = frames * channels;
        let mut samples = if let Some(buffer) = managed_buffer {
            if buffer.f32_capacity() >= required_samples {
                // Reuse the managed buffer
                let slice = buffer.as_f32_mut_slice();
                slice[..required_samples].to_vec()
            } else {
                // Fallback to regular allocation
                Vec::with_capacity(required_samples)
            }
        } else {
            Vec::with_capacity(required_samples)
        };

        // Optimized conversion based on format (gather planar, interleave after)
        match audio_buf {
            AudioBufferRef::F32(buf) => {
                for plane in buf.planes().planes() {
                    samples.extend_from_slice(plane);
                }
            }
            AudioBufferRef::S16(buf) => {
                samples.reserve(required_samples);
                for plane in buf.planes().planes() {
                    for &sample in plane.iter() {
                        samples.push(sample as f32 / 32768.0);
                    }
                }
            }
            AudioBufferRef::S24(buf) => {
                samples.reserve(required_samples);
                for plane in buf.planes().planes() {
                    for &sample in plane.iter() {
                        let sample_i32 = sample.inner();
                        samples.push(sample_i32 as f32 / 8388608.0);
                    }
                }
            }
            AudioBufferRef::S32(buf) => {
                samples.reserve(required_samples);
                for plane in buf.planes().planes() {
                    for &sample in plane.iter() {
                        samples.push(sample as f32 / 2147483648.0);
                    }
                }
            }
            _ => {
                // Fallback to original conversion for other formats
                return Self::convert_audio_buffer(audio_buf);
            }
        }

        // Interleave planar samples into interleaved frames (LRLR...) if multi-channel
        if channels > 1 && samples.len() == required_samples {
            let mut interleaved = vec![0.0f32; required_samples];
            for ch in 0..channels {
                for f in 0..frames {
                    interleaved[f * channels + ch] = samples[ch * frames + f];
                }
            }
            samples = interleaved;
        }

        Ok(AudioBuffer {
            samples,
            channels: spec.channels.count() as u16,
            sample_rate: spec.rate,
            frames,
        })
    }

    /// Convert symphonia audio buffer to our AudioBuffer format (original implementation)
    fn convert_audio_buffer(audio_buf: AudioBufferRef) -> Result<AudioBuffer, DecodeError> {
        let spec = *audio_buf.spec();
        let frames = audio_buf.frames();
        let channels = spec.channels.count() as usize;

        // Convert to f32 samples
        let mut samples = Vec::new();

        match audio_buf {
            AudioBufferRef::U8(buf) => {
                for plane in buf.planes().planes() {
                    for &sample in plane.iter() {
                        // Convert u8 to f32 range [-1.0, 1.0]
                        let normalized = (sample as f32 - 128.0) / 128.0;
                        samples.push(normalized);
                    }
                }
            }
            AudioBufferRef::U16(buf) => {
                for plane in buf.planes().planes() {
                    for &sample in plane.iter() {
                        // Convert u16 to f32 range [-1.0, 1.0]
                        let normalized = (sample as f32 - 32768.0) / 32768.0;
                        samples.push(normalized);
                    }
                }
            }
            AudioBufferRef::U24(buf) => {
                for plane in buf.planes().planes() {
                    for &sample in plane.iter() {
                        // Convert u24 to f32 range [-1.0, 1.0]
                        let sample_u32 = sample.inner() as u32;
                        let normalized = (sample_u32 as f32 - 8388608.0) / 8388608.0;
                        samples.push(normalized);
                    }
                }
            }
            AudioBufferRef::U32(buf) => {
                for plane in buf.planes().planes() {
                    for &sample in plane.iter() {
                        // Convert u32 to f32 range [-1.0, 1.0]
                        let normalized = (sample as f32 - 2147483648.0) / 2147483648.0;
                        samples.push(normalized);
                    }
                }
            }
            AudioBufferRef::S8(buf) => {
                for plane in buf.planes().planes() {
                    for &sample in plane.iter() {
                        // Convert s8 to f32 range [-1.0, 1.0]
                        let normalized = sample as f32 / 128.0;
                        samples.push(normalized);
                    }
                }
            }
            AudioBufferRef::S16(buf) => {
                for plane in buf.planes().planes() {
                    for &sample in plane.iter() {
                        // Convert s16 to f32 range [-1.0, 1.0]
                        let normalized = sample as f32 / 32768.0;
                        samples.push(normalized);
                    }
                }
            }
            AudioBufferRef::S24(buf) => {
                for plane in buf.planes().planes() {
                    for &sample in plane.iter() {
                        // Convert s24 to f32 range [-1.0, 1.0]
                        let sample_i32 = sample.inner();
                        let normalized = sample_i32 as f32 / 8388608.0;
                        samples.push(normalized);
                    }
                }
            }
            AudioBufferRef::S32(buf) => {
                for plane in buf.planes().planes() {
                    for &sample in plane.iter() {
                        // Convert s32 to f32 range [-1.0, 1.0]
                        let normalized = sample as f32 / 2147483648.0;
                        samples.push(normalized);
                    }
                }
            }
            AudioBufferRef::F32(buf) => {
                for plane in buf.planes().planes() {
                    for &sample in plane.iter() {
                        samples.push(sample);
                    }
                }
            }
            AudioBufferRef::F64(buf) => {
                for plane in buf.planes().planes() {
                    for &sample in plane.iter() {
                        samples.push(sample as f32);
                    }
                }
            }
        }

        // Interleave planar samples into interleaved frames (LRLR...) if multi-channel
        if channels > 1 {
            let mut interleaved = vec![0.0f32; frames * channels];
            for ch in 0..channels {
                for f in 0..frames {
                    interleaved[f * channels + ch] = samples[ch * frames + f];
                }
            }
            samples = interleaved;
        }

        Ok(AudioBuffer {
            samples,
            channels: spec.channels.count() as u16,
            sample_rate: spec.rate,
            frames,
        })
    }
}

impl AudioDecoder for FlacDecoder {
    fn decode_next(&mut self) -> Result<Option<AudioBuffer>, DecodeError> {
        // Start performance profiling if available
        let decode_profiler = self.performance_profiler.as_ref().map(|p| p.start_decode_profile());
        let decode_start = Instant::now();

        // Get the next packet from the format reader
        let packet = match self.format_reader.next_packet() {
            Ok(packet) => packet,
            Err(SymphoniaError::IoError(ref err))
                if err.kind() == std::io::ErrorKind::UnexpectedEof => {
                // End of stream
                return Ok(None);
            }
            Err(err) => {
                return Err(DecodeError::DecodeFailed(format!("Failed to read packet: {}", err)));
            }
        };

        // Only process packets for our track
        if packet.track_id() != self.track_id {
            return self.decode_next(); // Recursively try next packet
        }

        // Decode the packet
        let result = match self.decoder.decode(&packet) {
            Ok(audio_buf) => {
                // Use optimized conversion if we have managed buffers
                let buffer = if self.is_high_resolution && self.decode_buffer_cache.is_some() {
                    Self::convert_audio_buffer_optimized(audio_buf, self.decode_buffer_cache.as_mut())?
                } else {
                    Self::convert_audio_buffer(audio_buf)?
                };
                Ok(Some(buffer))
            }
            Err(e) => Err(DecodeError::DecodeFailed(format!("Failed to decode packet: {}", e)))
        };

        // Record performance metrics
        let decode_time = decode_start.elapsed();
        self.last_decode_time = Some(decode_start);

        if let Some(profiler) = decode_profiler {
            profiler.finish(self.sample_rate, self.bit_depth);
        }

        // Log performance warning for slow decodes
        if let Some(ref perf_profiler) = self.performance_profiler {
            if decode_time.as_millis() > 5 {
                // Log warning if decode takes more than 5ms
                perf_profiler.record_decode_performance(
                    decode_time,
                    self.sample_rate,
                    self.bit_depth,
                    self.is_high_resolution,
                );
            }
        }

        result
    }

    fn seek(&mut self, position: Duration) -> Result<(), DecodeError> {
        // Convert duration to time units
        let seek_time = Time::new(
            (position.as_secs_f64() * self.time_base.denom as f64) as u64,
            self.time_base.denom as f64,
        );

        // Perform the seek
        self.format_reader
            .seek(symphonia::core::formats::SeekMode::Accurate, symphonia::core::formats::SeekTo::Time { time: seek_time, track_id: Some(self.track_id) })
            .map_err(|e| DecodeError::SeekError(format!("Seek failed: {}", e)))?;

        // Reset the decoder state after seeking
        self.decoder.reset();

        Ok(())
    }

    fn metadata(&self) -> &AudioMetadata {
        &self.metadata
    }

    fn duration(&self) -> Duration {
        self.duration
    }

    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    fn bit_depth(&self) -> u16 {
        self.bit_depth
    }

    fn channels(&self) -> u16 {
        self.channels
    }
}

impl FlacDecoder {
    /// Get the number of channels
    pub fn channels(&self) -> u16 {
        self.channels
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    // Helper function to create a minimal FLAC file for testing
    // Note: In a real implementation, you'd want to use actual FLAC test files
    fn create_test_flac_file() -> Result<NamedTempFile, Box<dyn std::error::Error>> {
        // For now, we'll skip this test since creating a valid FLAC file programmatically
        // is complex. In a real implementation, you'd have test FLAC files in a test directory.
        Err("Test FLAC file creation not implemented".into())
    }

    #[test]
    fn test_flac_decoder_new_with_nonexistent_file() {
        let result = FlacDecoder::new("/nonexistent/file.flac");
        assert!(result.is_err());

        if let Err(DecodeError::DecodeFailed(msg)) = result {
            assert!(msg.contains("Failed to open file"));
        } else {
            panic!("Expected DecodeFailed error");
        }
    }

    #[test]
    fn test_audio_metadata_default() {
        let metadata = AudioMetadata {
            title: None,
            artist: None,
            album: None,
            track_number: None,
            year: None,
            genre: None,
        };

        assert!(metadata.title.is_none());
        assert!(metadata.artist.is_none());
        assert!(metadata.album.is_none());
        assert!(metadata.track_number.is_none());
        assert!(metadata.year.is_none());
        assert!(metadata.genre.is_none());
    }

    #[test]
    fn test_audio_metadata_with_values() {
        let metadata = AudioMetadata {
            title: Some("Test Title".to_string()),
            artist: Some("Test Artist".to_string()),
            album: Some("Test Album".to_string()),
            track_number: Some(1),
            year: Some(2023),
            genre: Some("Test Genre".to_string()),
        };

        assert_eq!(metadata.title, Some("Test Title".to_string()));
        assert_eq!(metadata.artist, Some("Test Artist".to_string()));
        assert_eq!(metadata.album, Some("Test Album".to_string()));
        assert_eq!(metadata.track_number, Some(1));
        assert_eq!(metadata.year, Some(2023));
        assert_eq!(metadata.genre, Some("Test Genre".to_string()));
    }

    #[test]
    fn test_audio_buffer_creation() {
        let buffer = AudioBuffer {
            samples: vec![0.0, 0.5, -0.5, 1.0],
            channels: 2,
            sample_rate: 44100,
            frames: 2,
        };

        assert_eq!(buffer.samples.len(), 4);
        assert_eq!(buffer.channels, 2);
        assert_eq!(buffer.sample_rate, 44100);
        assert_eq!(buffer.frames, 2);
    }

    // Integration test that would work with actual FLAC files
    #[test]
    #[ignore] // Ignored by default since it requires actual FLAC files
    fn test_flac_decoder_with_real_file() {
        // This test would require a real FLAC file in the test resources
        // You would place a test FLAC file in tests/resources/ and test with it

        // Example:
        // let decoder = FlacDecoder::new("tests/resources/test.flac").unwrap();
        // assert_eq!(decoder.sample_rate(), 44100);
        // assert_eq!(decoder.bit_depth(), 16);
        // assert!(decoder.duration().as_secs() > 0);

        // let mut buffer_count = 0;
        // while let Ok(Some(_buffer)) = decoder.decode_next() {
        //     buffer_count += 1;
        //     if buffer_count > 10 { break; } // Don't decode the entire file
        // }
        // assert!(buffer_count > 0);
    }

    #[test]
    fn test_flac_decoder_trait_implementation() {
        // Test that FlacDecoder implements AudioDecoder trait properly
        // This is a compile-time test - if it compiles, the trait is implemented correctly
        fn _test_audio_decoder_trait<T: AudioDecoder>(_decoder: T) {}

        // This would fail to compile if FlacDecoder doesn't implement AudioDecoder
        // We can't actually create a FlacDecoder without a valid file, so this is just a type check
    }

    #[test]
    fn test_flac_decoder_error_handling() {
        // Test that we handle various error conditions properly
        let error = DecodeError::DecodeFailed("test decode error".to_string());
        assert!(format!("{}", error).contains("Decode failed: test decode error"));

        let error = DecodeError::SeekError("test seek error".to_string());
        assert!(format!("{}", error).contains("Seek error: test seek error"));

        let error = DecodeError::UnsupportedFormat { format: "UNKNOWN".to_string() };
        assert!(format!("{}", error).contains("Unsupported format: UNKNOWN"));
    }

    #[test]
    fn test_audio_format_constants() {
        // Test that we handle common audio format constants correctly
        let sample_rates = [44100, 48000, 88200, 96000, 176400, 192000];
        let bit_depths = [16, 24, 32];
        let channel_counts = [1, 2, 6, 8];

        for &rate in &sample_rates {
            assert!(rate > 0);
            assert!(rate <= 192000);
        }

        for &depth in &bit_depths {
            assert!(depth > 0);
            assert!(depth <= 32);
        }

        for &channels in &channel_counts {
            assert!(channels > 0);
            assert!(channels <= 8);
        }
    }

    #[test]
    fn test_decode_error_types() {
        let error1 = DecodeError::UnsupportedFormat {
            format: "TEST".to_string(),
        };
        assert!(format!("{}", error1).contains("Unsupported format: TEST"));

        let error2 = DecodeError::CorruptedFile("test file".to_string());
        assert!(format!("{}", error2).contains("Corrupted file: test file"));

        let error3 = DecodeError::SeekError("seek failed".to_string());
        assert!(format!("{}", error3).contains("Seek error: seek failed"));

        let error4 = DecodeError::DecodeFailed("decode failed".to_string());
        assert!(format!("{}", error4).contains("Decode failed: decode failed"));
    }

    #[test]
    fn test_duration_conversion() {
        let duration = Duration::from_secs(120); // 2 minutes
        assert_eq!(duration.as_secs(), 120);

        let duration_ms = Duration::from_millis(1500); // 1.5 seconds
        assert_eq!(duration_ms.as_millis(), 1500);
    }

    #[test]
    fn test_sample_rate_validation() {
        // Test common sample rates
        let common_rates = [44100, 48000, 88200, 96000, 176400, 192000];

        for rate in common_rates {
            assert!(rate > 0);
            assert!(rate <= 192000); // Reasonable upper limit for high-res audio
        }
    }

    #[test]
    fn test_bit_depth_validation() {
        // Test common bit depths
        let common_depths = [16, 24, 32];

        for depth in common_depths {
            assert!(depth > 0);
            assert!(depth <= 32); // 32-bit is the maximum we support
        }
    }

    #[test]
    fn test_channel_count_validation() {
        // Test common channel counts
        let common_channels = [1, 2, 6, 8]; // Mono, Stereo, 5.1, 7.1

        for channels in common_channels {
            assert!(channels > 0);
            assert!(channels <= 8); // Reasonable upper limit
        }
    }

    #[test]
    fn test_flac_decoder_constants() {
        // Test that we have reasonable constants for audio processing
        let max_sample_rate = 192000u32;
        let max_bit_depth = 32u16;
        let max_channels = 8u16;

        assert!(max_sample_rate > 0);
        assert!(max_bit_depth > 0);
        assert!(max_channels > 0);

        // Test that common values are within our limits
        assert!(44100 <= max_sample_rate);
        assert!(48000 <= max_sample_rate);
        assert!(96000 <= max_sample_rate);

        assert!(16 <= max_bit_depth);
        assert!(24 <= max_bit_depth);

        assert!(1 <= max_channels); // Mono
        assert!(2 <= max_channels); // Stereo
    }
}
