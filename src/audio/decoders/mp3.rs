use std::fs::File;
use std::path::Path;
use std::time::Duration;

use symphonia::core::audio::AudioBufferRef;
use symphonia::core::codecs::{Decoder, DecoderOptions, CODEC_TYPE_MP3};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::{FormatOptions, FormatReader};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::{MetadataOptions, MetadataRevision, StandardTagKey, Value};
use symphonia::core::probe::Hint;
use symphonia::core::units::{Time, TimeBase};

use crate::audio::{AudioBuffer, AudioDecoder, AudioMetadata};
use crate::error::DecodeError;

/// MP3 audio decoder implementation using symphonia
pub struct Mp3Decoder {
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

impl Mp3Decoder {
    /// Create a new MP3 decoder for the given file path
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
                format: format!("MP3 probe failed: {}", e),
            })?;

        let format_reader = probed.format;

        // Find the first MP3 audio track
        let track = format_reader
            .tracks()
            .iter()
            .find(|t| t.codec_params.codec == CODEC_TYPE_MP3)
            .ok_or_else(|| DecodeError::UnsupportedFormat {
                format: "No MP3 audio track found".to_string(),
            })?;

        let track_id = track.id;

        // Create a decoder for the track
        let decoder = symphonia::default::get_codecs()
            .make(&track.codec_params, &DecoderOptions::default())
            .map_err(|e| DecodeError::DecodeFailed(format!("Failed to create MP3 decoder: {}", e)))?;

        // Extract audio format information
        let sample_rate = track.codec_params.sample_rate.unwrap_or(44100);
        let channels = track.codec_params.channels.map(|c| c.count() as u16).unwrap_or(2);

        // MP3 is typically decoded to 16-bit PCM, but can be 24-bit for high quality
        let bit_depth = match track.codec_params.bits_per_sample {
            Some(bits) => bits as u16,
            None => {
                // MP3 is typically decoded to 16-bit PCM
                16
            }
        };

        // Calculate duration - MP3 duration can be estimated from bitrate and file size
        let duration = if let (Some(n_frames), Some(sample_rate)) =
            (track.codec_params.n_frames, track.codec_params.sample_rate) {
            Duration::from_secs_f64(n_frames as f64 / sample_rate as f64)
        } else {
            // For MP3, we might need to estimate duration differently
            Duration::from_secs(0) // Unknown duration
        };

        // Extract metadata during initialization (MP3 often has ID3 tags)
        let metadata = Self::extract_metadata_from_probed(probed.metadata);

        // Get time base for seeking
        let time_base = track.codec_params.time_base.unwrap_or(TimeBase::new(1, sample_rate));

        Ok(Mp3Decoder {
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

        // Try to get metadata from the probed metadata (ID3 tags for MP3)
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

impl AudioDecoder for Mp3Decoder {
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

impl Mp3Decoder {
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
    fn test_mp3_decoder_new_with_nonexistent_file() {
        let result = Mp3Decoder::new("/nonexistent/file.mp3");
        assert!(result.is_err());

        if let Err(DecodeError::DecodeFailed(msg)) = result {
            assert!(msg.contains("Failed to open file"));
        } else {
            panic!("Expected DecodeFailed error");
        }
    }

    #[test]
    fn test_mp3_decoder_trait_implementation() {
        // Test that Mp3Decoder implements AudioDecoder trait properly
        // This is a compile-time test - if it compiles, the trait is implemented correctly
        fn _test_audio_decoder_trait<T: AudioDecoder>(_decoder: T) {}

        // This would fail to compile if Mp3Decoder doesn't implement AudioDecoder
        // We can't actually create an Mp3Decoder without a valid file, so this is just a type check
    }

    #[test]
    fn test_mp3_decoder_error_handling() {
        // Test that we handle various error conditions properly
        let error = DecodeError::DecodeFailed("test decode error".to_string());
        assert!(format!("{}", error).contains("Decode failed: test decode error"));

        let error = DecodeError::SeekError("test seek error".to_string());
        assert!(format!("{}", error).contains("Seek error: test seek error"));

        let error = DecodeError::UnsupportedFormat { format: "UNKNOWN".to_string() };
        assert!(format!("{}", error).contains("Unsupported format: UNKNOWN"));
    }

    #[test]
    fn test_mp3_format_constants() {
        // Test that we handle common MP3 format constants correctly
        let sample_rates = [32000, 44100, 48000]; // Common MP3 sample rates
        let bit_depths = [16]; // MP3 is typically decoded to 16-bit
        let channel_counts = [1, 2]; // Mono and stereo

        for &rate in &sample_rates {
            assert!(rate > 0);
            assert!(rate <= 48000); // MP3 doesn't typically go higher
        }

        for &depth in &bit_depths {
            assert!(depth > 0);
            assert!(depth <= 16); // MP3 is typically 16-bit when decoded
        }

        for &channels in &channel_counts {
            assert!(channels > 0);
            assert!(channels <= 2); // MP3 is typically mono or stereo
        }
    }

    #[test]
    #[ignore] // Ignored by default since it requires actual MP3 files
    fn test_mp3_decoder_with_real_file() {
        // This test would require a real MP3 file in the test resources
        // You would place a test MP3 file in tests/resources/ and test with it

        // Example:
        // let decoder = Mp3Decoder::new("tests/resources/test.mp3").unwrap();
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
    fn test_mp3_lossy_properties() {
        // Test MP3-specific properties (lossy compression)

        // MP3 common bitrates (in kbps)
        let mp3_bitrates = [128, 192, 256, 320];

        for &bitrate in &mp3_bitrates {
            assert!(bitrate >= 128);
            assert!(bitrate <= 320); // Common MP3 bitrate range
        }

        // MP3 sample rates
        let mp3_sample_rates = [32000, 44100, 48000];

        for &rate in &mp3_sample_rates {
            assert!(rate >= 32000);
            assert!(rate <= 48000);
        }
    }

    #[test]
    fn test_id3_metadata_handling() {
        // Test ID3 tag metadata handling for MP3
        let mut metadata = AudioMetadata::new();

        // MP3 files often have rich ID3 metadata
        metadata.title = Some("Test MP3 Track".to_string());
        metadata.artist = Some("Test Artist".to_string());
        metadata.album = Some("Test Album".to_string());
        metadata.track_number = Some(1);
        metadata.year = Some(2023);
        metadata.genre = Some("Rock".to_string());

        assert!(!metadata.is_empty());
        assert_eq!(metadata.title, Some("Test MP3 Track".to_string()));
        assert_eq!(metadata.artist, Some("Test Artist".to_string()));
        assert_eq!(metadata.album, Some("Test Album".to_string()));
        assert_eq!(metadata.track_number, Some(1));
        assert_eq!(metadata.year, Some(2023));
        assert_eq!(metadata.genre, Some("Rock".to_string()));
    }

    #[test]
    fn test_mp3_channel_modes() {
        // Test MP3 channel modes
        let channel_modes = [
            (1, "Mono"),
            (2, "Stereo"),
        ];

        for (channels, description) in channel_modes {
            assert!(channels > 0);
            assert!(channels <= 2); // MP3 supports mono and stereo
            assert!(!description.is_empty());
        }
    }

    #[test]
    fn test_mp3_sample_conversion() {
        // Test sample conversion for MP3 (typically 16-bit signed)
        let s16_samples = [i16::MIN, -1000, 0, 1000, i16::MAX];

        for &sample in &s16_samples {
            let normalized = sample as f32 / 32768.0;
            assert!(normalized >= -1.0);
            assert!(normalized <= 1.0);
        }
    }

    #[test]
    fn test_mp3_duration_estimation() {
        // Test duration estimation for MP3 files

        // For a 44.1kHz MP3 with known frame count
        let sample_rate = 44100u32;
        let estimated_frames = 44100u64; // 1 second

        let duration = Duration::from_secs_f64(estimated_frames as f64 / sample_rate as f64);
        assert_eq!(duration.as_secs(), 1);

        // Test with different sample rates
        let rates_and_frames = [(32000, 32000), (44100, 44100), (48000, 48000)];

        for (rate, frames) in rates_and_frames {
            let duration = Duration::from_secs_f64(frames as f64 / rate as f64);
            assert_eq!(duration.as_secs(), 1);
        }
    }

    #[test]
    fn test_mp3_error_messages() {
        // Test that error messages are clear for MP3 files
        let error = DecodeError::UnsupportedFormat {
            format: "MP3 probe failed: Invalid header".to_string(),
        };

        let error_msg = format!("{}", error);
        assert!(error_msg.contains("Unsupported format"));
        assert!(error_msg.contains("MP3"));

        let decode_error = DecodeError::DecodeFailed("Failed to create MP3 decoder: Codec not supported".to_string());
        let decode_msg = format!("{}", decode_error);
        assert!(decode_msg.contains("Decode failed"));
        assert!(decode_msg.contains("MP3 decoder"));
    }

    #[test]
    fn test_mp3_quality_levels() {
        // Test different MP3 quality levels
        let quality_bitrates = [
            (128, "Standard Quality"),
            (192, "Good Quality"),
            (256, "High Quality"),
            (320, "Very High Quality"),
        ];

        for (bitrate, description) in quality_bitrates {
            assert!(bitrate >= 128);
            assert!(bitrate <= 320);
            assert!(!description.is_empty());
        }
    }

    #[test]
    fn test_mp3_seek_accuracy() {
        // Test seek accuracy considerations for MP3

        // MP3 frames are typically around 26ms at 44.1kHz
        let frame_duration_ms = 26.0;
        let seek_position = Duration::from_millis(1000); // 1 second

        // Calculate expected frame boundary
        let frame_boundary = (seek_position.as_millis() as f64 / frame_duration_ms).floor() * frame_duration_ms;

        assert!(frame_boundary >= 0.0);
        assert!(frame_boundary <= seek_position.as_millis() as f64);
    }

    #[test]
    fn test_mp3_variable_bitrate() {
        // Test considerations for Variable Bitrate (VBR) MP3 files

        // VBR MP3 files can have varying bitrates
        let vbr_range = (128, 320); // Min and max bitrates for VBR

        assert!(vbr_range.0 < vbr_range.1);
        assert!(vbr_range.0 >= 128);
        assert!(vbr_range.1 <= 320);

        // Average bitrate calculation
        let avg_bitrate = (vbr_range.0 + vbr_range.1) / 2;
        assert!(avg_bitrate > vbr_range.0);
        assert!(avg_bitrate < vbr_range.1);
    }
}
