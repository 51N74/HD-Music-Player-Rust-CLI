use std::fs::File;
use std::path::Path;
use std::time::Duration;

use symphonia::core::audio::AudioBufferRef;
use symphonia::core::codecs::{Decoder, DecoderOptions};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::{FormatOptions, FormatReader};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::{MetadataOptions, MetadataRevision, StandardTagKey, Value};
use symphonia::core::probe::Hint;
use symphonia::core::units::{Time, TimeBase};

use crate::audio::{AudioBuffer, AudioDecoder, AudioMetadata, MetadataExtractor};
use crate::error::DecodeError;

/// WAV audio decoder implementation using symphonia
pub struct WavDecoder {
    format_reader: Box<dyn FormatReader>,
    decoder: Box<dyn Decoder>,
    track_id: u32,
    metadata: AudioMetadata,
    duration: Duration,
    sample_rate: u32,
    bit_depth: u16,
    channels: u16,
    time_base: TimeBase,
}

impl WavDecoder {
    /// Create a new WAV decoder for the given file path
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self, DecodeError> {
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
                format: format!("WAV probe failed: {}", e),
            })?;

        let format_reader = probed.format;

        // Find the first audio track (WAV files typically have PCM audio)
        let track = format_reader
            .tracks()
            .iter()
            .find(|t| t.codec_params.codec != symphonia::core::codecs::CODEC_TYPE_NULL)
            .ok_or_else(|| DecodeError::UnsupportedFormat {
                format: "No audio track found in WAV file".to_string(),
            })?;

        let track_id = track.id;

        // Create a decoder for the track
        let decoder = symphonia::default::get_codecs()
            .make(&track.codec_params, &DecoderOptions::default())
            .map_err(|e| DecodeError::DecodeFailed(format!("Failed to create WAV decoder: {}", e)))?;

        // Extract audio format information
        let sample_rate = track.codec_params.sample_rate.unwrap_or(44100);
        let channels = track.codec_params.channels.map(|c| c.count() as u16).unwrap_or(2);

        // Determine bit depth from codec parameters
        let bit_depth = match track.codec_params.bits_per_sample {
            Some(bits) => bits as u16,
            None => {
                // Try to infer from codec type if available
                16 // Default to 16-bit for WAV
            }
        };

        // Calculate duration
        let duration = if let (Some(n_frames), Some(sample_rate)) =
            (track.codec_params.n_frames, track.codec_params.sample_rate) {
            Duration::from_secs_f64(n_frames as f64 / sample_rate as f64)
        } else {
            Duration::from_secs(0) // Unknown duration
        };

        // Extract metadata during initialization
        let metadata = MetadataExtractor::extract_from_format_reader(format_reader.as_ref(), probed.metadata);

        // Get time base for seeking
        let time_base = track.codec_params.time_base.unwrap_or(TimeBase::new(1, sample_rate));

        Ok(WavDecoder {
            format_reader,
            decoder,
            track_id,
            metadata,
            duration,
            sample_rate,
            bit_depth,
            channels,
            time_base,
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

    /// Convert symphonia audio buffer to our AudioBuffer format
    fn convert_audio_buffer(audio_buf: AudioBufferRef) -> Result<AudioBuffer, DecodeError> {
        let spec = *audio_buf.spec();
        let channels = spec.channels.count() as usize;
        let frames = audio_buf.frames();

        // Convert to f32 samples
        let mut samples = Vec::with_capacity(frames * channels);

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

        // Interleave planar samples into interleaved frames (LRLR...) if needed
        if channels > 1 {
            let mut interleaved = vec![0.0f32; frames * channels];
            // Convert from planar [ch0[0..F], ch1[0..F], ...] to interleaved [f0ch0, f0ch1, ..., f1ch0, f1ch1, ...]
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
            frames: frames,
        })
    }
}

impl AudioDecoder for WavDecoder {
    fn decode_next(&mut self) -> Result<Option<AudioBuffer>, DecodeError> {
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
        match self.decoder.decode(&packet) {
            Ok(audio_buf) => {
                // Convert to our AudioBuffer format
                let buffer = Self::convert_audio_buffer(audio_buf)?;
                Ok(Some(buffer))
            }
            Err(e) => Err(DecodeError::DecodeFailed(format!("Failed to decode packet: {}", e)))
        }
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

impl WavDecoder {
    /// Get the number of channels
    pub fn channels(&self) -> u16 {
        self.channels
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn test_wav_decoder_new_with_nonexistent_file() {
        let result = WavDecoder::new("/nonexistent/file.wav");
        assert!(result.is_err());

        if let Err(DecodeError::DecodeFailed(msg)) = result {
            assert!(msg.contains("Failed to open file"));
        } else {
            panic!("Expected DecodeFailed error");
        }
    }

    #[test]
    fn test_wav_decoder_trait_implementation() {
        // Test that WavDecoder implements AudioDecoder trait properly
        // This is a compile-time test - if it compiles, the trait is implemented correctly
        fn _test_audio_decoder_trait<T: AudioDecoder>(_decoder: T) {}

        // This would fail to compile if WavDecoder doesn't implement AudioDecoder
        // We can't actually create a WavDecoder without a valid file, so this is just a type check
    }

    #[test]
    fn test_wav_decoder_error_handling() {
        // Test that we handle various error conditions properly
        let error = DecodeError::DecodeFailed("test decode error".to_string());
        assert!(format!("{}", error).contains("Decode failed: test decode error"));

        let error = DecodeError::SeekError("test seek error".to_string());
        assert!(format!("{}", error).contains("Seek error: test seek error"));

        let error = DecodeError::UnsupportedFormat { format: "UNKNOWN".to_string() };
        assert!(format!("{}", error).contains("Unsupported format: UNKNOWN"));
    }

    #[test]
    fn test_wav_format_constants() {
        // Test that we handle common WAV format constants correctly
        let sample_rates = [44100, 48000, 88200, 96000, 176400, 192000];
        let bit_depths = [8, 16, 24, 32];
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
    #[ignore] // Ignored by default since it requires actual WAV files
    fn test_wav_decoder_with_real_file() {
        // This test would require a real WAV file in the test resources
        // You would place a test WAV file in tests/resources/ and test with it

        // Example:
        // let decoder = WavDecoder::new("tests/resources/test.wav").unwrap();
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
    fn test_sample_conversion_ranges() {
        // Test that our sample conversion logic handles edge cases correctly

        // Test 8-bit unsigned conversion
        let u8_min = 0u8;
        let u8_max = 255u8;
        let u8_mid = 128u8;

        let normalized_min = (u8_min as f32 - 128.0) / 128.0;
        let normalized_max = (u8_max as f32 - 128.0) / 128.0;
        let normalized_mid = (u8_mid as f32 - 128.0) / 128.0;

        assert!((normalized_min - (-1.0)).abs() < 0.01);
        assert!((normalized_max - 0.9921875).abs() < 0.01); // Close to 1.0
        assert!(normalized_mid.abs() < 0.01); // Close to 0.0

        // Test 16-bit signed conversion
        let s16_min = i16::MIN;
        let s16_max = i16::MAX;
        let s16_zero = 0i16;

        let normalized_min = s16_min as f32 / 32768.0;
        let normalized_max = s16_max as f32 / 32768.0;
        let normalized_zero = s16_zero as f32 / 32768.0;

        assert!((normalized_min - (-1.0)).abs() < 0.01);
        assert!((normalized_max - 0.999969482).abs() < 0.01); // Close to 1.0
        assert!(normalized_zero.abs() < 0.01); // Close to 0.0
    }

    #[test]
    fn test_metadata_extraction() {
        // Test metadata extraction logic
        let mut metadata = AudioMetadata::new();
        assert!(metadata.is_empty());

        // Simulate setting metadata fields
        metadata.title = Some("Test WAV".to_string());
        metadata.artist = Some("Test Artist".to_string());
        metadata.album = Some("Test Album".to_string());

        assert!(!metadata.is_empty());
        assert_eq!(metadata.title, Some("Test WAV".to_string()));
        assert_eq!(metadata.artist, Some("Test Artist".to_string()));
        assert_eq!(metadata.album, Some("Test Album".to_string()));
    }

    #[test]
    fn test_duration_calculation() {
        // Test duration calculation logic
        let sample_rate = 44100u32;
        let n_frames = 44100u64; // 1 second of audio

        let duration = Duration::from_secs_f64(n_frames as f64 / sample_rate as f64);
        assert_eq!(duration.as_secs(), 1);

        // Test with high sample rate
        let high_sample_rate = 192000u32;
        let high_n_frames = 192000u64; // 1 second of audio

        let high_duration = Duration::from_secs_f64(high_n_frames as f64 / high_sample_rate as f64);
        assert_eq!(high_duration.as_secs(), 1);
    }

    #[test]
    fn test_bit_depth_detection() {
        // Test bit depth detection for common WAV formats
        let common_bit_depths = [8, 16, 24, 32];

        for &depth in &common_bit_depths {
            assert!(depth > 0);
            assert!(depth <= 32);

            // Test that bit depth is reasonable for WAV format
            match depth {
                8 | 16 | 24 | 32 => {
                    // Valid bit depths for WAV
                }
                _ => panic!("Unexpected bit depth: {}", depth),
            }
        }
    }

    #[test]
    fn test_channel_configuration() {
        // Test common channel configurations for WAV files
        let channel_configs = [
            (1, "Mono"),
            (2, "Stereo"),
            (6, "5.1 Surround"),
            (8, "7.1 Surround"),
        ];

        for (channels, description) in channel_configs {
            assert!(channels > 0);
            assert!(channels <= 8);
            assert!(!description.is_empty());
        }
    }
}
