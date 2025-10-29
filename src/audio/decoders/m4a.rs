use std::fs::File;
use std::path::Path;
use std::time::Duration;

use symphonia::core::audio::AudioBufferRef;
use symphonia::core::codecs::{Decoder, DecoderOptions, CODEC_TYPE_AAC, CODEC_TYPE_ALAC};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::{FormatOptions, FormatReader};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::{MetadataOptions, MetadataRevision, StandardTagKey, Value};
use symphonia::core::probe::Hint;
use symphonia::core::units::{Time, TimeBase};

use crate::audio::{AudioBuffer, AudioDecoder, AudioMetadata};
use crate::error::DecodeError;

/// M4A/MP4 audio decoder implementation (supports AAC and ALAC via Symphonia)
pub struct M4aDecoder {
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

impl M4aDecoder {
    /// Create a new M4A decoder for the given file path (supports AAC and ALAC)
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

        // Probe the media source for a format (isomp4/m4a)
        let probed = symphonia::default::get_probe()
            .format(&hint, media_source, &FormatOptions::default(), &MetadataOptions::default())
            .map_err(|e| DecodeError::UnsupportedFormat {
                format: format!("M4A probe failed: {}", e),
            })?;

        let format_reader = probed.format;

        // Find the first AAC or ALAC audio track
        let track = format_reader
            .tracks()
            .iter()
            .find(|t| t.codec_params.codec == CODEC_TYPE_AAC || t.codec_params.codec == CODEC_TYPE_ALAC)
            .ok_or_else(|| DecodeError::UnsupportedFormat {
                format: "No AAC or ALAC audio track found in M4A/MP4 file".to_string(),
            })?;

        let track_id = track.id;

        // Create a decoder for the track
        let decoder = symphonia::default::get_codecs()
            .make(&track.codec_params, &DecoderOptions::default())
            .map_err(|e| DecodeError::DecodeFailed(format!("Failed to create M4A decoder: {}", e)))?;

        // Extract audio format information
        let sample_rate = track.codec_params.sample_rate.unwrap_or(44100);
        let channels = track.codec_params.channels.map(|c| c.count() as u16).unwrap_or(2);

        // Determine bit depth:
        // - ALAC: preserve bits_per_sample if available (commonly 16/24)
        // - AAC: typically decoded to PCM with an effective ~16-bit depth; default to 16
        let bit_depth = match track.codec_params.bits_per_sample {
            Some(bits) => bits as u16,
            None => 16,
        };

        // Calculate duration
        let duration = if let (Some(n_frames), Some(sample_rate)) =
            (track.codec_params.n_frames, track.codec_params.sample_rate) {
            Duration::from_secs_f64(n_frames as f64 / sample_rate as f64)
        } else {
            Duration::from_secs(0) // Unknown duration
        };

        // Extract metadata during initialization (iTunes/MP4-style tags)
        let metadata = Self::extract_metadata_from_probed(probed.metadata);

        // Get time base for seeking
        let time_base = track.codec_params.time_base.unwrap_or(TimeBase::new(1, sample_rate));

        Ok(M4aDecoder {
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
                    _ => {}
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
                        let normalized = (sample_u32 as f32 - 8_388_608.0) / 8_388_608.0;
                        samples.push(normalized);
                    }
                }
            }
            AudioBufferRef::U32(buf) => {
                for plane in buf.planes().planes() {
                    for &sample in plane.iter() {
                        // Convert u32 to f32 range [-1.0, 1.0]
                        let normalized = (sample as f32 - 2_147_483_648.0) / 2_147_483_648.0;
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
                        let normalized = sample_i32 as f32 / 8_388_608.0;
                        samples.push(normalized);
                    }
                }
            }
            AudioBufferRef::S32(buf) => {
                for plane in buf.planes().planes() {
                    for &sample in plane.iter() {
                        // Convert s32 to f32 range [-1.0, 1.0]
                        let normalized = sample as f32 / 2_147_483_648.0;
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
            // Existing 'samples' is channel-planar (CCCC... DDDD...) from earlier pushes.
            // Convert from [ch0[0..F], ch1[0..F], ...] to interleaved [f0ch0, f0ch1, ..., f1ch0, f1ch1, ...]
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

impl AudioDecoder for M4aDecoder {
    fn decode_next(&mut self) -> Result<Option<AudioBuffer>, DecodeError> {
        // Get the next packet from the format reader
        let packet = match self.format_reader.next_packet() {
            Ok(packet) => packet,
            Err(SymphoniaError::IoError(ref err))
                if err.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
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
            Err(e) => Err(DecodeError::DecodeFailed(format!("Failed to decode packet: {}", e))),
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
            .seek(
                symphonia::core::formats::SeekMode::Accurate,
                symphonia::core::formats::SeekTo::Time {
                    time: seek_time,
                    track_id: Some(self.track_id),
                },
            )
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

impl M4aDecoder {
    /// Get the number of channels
    pub fn channels(&self) -> u16 {
        self.channels
    }
}
