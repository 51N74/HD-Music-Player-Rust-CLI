use std::path::Path;
use std::time::Duration;

use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::{MetadataOptions, MetadataRevision, StandardTagKey, Value};
use symphonia::core::probe::Hint;

use crate::error::DecodeError;
use crate::models::{AudioMetadata, AudioFormat, AudioCodec};

/// Metadata extractor for audio files using symphonia
pub struct MetadataExtractor;

impl MetadataExtractor {
    /// Extract metadata from an audio file
    pub fn extract_from_file<P: AsRef<Path>>(path: P) -> Result<(AudioMetadata, AudioFormat, Duration), DecodeError> {
        let file = std::fs::File::open(&path).map_err(|e| {
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
                format: format!("Probe failed: {}", e),
            })?;

        let format_reader = probed.format;

        // Find the first audio track
        let track = format_reader
            .tracks()
            .iter()
            .find(|t| t.codec_params.codec != symphonia::core::codecs::CODEC_TYPE_NULL)
            .ok_or_else(|| DecodeError::UnsupportedFormat {
                format: "No audio track found".to_string(),
            })?;

        // Extract audio format information
        let sample_rate = track.codec_params.sample_rate.unwrap_or(44100);
        let channels = track.codec_params.channels.map(|c| c.count() as u16).unwrap_or(2);
        let bit_depth = track.codec_params.bits_per_sample.map(|b| b as u16).unwrap_or(16);

        // Determine codec from file extension and codec type
        let codec = Self::determine_codec(&path, track.codec_params.codec)?;

        let audio_format = AudioFormat::new(sample_rate, bit_depth, channels, codec);

        // Calculate duration
        let duration = if let (Some(n_frames), Some(sample_rate)) = 
            (track.codec_params.n_frames, track.codec_params.sample_rate) {
            Duration::from_secs_f64(n_frames as f64 / sample_rate as f64)
        } else {
            Duration::from_secs(0) // Unknown duration
        };

        // Extract metadata
        let metadata = Self::extract_metadata_from_probed(probed.metadata);

        Ok((metadata, audio_format, duration))
    }

    /// Extract metadata from probed metadata
    fn extract_metadata_from_probed(
        mut probed_metadata: symphonia::core::probe::ProbedMetadata,
    ) -> AudioMetadata {
        let mut metadata = AudioMetadata::new();

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
                                // Handle "1/12" format or just "1"
                                let track_part = track_str.split('/').next().unwrap_or(track_str);
                                if let Ok(track_num) = track_part.parse::<u32>() {
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
                    // Handle additional common tags
                    StandardTagKey::AlbumArtist => {
                        if let Value::String(album_artist) = &tag.value {
                            // If no artist is set, use album artist
                            if metadata.artist.is_none() {
                                metadata.artist = Some(album_artist.clone());
                            }
                        }
                    }
                    _ => {} // Ignore other tags for now
                }
            } else {
                let key = &tag.key;
                // Handle non-standard tags by key name
                match key.to_lowercase().as_str() {
                    "title" | "tit2" => {
                        if let Value::String(title) = &tag.value {
                            metadata.title = Some(title.clone());
                        }
                    }
                    "artist" | "tpe1" => {
                        if let Value::String(artist) = &tag.value {
                            metadata.artist = Some(artist.clone());
                        }
                    }
                    "album" | "talb" => {
                        if let Value::String(album) = &tag.value {
                            metadata.album = Some(album.clone());
                        }
                    }
                    "date" | "tyer" | "tdrc" => {
                        if let Value::String(date_str) = &tag.value {
                            if let Some(year_str) = date_str.split('-').next() {
                                if let Ok(year) = year_str.parse::<u32>() {
                                    metadata.year = Some(year);
                                }
                            }
                        }
                    }
                    "genre" | "tcon" => {
                        if let Value::String(genre) = &tag.value {
                            metadata.genre = Some(genre.clone());
                        }
                    }
                    "tracknumber" | "trck" => {
                        match &tag.value {
                            Value::String(track_str) => {
                                let track_part = track_str.split('/').next().unwrap_or(track_str);
                                if let Ok(track_num) = track_part.parse::<u32>() {
                                    metadata.track_number = Some(track_num);
                                }
                            }
                            Value::UnsignedInt(track_num) => {
                                metadata.track_number = Some(*track_num as u32);
                            }
                            _ => {}
                        }
                    }
                    _ => {} // Ignore other non-standard tags
                }
            }
        }
    }

    /// Determine codec from file path and symphonia codec type
    fn determine_codec<P: AsRef<Path>>(path: P, codec_type: symphonia::core::codecs::CodecType) -> Result<AudioCodec, DecodeError> {
        // First try to determine from file extension
        if let Some(extension) = path.as_ref().extension() {
            if let Some(ext_str) = extension.to_str() {
                match ext_str.to_lowercase().as_str() {
                    "flac" => return Ok(AudioCodec::Flac),
                    "wav" | "wave" => return Ok(AudioCodec::Wav),
                    "m4a" | "alac" => return Ok(AudioCodec::Alac),
                    "mp3" => return Ok(AudioCodec::Mp3),
                    "ogg" | "oga" => return Ok(AudioCodec::OggVorbis),
                    _ => {}
                }
            }
        }

        // Fall back to codec type detection
        use symphonia::core::codecs::*;
        match codec_type {
            CODEC_TYPE_FLAC => Ok(AudioCodec::Flac),
            CODEC_TYPE_PCM_S16LE | CODEC_TYPE_PCM_S16BE | 
            CODEC_TYPE_PCM_S24LE | CODEC_TYPE_PCM_S24BE |
            CODEC_TYPE_PCM_S32LE | CODEC_TYPE_PCM_S32BE |
            CODEC_TYPE_PCM_F32LE | CODEC_TYPE_PCM_F32BE |
            CODEC_TYPE_PCM_F64LE | CODEC_TYPE_PCM_F64BE => Ok(AudioCodec::Wav),
            CODEC_TYPE_ALAC => Ok(AudioCodec::Alac),
            CODEC_TYPE_MP3 => Ok(AudioCodec::Mp3),
            CODEC_TYPE_VORBIS => Ok(AudioCodec::OggVorbis),
            _ => Err(DecodeError::UnsupportedFormat {
                format: format!("Unknown codec type: {:?}", codec_type),
            }),
        }
    }

    /// Extract metadata from an already opened format reader (for use in decoders)
    pub fn extract_from_format_reader(
        _format_reader: &dyn symphonia::core::formats::FormatReader,
        probed_metadata: symphonia::core::probe::ProbedMetadata,
    ) -> AudioMetadata {
        Self::extract_metadata_from_probed(probed_metadata)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn test_metadata_extractor_with_nonexistent_file() {
        let result = MetadataExtractor::extract_from_file("/nonexistent/file.flac");
        assert!(result.is_err());
        
        if let Err(DecodeError::DecodeFailed(msg)) = result {
            assert!(msg.contains("Failed to open file"));
        } else {
            panic!("Expected DecodeFailed error");
        }
    }

    #[test]
    fn test_determine_codec_from_extension() {
        let flac_result = MetadataExtractor::determine_codec(
            "/test/file.flac", 
            symphonia::core::codecs::CODEC_TYPE_NULL
        );
        assert!(matches!(flac_result, Ok(AudioCodec::Flac)));

        let wav_result = MetadataExtractor::determine_codec(
            "/test/file.wav", 
            symphonia::core::codecs::CODEC_TYPE_NULL
        );
        assert!(matches!(wav_result, Ok(AudioCodec::Wav)));

        let mp3_result = MetadataExtractor::determine_codec(
            "/test/file.mp3", 
            symphonia::core::codecs::CODEC_TYPE_NULL
        );
        assert!(matches!(mp3_result, Ok(AudioCodec::Mp3)));

        let ogg_result = MetadataExtractor::determine_codec(
            "/test/file.ogg", 
            symphonia::core::codecs::CODEC_TYPE_NULL
        );
        assert!(matches!(ogg_result, Ok(AudioCodec::OggVorbis)));

        let alac_result = MetadataExtractor::determine_codec(
            "/test/file.m4a", 
            symphonia::core::codecs::CODEC_TYPE_NULL
        );
        assert!(matches!(alac_result, Ok(AudioCodec::Alac)));
    }

    #[test]
    fn test_determine_codec_from_codec_type() {
        use symphonia::core::codecs::*;

        let flac_result = MetadataExtractor::determine_codec(
            "/test/file.unknown", 
            CODEC_TYPE_FLAC
        );
        assert!(matches!(flac_result, Ok(AudioCodec::Flac)));

        let pcm_result = MetadataExtractor::determine_codec(
            "/test/file.unknown", 
            CODEC_TYPE_PCM_S16LE
        );
        assert!(matches!(pcm_result, Ok(AudioCodec::Wav)));

        let mp3_result = MetadataExtractor::determine_codec(
            "/test/file.unknown", 
            CODEC_TYPE_MP3
        );
        assert!(matches!(mp3_result, Ok(AudioCodec::Mp3)));

        let vorbis_result = MetadataExtractor::determine_codec(
            "/test/file.unknown", 
            CODEC_TYPE_VORBIS
        );
        assert!(matches!(vorbis_result, Ok(AudioCodec::OggVorbis)));

        let alac_result = MetadataExtractor::determine_codec(
            "/test/file.unknown", 
            CODEC_TYPE_ALAC
        );
        assert!(matches!(alac_result, Ok(AudioCodec::Alac)));
    }

    #[test]
    fn test_determine_codec_unknown() {
        let result = MetadataExtractor::determine_codec(
            "/test/file.unknown", 
            symphonia::core::codecs::CODEC_TYPE_NULL
        );
        assert!(result.is_err());
        
        if let Err(DecodeError::UnsupportedFormat { format }) = result {
            assert!(format.contains("Unknown codec type"));
        } else {
            panic!("Expected UnsupportedFormat error");
        }
    }

    #[test]
    fn test_extract_metadata_empty() {
        // Test with empty metadata
        let metadata = AudioMetadata::new();
        
        assert!(metadata.is_empty());
        assert!(metadata.title.is_none());
        assert!(metadata.artist.is_none());
        assert!(metadata.album.is_none());
        assert!(metadata.track_number.is_none());
        assert!(metadata.year.is_none());
        assert!(metadata.genre.is_none());
    }

    #[test]
    fn test_track_number_parsing() {
        // Test various track number formats that might be encountered
        let test_cases = [
            ("1", Some(1)),
            ("01", Some(1)),
            ("1/12", Some(1)),
            ("01/12", Some(1)),
            ("12/12", Some(12)),
            ("invalid", None),
            ("", None),
        ];

        for (input, expected) in test_cases {
            let track_part = input.split('/').next().unwrap_or(input);
            let result = track_part.parse::<u32>().ok();
            assert_eq!(result, expected, "Failed for input: {}", input);
        }
    }

    #[test]
    fn test_year_parsing() {
        // Test various date formats that might be encountered
        let test_cases = [
            ("2023", Some(2023)),
            ("2023-01-01", Some(2023)),
            ("2023-12-31", Some(2023)),
            ("invalid", None),
            ("", None),
        ];

        for (input, expected) in test_cases {
            let year_str = input.split('-').next().unwrap_or(input);
            let result = year_str.parse::<u32>().ok();
            assert_eq!(result, expected, "Failed for input: {}", input);
        }
    }

    #[test]
    fn test_case_insensitive_tag_matching() {
        // Test that tag key matching is case insensitive
        let test_keys = [
            ("title", true),
            ("TITLE", true),
            ("Title", true),
            ("TiTlE", true),
            ("artist", true),
            ("ARTIST", true),
            ("album", true),
            ("ALBUM", true),
            ("unknown", false),
        ];

        for (key, should_match) in test_keys {
            let matches = matches!(key.to_lowercase().as_str(), 
                "title" | "tit2" | "artist" | "tpe1" | "album" | "talb" | 
                "date" | "tyer" | "tdrc" | "genre" | "tcon" | "tracknumber" | "trck"
            );
            assert_eq!(matches, should_match, "Failed for key: {}", key);
        }
    }

    #[test]
    fn test_audio_format_creation() {
        let format = AudioFormat::new(44100, 16, 2, AudioCodec::Flac);
        
        assert_eq!(format.sample_rate, 44100);
        assert_eq!(format.bit_depth, 16);
        assert_eq!(format.channels, 2);
        assert_eq!(format.codec, AudioCodec::Flac);
        
        // Test format description
        assert_eq!(format.format_description(), "FLAC - 16-bit/44100 Hz - 2 channels");
        
        // Test high resolution detection
        assert!(!format.is_high_resolution()); // CD quality is not high-res
        
        let high_res_format = AudioFormat::new(96000, 24, 2, AudioCodec::Flac);
        assert!(high_res_format.is_high_resolution());
    }

    #[test]
    fn test_duration_calculation() {
        // Test duration calculation with various sample rates and frame counts
        let test_cases = [
            (44100, 44100, 1.0), // 1 second at 44.1kHz
            (48000, 48000, 1.0), // 1 second at 48kHz
            (44100, 22050, 0.5), // 0.5 seconds at 44.1kHz
            (96000, 96000, 1.0), // 1 second at 96kHz
        ];

        for (sample_rate, n_frames, expected_seconds) in test_cases {
            let duration = Duration::from_secs_f64(n_frames as f64 / sample_rate as f64);
            let actual_seconds = duration.as_secs_f64();
            assert!((actual_seconds - expected_seconds).abs() < 0.001, 
                "Duration calculation failed for {}Hz, {} frames", sample_rate, n_frames);
        }
    }

    #[test]
    #[ignore] // Ignored by default since it requires actual audio files
    fn test_metadata_extraction_with_real_files() {
        // This test would require real audio files with metadata in the test resources
        // You would place test files in tests/resources/ and test with them
        
        // Example:
        // let (metadata, format, duration) = MetadataExtractor::extract_from_file("tests/resources/test.flac").unwrap();
        // assert!(metadata.title.is_some());
        // assert!(metadata.artist.is_some());
        // assert_eq!(format.codec, AudioCodec::Flac);
        // assert!(duration.as_secs() > 0);
    }
}