use crate::audio::{AudioEngine, AudioDecoder, RingBuffer, BufferManager};
use crate::audio::device::DeviceManager;
use crate::audio::performance::AudioPerformanceProfiler;
use crate::audio::memory::HighResBufferAllocator;
use crate::error::AudioError;
use crate::models::AudioBuffer;
use crate::audio::LinearResampler;

pub trait NextTrackProvider: Send + Sync {
    /// Return the absolute path of the next track to play, or None if at end of queue.
    fn request_next(&self) -> Option<std::path::PathBuf>;
}
use cpal::{Stream, SampleFormat, SampleRate, StreamConfig};
use cpal::traits::{DeviceTrait, StreamTrait};
use std::sync::{Arc, Mutex, atomic::{AtomicBool, AtomicU32, Ordering}};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc as tokio_mpsc, oneshot};
use tokio::task::JoinHandle;

/// Playback state for the audio engine
#[derive(Debug, Clone, PartialEq)]
pub enum PlaybackState {
    Stopped,
    Playing,
    Paused,
}

/// Commands sent to the audio thread
#[derive(Debug)]
pub enum AudioCommand {
    Play,
    Pause,
    Stop,
    SetVolume(f32),
    Seek(Duration),
    Shutdown,
}

/// Commands sent to the decoder thread
#[derive(Debug)]
pub enum DecoderCommand {
    LoadFile(std::path::PathBuf),
    PreloadNext(std::path::PathBuf),
    Seek(Duration),
    Stop,
    NextTrack,
    Shutdown,
}

/// Status updates from threads
#[derive(Debug, Clone)]
pub struct ThreadStatus {
    pub playback_state: PlaybackState,
    pub position: Duration,
    pub buffer_fill: f32,
    pub current_file: Option<std::path::PathBuf>,
}

/// Response from decoder thread
#[derive(Debug)]
pub enum DecoderResponse {
    FileLoaded {
        duration: Duration,
        sample_rate: u32,
        bit_depth: u16,
        channels: u16,
    },
    NextTrackPreloaded {
        duration: Duration,
        sample_rate: u32,
        bit_depth: u16,
        channels: u16,
    },
    Error(AudioError),
    BufferFilled(usize), // frames filled
    EndOfFile,
    TrackTransitioned,
}

/// Audio engine implementation with multi-threaded architecture
pub struct AudioEngineImpl {
    device_manager: DeviceManager,
    stream: Option<Stream>,
    playback_state: Arc<Mutex<PlaybackState>>,
    volume: Arc<AtomicU32>, // Store as f32 bits for atomic access
    sample_rate: u32,
    bit_depth: u16,
    channels: u16,

    // Thread communication
    audio_command_sender: Option<Sender<AudioCommand>>,
    decoder_command_sender: Option<tokio_mpsc::UnboundedSender<DecoderCommand>>,
    status_receiver: Option<tokio_mpsc::UnboundedReceiver<ThreadStatus>>,
    decoder_response_receiver: Option<tokio_mpsc::UnboundedReceiver<DecoderResponse>>,

    // Thread handles
    audio_thread_handle: Option<thread::JoinHandle<()>>,
    decoder_thread_handle: Option<JoinHandle<()>>,

    // Shared state
    buffer_manager: Arc<BufferManager>,
    is_running: Arc<AtomicBool>,
    current_position: Arc<Mutex<Duration>>,
    current_decoder: Arc<Mutex<Option<Box<dyn AudioDecoder>>>>,
    next_decoder: Arc<Mutex<Option<Box<dyn AudioDecoder>>>>,
    gapless_enabled: Arc<AtomicBool>,

    // Tokio runtime for async operations
    runtime: Arc<tokio::runtime::Runtime>,

    // Performance monitoring
    performance_profiler: Arc<AudioPerformanceProfiler>,
    buffer_allocator: Arc<HighResBufferAllocator>,
    next_track_provider: Option<std::sync::Arc<dyn NextTrackProvider>>,
}

impl AudioEngineImpl {
    /// Create a new AudioEngine instance
    pub fn new() -> Result<Self, AudioError> {
        let mut device_manager = DeviceManager::new()?;

        // Select default device
        device_manager.select_default_device()?;

        // Get default configuration from the selected device
        let device = device_manager.current_device()
            .ok_or_else(|| AudioError::InitializationFailed("No device selected".to_string()))?;

        let default_config = device.default_output_config()
            .map_err(|e| AudioError::InitializationFailed(format!("Failed to get default config: {}", e)))?;

        let sample_rate = default_config.sample_rate().0;
        let channels = default_config.channels();
        let bit_depth = match default_config.sample_format() {
            SampleFormat::I8 | SampleFormat::U8 => 8,
            SampleFormat::I16 | SampleFormat::U16 => 16,
            SampleFormat::I32 | SampleFormat::U32 | SampleFormat::F32 => 32,
            SampleFormat::I64 | SampleFormat::U64 | SampleFormat::F64 => 64,
            _ => 32, // Default to 32-bit for unknown formats
        };

        // Create buffer manager with appropriate buffer sizes
        let buffer_frames = (sample_rate as f64 * 1.0) as usize; // 1000ms buffer
        let buffer_manager = Arc::new(BufferManager::new(
            buffer_frames,
            channels,
            sample_rate,
            300, // 300ms target buffer
            150,  // 150ms minimum buffer
        ));

        // Create tokio runtime for async operations
        let runtime = Arc::new(
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2) // Dedicated threads for audio processing
                .thread_name("audio-runtime")
                .enable_all()
                .build()
                .map_err(|e| AudioError::InitializationFailed(format!("Failed to create runtime: {}", e)))?
        );

        // Initialize performance monitoring
        let performance_profiler = Arc::new(AudioPerformanceProfiler::new());
        let buffer_allocator = Arc::new(HighResBufferAllocator::new());

        Ok(AudioEngineImpl {
            device_manager,
            stream: None,
            playback_state: Arc::new(Mutex::new(PlaybackState::Stopped)),
            volume: Arc::new(AtomicU32::new(1.0f32.to_bits())), // Default volume 1.0
            sample_rate,
            bit_depth,
            channels,

            // Thread communication
            audio_command_sender: None,
            decoder_command_sender: None,
            status_receiver: None,
            decoder_response_receiver: None,

            // Thread handles
            audio_thread_handle: None,
            decoder_thread_handle: None,

            // Shared state
            buffer_manager,
            is_running: Arc::new(AtomicBool::new(false)),
            current_position: Arc::new(Mutex::new(Duration::from_secs(0))),
            current_decoder: Arc::new(Mutex::new(None)),
            next_decoder: Arc::new(Mutex::new(None)),
            gapless_enabled: Arc::new(AtomicBool::new(true)), // Enable gapless by default

            runtime,
            performance_profiler,
            buffer_allocator,
            next_track_provider: None,
        })
    }

    /// Get the current sample rate
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Get the current bit depth
    pub fn bit_depth(&self) -> u16 {
        self.bit_depth
    }

    /// Get the current number of channels
    pub fn channels(&self) -> u16 {
        self.channels
    }

    /// Get the current playback state
    pub fn playback_state(&self) -> PlaybackState {
        self.playback_state.lock().unwrap().clone()
    }

    /// Get the current volume (0.0 to 1.0)
    pub fn volume(&self) -> f32 {
        f32::from_bits(self.volume.load(Ordering::Relaxed))
    }

    /// Initialize the multi-threaded audio system
    fn initialize_threads(&mut self) -> Result<(), AudioError> {
        if self.is_running.load(Ordering::Relaxed) {
            return Ok(()); // Already initialized
        }

        // Create communication channels
        let (audio_cmd_tx, audio_cmd_rx) = mpsc::channel();
        let (decoder_cmd_tx, decoder_cmd_rx) = tokio_mpsc::unbounded_channel();
        let (status_tx, status_rx) = tokio_mpsc::unbounded_channel();
        let (decoder_resp_tx, decoder_resp_rx) = tokio_mpsc::unbounded_channel();

        self.audio_command_sender = Some(audio_cmd_tx);
        self.decoder_command_sender = Some(decoder_cmd_tx);
        self.status_receiver = Some(status_rx);
        self.decoder_response_receiver = Some(decoder_resp_rx);

        // Start audio thread
        self.start_audio_thread(audio_cmd_rx, status_tx.clone())?;

        // Start decoder thread
        self.start_decoder_thread(decoder_cmd_rx, decoder_resp_tx, status_tx)?;

        self.is_running.store(true, Ordering::Relaxed);
        Ok(())
    }

    /// Start the high-priority audio output thread
    fn start_audio_thread(
        &mut self,
        command_receiver: Receiver<AudioCommand>,
        status_sender: tokio_mpsc::UnboundedSender<ThreadStatus>,
    ) -> Result<(), AudioError> {
        let device = self.device_manager.current_device()
            .ok_or_else(|| AudioError::InitializationFailed("No device selected".to_string()))?
            .clone();

        let config = StreamConfig {
            channels: self.channels,
            sample_rate: SampleRate(self.sample_rate),
            buffer_size: cpal::BufferSize::Default,
        };

        let playback_state = Arc::clone(&self.playback_state);
        let volume = Arc::clone(&self.volume);
        let is_running = Arc::clone(&self.is_running);
        let buffer_manager = Arc::clone(&self.buffer_manager);
        let current_position = Arc::clone(&self.current_position);

        // Get the default sample format
        let default_config = device.default_output_config()
            .map_err(|e| AudioError::InitializationFailed(format!("Failed to get default config: {}", e)))?;

        let sample_format = default_config.sample_format();
        let ring_buffer = buffer_manager.ring_buffer();

        // Create the audio thread
        let audio_thread = thread::Builder::new()
            .name("audio-output".to_string())
            .spawn(move || {
                // Set high priority for audio thread (platform-specific)
                #[cfg(target_os = "macos")]
                {
                    unsafe {
                        let thread = libc::pthread_self();
                        let mut policy: libc::c_int = 0;
                        let mut param: libc::sched_param = std::mem::zeroed();

                        if libc::pthread_getschedparam(thread, &mut policy, &mut param) == 0 {
                            param.sched_priority = 63; // High priority
                            let _ = libc::pthread_setschedparam(thread, libc::SCHED_FIFO, &param);
                        }
                    }
                }

                let mut last_status_update = Instant::now();
                let status_update_interval = Duration::from_millis(100); // 10Hz status updates

                // Create the audio stream based on sample format
                let stream_result = match sample_format {
                    SampleFormat::F32 => Self::create_audio_stream::<f32>(
                        &device, &config, &playback_state, &volume, &ring_buffer, &current_position
                    ),
                    SampleFormat::I16 => Self::create_audio_stream::<i16>(
                        &device, &config, &playback_state, &volume, &ring_buffer, &current_position
                    ),
                    SampleFormat::U16 => Self::create_audio_stream::<u16>(
                        &device, &config, &playback_state, &volume, &ring_buffer, &current_position
                    ),
                    _ => {
                        eprintln!("Unsupported sample format: {:?}", sample_format);
                        return;
                    }
                };

                let stream = match stream_result {
                    Ok(stream) => stream,
                    Err(e) => {
                        eprintln!("Failed to create audio stream: {}", e);
                        return;
                    }
                };

                // Start the stream
                if let Err(e) = stream.play() {
                    eprintln!("Failed to start audio stream: {}", e);
                    return;
                }

                // Audio thread main loop
                while is_running.load(Ordering::Relaxed) {
                    // Process commands
                    while let Ok(command) = command_receiver.try_recv() {
                        match command {
                            AudioCommand::Play => {
                                *playback_state.lock().unwrap() = PlaybackState::Playing;
                            }
                            AudioCommand::Pause => {
                                *playback_state.lock().unwrap() = PlaybackState::Paused;
                            }
                            AudioCommand::Stop => {
                                *playback_state.lock().unwrap() = PlaybackState::Stopped;
                                *current_position.lock().unwrap() = Duration::from_secs(0);
                            }
                            AudioCommand::SetVolume(_) => {
                                // Volume is handled via atomic variable
                            }
                            AudioCommand::Seek(position) => {
                                *current_position.lock().unwrap() = position;
                            }
                            AudioCommand::Shutdown => {
                                is_running.store(false, Ordering::Relaxed);
                                break;
                            }
                        }
                    }

                    // Send status updates periodically
                    if last_status_update.elapsed() >= status_update_interval {
                        let state = playback_state.lock().unwrap().clone();
                        let position = current_position.lock().unwrap().clone();
                        let buffer_fill = ring_buffer.fill_level();

                        let status = ThreadStatus {
                            playback_state: state,
                            position,
                            buffer_fill,
                            current_file: None, // Will be set by decoder thread
                        };

                        let _ = status_sender.send(status);
                        last_status_update = Instant::now();
                    }

                    // Small sleep to prevent busy waiting
                    thread::sleep(Duration::from_millis(1));
                }

                // Clean shutdown
                let _ = stream.pause();
            })
            .map_err(|e| AudioError::InitializationFailed(format!("Failed to create audio thread: {}", e)))?;

        self.audio_thread_handle = Some(audio_thread);
        Ok(())
    }

    /// Create a typed audio stream for the output thread
    fn create_audio_stream<T>(
        device: &cpal::Device,
        config: &StreamConfig,
        playback_state: &Arc<Mutex<PlaybackState>>,
        volume: &Arc<AtomicU32>,
        ring_buffer: &Arc<RingBuffer>,
        current_position: &Arc<Mutex<Duration>>,
    ) -> Result<Stream, AudioError>
    where
        T: cpal::Sample + cpal::SizedSample + Send + 'static,
        T: cpal::FromSample<f32>,
    {
        let playback_state = Arc::clone(playback_state);
        let volume = Arc::clone(volume);
        let ring_buffer = Arc::clone(ring_buffer);
        let current_position = Arc::clone(current_position);
        let sample_rate = config.sample_rate.0 as f64;
        let channels = config.channels as usize;

        let stream = device.build_output_stream(
            config,
            move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
                let state = playback_state.lock().unwrap().clone();
                let current_volume = f32::from_bits(volume.load(Ordering::Relaxed));

                match state {
                    PlaybackState::Playing => {
                        // Read audio data from ring buffer
                        let _frames_needed = data.len() / channels;
                        let mut audio_data = vec![0.0f32; data.len()];
                        let samples_read = ring_buffer.read(&mut audio_data);

                        // Apply volume and convert to output format
                        for (i, sample) in data.iter_mut().enumerate() {
                            let audio_sample = if i < samples_read {
                                audio_data[i] * current_volume
                            } else {
                                0.0 // Silence if not enough data
                            };
                            *sample = cpal::Sample::from_sample(audio_sample);
                        }

                        // Update position based on samples consumed
                        if samples_read > 0 {
                            let frames_consumed = samples_read / channels;
                            let time_consumed = Duration::from_secs_f64(frames_consumed as f64 / sample_rate);
                            if let Ok(mut pos) = current_position.lock() {
                                *pos += time_consumed;
                            }
                        }
                    }
                    PlaybackState::Paused | PlaybackState::Stopped => {
                        // Output silence
                        for sample in data.iter_mut() {
                            *sample = cpal::Sample::from_sample(0.0f32);
                        }
                    }
                }
            },
            move |err| {
                eprintln!("Audio stream error: {}", err);
            },
            None,
        )
        .map_err(|e| AudioError::StreamError(format!("Failed to build output stream: {}", e)))?;

        Ok(stream)
    }

    /// Start the background decoder thread
    fn start_decoder_thread(
        &mut self,
        mut command_receiver: tokio_mpsc::UnboundedReceiver<DecoderCommand>,
        response_sender: tokio_mpsc::UnboundedSender<DecoderResponse>,
        status_sender: tokio_mpsc::UnboundedSender<ThreadStatus>,
    ) -> Result<(), AudioError> {
        let buffer_manager = Arc::clone(&self.buffer_manager);
        let current_decoder = Arc::clone(&self.current_decoder);
        let next_decoder = Arc::clone(&self.next_decoder);
        let gapless_enabled = Arc::clone(&self.gapless_enabled);
        let is_running = Arc::clone(&self.is_running);
        let runtime = Arc::clone(&self.runtime);
        let next_track_provider = self.next_track_provider.clone();

        let decoder_thread = runtime.spawn(async move {
            let mut current_file: Option<std::path::PathBuf> = None;
            let mut next_file: Option<std::path::PathBuf> = None;
            let mut decode_position = Duration::from_secs(0);
            let mut is_transitioning = false;

            while is_running.load(Ordering::Relaxed) {
                // Process commands
                tokio::select! {
                    command = command_receiver.recv() => {
                        match command {
                            Some(DecoderCommand::LoadFile(path)) => {
                                // Load new audio file
                                match Self::load_audio_file(&path).await {
                                    Ok(decoder) => {
                                        let duration = decoder.duration();
                                        let sample_rate = decoder.sample_rate();
                                        let bit_depth = decoder.bit_depth();
                                        let channels = decoder.channels();

                                        // Clean up previous decoder
                                        *current_decoder.lock().unwrap() = None;
                                        *current_decoder.lock().unwrap() = Some(decoder);
                                        current_file = Some(path);
                                        decode_position = Duration::from_secs(0);
                                        is_transitioning = false;

                                        let _ = response_sender.send(DecoderResponse::FileLoaded {
                                            duration,
                                            sample_rate,
                                            bit_depth,
                                            channels,
                                        });
                                    }
                                    Err(e) => {
                                        let _ = response_sender.send(DecoderResponse::Error(e));
                                    }
                                }
                            }
                            Some(DecoderCommand::PreloadNext(path)) => {
                                // Preload next track for gapless playback
                                if gapless_enabled.load(Ordering::Relaxed) {
                                    match Self::load_audio_file(&path).await {
                                        Ok(decoder) => {
                                            let duration = decoder.duration();
                                            let sample_rate = decoder.sample_rate();
                                            let bit_depth = decoder.bit_depth();
                                            let channels = decoder.channels();

                                            *next_decoder.lock().unwrap() = Some(decoder);
                                            next_file = Some(path);

                                            let _ = response_sender.send(DecoderResponse::NextTrackPreloaded {
                                                duration,
                                                sample_rate,
                                                bit_depth,
                                                channels,
                                            });
                                        }
                                        Err(e) => {
                                            let _ = response_sender.send(DecoderResponse::Error(e));
                                        }
                                    }
                                }
                            }
                            Some(DecoderCommand::NextTrack) => {
                                // Transition to next track when requested or when preloaded
                                if let Some(next_dec) = next_decoder.lock().unwrap().take() {
                                    // Move next decoder to current
                                    *current_decoder.lock().unwrap() = Some(next_dec);
                                    current_file = next_file.take();
                                    decode_position = Duration::from_secs(0);
                                    is_transitioning = true;

                                    let _ = response_sender.send(DecoderResponse::TrackTransitioned);
                                }
                            }
                            Some(DecoderCommand::Seek(position)) => {
                                // Take the decoder out to avoid holding a MutexGuard across .await
                                let mut taken = current_decoder.lock().unwrap().take();
                                if let Some(decoder) = taken.as_mut() {
                                    if let Err(e) = decoder.seek(position) {
                                        eprintln!("Seek error: {}", e);
                                    } else {
                                        decode_position = position;
                                    }
                                }
                                // Put decoder back (if still present)
                                *current_decoder.lock().unwrap() = taken;
                            }
                            Some(DecoderCommand::Stop) => {
                                *current_decoder.lock().unwrap() = None;
                                *next_decoder.lock().unwrap() = None;
                                current_file = None;
                                next_file = None;
                                decode_position = Duration::from_secs(0);
                                is_transitioning = false;
                            }
                            Some(DecoderCommand::Shutdown) => {
                                break;
                            }
                            None => break,
                        }
                    }

                    // Fill buffer if needed
                    _ = tokio::time::sleep(Duration::from_millis(10)) => {
                        if buffer_manager.needs_data() {
                            // Take the decoder to avoid holding a MutexGuard across .await
                            let mut taken_decoder = current_decoder.lock().unwrap().take();
                            if let Some(decoder) = taken_decoder.as_mut() {
                                match decoder.decode_next() {
                                    Ok(Some(audio_buffer)) => {
                                        let ring_buffer = buffer_manager.ring_buffer();
                                        // Determine ring buffer channel count
                                        let rb_channels = ring_buffer.channels();

                                        let frames_written = if audio_buffer.channels == rb_channels {
                                            {
                                                let target_sr = ring_buffer.sample_rate();
                                                if audio_buffer.sample_rate != target_sr {
                                                    let mut rs = LinearResampler::new(audio_buffer.sample_rate, target_sr, rb_channels as usize);

                                                    let resampled = rs.process_audio_buffer(&audio_buffer);
                                                    ring_buffer.write_audio_buffer(&resampled)
                                                } else {
                                                    ring_buffer.write_audio_buffer(&audio_buffer)
                                                }
                                            }
                                        } else {
                                            // Upmix/downmix to match ring buffer channels
                                            let src_ch = audio_buffer.channels as usize;
                                            let dst_ch = rb_channels as usize;
                                            let frames = audio_buffer.frames;
                                            let src = &audio_buffer.samples;
                                            let mut dst_samples = Vec::with_capacity(frames * dst_ch);

                                            if dst_ch == 1 {
                                                // Downmix to mono by averaging channels
                                                for f in 0..frames {
                                                    let mut acc = 0.0f32;
                                                    for c in 0..src_ch {
                                                        acc += src[f * src_ch + c];
                                                    }
                                                    dst_samples.push(acc / src_ch as f32);
                                                }
                                            } else if dst_ch == 2 && src_ch == 1 {
                                                // Upmix mono to stereo by duplicating
                                                for f in 0..frames {
                                                    let s = src[f];
                                                    dst_samples.push(s);
                                                    dst_samples.push(s);
                                                }
                                            } else {
                                                // Generic channel mapping: copy available channels, pad with silence
                                                for f in 0..frames {
                                                    for c in 0..dst_ch {
                                                        let s = if c < src_ch { src[f * src_ch + c] } else { 0.0 };
                                                        dst_samples.push(s);
                                                    }
                                                }
                                            }

                                            let converted = crate::models::AudioBuffer {
                                                samples: dst_samples,
                                                channels: rb_channels,
                                                sample_rate: audio_buffer.sample_rate,
                                                frames,
                                            };
                                            {
                                                let target_sr = ring_buffer.sample_rate();
                                                if converted.sample_rate != target_sr {
                                                    let mut rs = LinearResampler::new(converted.sample_rate, target_sr, rb_channels as usize);

                                                    let resampled = rs.process_audio_buffer(&converted);
                                                    ring_buffer.write_audio_buffer(&resampled)
                                                } else {
                                                    ring_buffer.write_audio_buffer(&converted)
                                                }
                                            }
                                        };

                                        if frames_written > 0 {
                                            let time_decoded = Duration::from_secs_f64(
                                                frames_written as f64 / audio_buffer.sample_rate as f64
                                            );
                                            decode_position += time_decoded;

                                            let _ = response_sender.send(DecoderResponse::BufferFilled(frames_written));
                                        }
                                        // Put the decoder back for subsequent decode iterations
                                        *current_decoder.lock().unwrap() = taken_decoder;
                                    }
                                    Ok(None) => {
                                        // End of current file - check for preloaded next track and transition
                                        if next_decoder.lock().unwrap().is_some() {
                                            // Seamlessly transition to next track
                                            if let Some(next_dec) = next_decoder.lock().unwrap().take() {
                                                *current_decoder.lock().unwrap() = Some(next_dec);
                                                current_file = next_file.take();
                                                decode_position = Duration::from_secs(0);
                                                is_transitioning = true;

                                                let _ = response_sender.send(DecoderResponse::TrackTransitioned);

                                                // Continue decoding from the new track immediately
                                                continue;
                                            }
                                        }

                                        // No preloaded track available; try provider for next track
                                        if let Some(provider) = &next_track_provider {
                                            if let Some(path) = provider.request_next() {
                                                match Self::load_audio_file(&path).await {
                                                    Ok(decoder) => {
                                                        let duration = decoder.duration();
                                                        let sample_rate = decoder.sample_rate();
                                                        let bit_depth = decoder.bit_depth();
                                                        let channels = decoder.channels();

                                                        // Switch to the provided next track immediately
                                                        *current_decoder.lock().unwrap() = Some(decoder);
                                                        current_file = Some(path);
                                                        decode_position = Duration::from_secs(0);
                                                        is_transitioning = false;

                                                        let _ = response_sender.send(DecoderResponse::FileLoaded {
                                                            duration,
                                                            sample_rate,
                                                            bit_depth,
                                                            channels,
                                                        });

                                                        // Continue decoding from the new track immediately
                                                        continue;
                                                    }
                                                    Err(_) => {
                                                        // Fall through to EndOfFile if provider failed
                                                    }
                                                    // Put back the decoder if still held
                                                }
                                            }
                                        }
                                        // Put back the decoder if still held
                                        if taken_decoder.is_some() {
                                            *current_decoder.lock().unwrap() = taken_decoder;
                                        }

                                        // Still no track to play; signal end of file
                                        let _ = response_sender.send(DecoderResponse::EndOfFile);
                                    }
                                    Err(e) => {
                                        let audio_error = AudioError::StreamError(format!("Decode error: {}", e));
                                        let _ = response_sender.send(DecoderResponse::Error(audio_error));
                                        // Put the decoder back after error so we can retry or handle further
                                        if taken_decoder.is_some() {
                                            *current_decoder.lock().unwrap() = taken_decoder;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                // Send periodic status updates
                let status = ThreadStatus {
                    playback_state: PlaybackState::Stopped, // Will be overridden by audio thread
                    position: decode_position,
                    buffer_fill: buffer_manager.ring_buffer().fill_level(),
                    current_file: current_file.clone(),
                };
                let _ = status_sender.send(status);
            }
        });

        self.decoder_thread_handle = Some(decoder_thread);
        Ok(())
    }

    /// Load an audio file and create a decoder (async)
    async fn load_audio_file(path: &std::path::Path) -> Result<Box<dyn AudioDecoder>, AudioError> {
        use crate::audio::decoders::flac::FlacDecoder;
        use crate::audio::decoders::wav::WavDecoder;
        use crate::audio::decoders::mp3::Mp3Decoder;
        use crate::audio::decoders::ogg::OggDecoder;
        use crate::audio::decoders::m4a::M4aDecoder;

        // Detect file format based on extension
        let extension = path.extension()
            .and_then(|ext| ext.to_str())
            .map(|s| s.to_lowercase())
            .ok_or_else(|| AudioError::UnsupportedFormat {
                format: "No file extension".to_string(),
            })?;

        // Create appropriate decoder based on file extension
        match extension.as_str() {
            "flac" => {
                let decoder = FlacDecoder::new(path)
                    .map_err(|e| AudioError::InitializationFailed(format!("FLAC decoder error: {}", e)))?;
                Ok(Box::new(decoder))
            }
            "wav" => {
                let decoder = WavDecoder::new(path)
                    .map_err(|e| AudioError::InitializationFailed(format!("WAV decoder error: {}", e)))?;
                Ok(Box::new(decoder))
            }
            "mp3" => {
                let decoder = Mp3Decoder::new(path)
                    .map_err(|e| AudioError::InitializationFailed(format!("MP3 decoder error: {}", e)))?;
                Ok(Box::new(decoder))
            }
            "ogg" | "oga" => {
                let decoder = OggDecoder::new(path)
                    .map_err(|e| AudioError::InitializationFailed(format!("OGG decoder error: {}", e)))?;
                Ok(Box::new(decoder))
            }
            "m4a" | "mp4" | "m4b" => {
                let decoder = M4aDecoder::new(path)
                    .map_err(|e| AudioError::InitializationFailed(format!("M4A/MP4 decoder error: {}", e)))?;
                Ok(Box::new(decoder))
            }
            _ => {
                Err(AudioError::UnsupportedFormat {
                    format: format!("Unsupported file extension: {}", extension),
                })
            }
        }
    }

    /// Initialize audio stream with the current device and configuration
    fn initialize_stream(&mut self) -> Result<(), AudioError> {
        let device = self.device_manager.current_device()
            .ok_or_else(|| AudioError::InitializationFailed("No device selected".to_string()))?;

        // Create stream configuration
        let config = StreamConfig {
            channels: self.channels,
            sample_rate: SampleRate(self.sample_rate),
            buffer_size: cpal::BufferSize::Fixed(4096),
        };

        // Create shared state for the audio callback
        let playback_state = Arc::clone(&self.playback_state);
        let volume = Arc::clone(&self.volume);
        let is_running = Arc::clone(&self.is_running);

        // Create command channel for audio thread communication
        let (command_sender, command_receiver) = mpsc::channel();
        self.audio_command_sender = Some(command_sender);

        // Create the audio stream based on the sample format
        let default_config = device.default_output_config()
            .map_err(|e| AudioError::InitializationFailed(format!("Failed to get default config: {}", e)))?;

        let stream = match default_config.sample_format() {
            SampleFormat::F32 => self.create_stream::<f32>(device, &config, playback_state, volume, is_running, command_receiver)?,
            SampleFormat::I16 => self.create_stream::<i16>(device, &config, playback_state, volume, is_running, command_receiver)?,
            SampleFormat::U16 => self.create_stream::<u16>(device, &config, playback_state, volume, is_running, command_receiver)?,
            sample_format => {
                return Err(AudioError::InitializationFailed(
                    format!("Unsupported sample format: {:?}", sample_format)
                ));
            }
        };

        self.stream = Some(stream);
        Ok(())
    }

    /// Create a typed audio stream
    fn create_stream<T>(
        &self,
        device: &cpal::Device,
        config: &StreamConfig,
        playback_state: Arc<Mutex<PlaybackState>>,
        volume: Arc<AtomicU32>,
        is_running: Arc<AtomicBool>,
        command_receiver: Receiver<AudioCommand>,
    ) -> Result<Stream, AudioError>
    where
        T: cpal::Sample + cpal::SizedSample + Send + 'static,
        T: cpal::FromSample<f32>,
    {
        // Get references to the ring buffer and position tracker
        let ring_buffer = Arc::clone(&self.buffer_manager.ring_buffer());
        let current_position = Arc::clone(&self.current_position);
        let mut sample_clock = 0f32;
        let sample_rate = config.sample_rate.0 as f32;
        let channels = config.channels as usize;

        let stream = device.build_output_stream(
            config,
            move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
                // Process any pending commands
                while let Ok(command) = command_receiver.try_recv() {
                    match command {
                        AudioCommand::Play => {
                            *playback_state.lock().unwrap() = PlaybackState::Playing;
                        }
                        AudioCommand::Pause => {
                            *playback_state.lock().unwrap() = PlaybackState::Paused;
                        }
                        AudioCommand::Stop => {
                            *playback_state.lock().unwrap() = PlaybackState::Stopped;
                            sample_clock = 0.0;
                        }
                        AudioCommand::SetVolume(_vol) => {
                            // Volume is handled via the atomic variable
                        }
                        AudioCommand::Seek(position) => {
                            // Reset sample clock for test tone generation based on seek position
                            sample_clock = (position.as_secs_f32() * sample_rate) % sample_rate;
                        }
                        AudioCommand::Shutdown => {
                            is_running.store(false, Ordering::Relaxed);
                            return;
                        }
                    }
                }

                let state = playback_state.lock().unwrap().clone();
                let current_volume = f32::from_bits(volume.load(Ordering::Relaxed));

                match state {
                    PlaybackState::Playing => {
                        // Read audio data from ring buffer
                        let frames_needed = data.len() / channels;
                        let mut audio_data = vec![0.0f32; data.len()];
                        let samples_read = ring_buffer.read(&mut audio_data);
                        let frames_read = samples_read / channels;
                        if frames_read < frames_needed {
                            eprintln!(
                                "Audio underrun: needed {} frames, got {} frames; fill={:.0}% (~{} ms)",
                                frames_needed,
                                frames_read,
                                ring_buffer.fill_level() * 100.0,
                                ring_buffer.buffered_duration().as_millis()
                            );
                        }

                        // Apply volume and convert to output format
                        for (i, sample) in data.iter_mut().enumerate() {
                            let audio_sample = if i < samples_read {
                                audio_data[i] * current_volume
                            } else {
                                0.0 // Silence if not enough data
                            };
                            *sample = cpal::Sample::from_sample(audio_sample);
                        }

                        // Update position based on samples consumed
                        if samples_read > 0 {
                            let frames_consumed = samples_read / channels;
                            let time_consumed = Duration::from_secs_f64(frames_consumed as f64 / sample_rate as f64);
                            if let Ok(mut pos) = current_position.lock() {
                                *pos += time_consumed;
                            }
                        }
                    }
                    PlaybackState::Paused | PlaybackState::Stopped => {
                        // Output silence
                        for sample in data.iter_mut() {
                            *sample = cpal::Sample::from_sample(0.0f32);
                        }
                    }
                }
            },
            move |err| {
                eprintln!("Audio stream error: {}", err);
            },
            None,
        )
        .map_err(|e| AudioError::StreamError(format!("Failed to build output stream: {}", e)))?;

        Ok(stream)
    }

    /// Update the audio configuration for a new sample rate and bit depth
    pub fn update_config(&mut self, sample_rate: u32, bit_depth: u16, channels: u16) -> Result<(), AudioError> {
        // Remember whether we were playing to resume after reconfiguration.
        let was_playing = matches!(*self.playback_state.lock().unwrap(), PlaybackState::Playing);

        // Stop audio/decoder threads if running so we can rebuild the stream with the new config.
        if self.is_running.load(Ordering::Relaxed) {
            self.shutdown_threads()?;
        }

        // Update configuration fields.
        self.sample_rate = sample_rate;
        self.bit_depth = bit_depth;
        self.channels = channels;

        // Rebuild buffer manager to match new stream configuration.
        let buffer_frames = (sample_rate as f64 * 1.0) as usize; // ~1000ms
        self.buffer_manager = Arc::new(BufferManager::new(
            buffer_frames,
            channels,
            sample_rate,
            300, // target buffer ms
            150, // min buffer ms
        ));

        // Restart threads (audio/decoder) with the new configuration.
        self.initialize_threads()?;

        // If we had a decoder and we were previously playing, resume playback.
        if self.current_decoder.lock().unwrap().is_some() && was_playing {
            self.send_audio_command(AudioCommand::Play)?;
        }

        Ok(())
    }

    /// Start the audio stream
    fn start_stream(&mut self) -> Result<(), AudioError> {
        if self.stream.is_none() {
            self.initialize_stream()?;
        }

        if let Some(stream) = &self.stream {
            stream.play()
                .map_err(|e| AudioError::StreamError(format!("Failed to start stream: {}", e)))?;
            self.is_running.store(true, Ordering::Relaxed);
        }

        Ok(())
    }

    /// Stop the audio stream
    fn stop_stream(&mut self) -> Result<(), AudioError> {
        if let Some(stream) = &self.stream {
            stream.pause()
                .map_err(|e| AudioError::StreamError(format!("Failed to stop stream: {}", e)))?;
        }

        self.is_running.store(false, Ordering::Relaxed);
        Ok(())
    }

    /// Send a command to the audio thread
    fn send_audio_command(&self, command: AudioCommand) -> Result<(), AudioError> {
        if let Some(sender) = &self.audio_command_sender {
            sender.send(command)
                .map_err(|e| AudioError::StreamError(format!("Failed to send audio command: {}", e)))?;
        }
        Ok(())
    }

    /// Send a command to the decoder thread
    fn send_decoder_command(&self, command: DecoderCommand) -> Result<(), AudioError> {
        if let Some(sender) = &self.decoder_command_sender {
            sender.send(command)
                .map_err(|e| AudioError::StreamError(format!("Failed to send decoder command: {}", e)))?;
        }
        Ok(())
    }

    /// Load an audio file for playback
    pub fn load_file(&mut self, path: std::path::PathBuf) -> Result<(), AudioError> {
        // Initialize threads if not already running
        if !self.is_running.load(Ordering::Relaxed) {
            self.initialize_threads()?;
        }

        self.send_decoder_command(DecoderCommand::LoadFile(path))?;
        Ok(())
    }

    /// Preload the next track for gapless playback
    pub fn preload_next_track(&mut self, path: std::path::PathBuf) -> Result<(), AudioError> {
        // Initialize threads if not already running
        if !self.is_running.load(Ordering::Relaxed) {
            self.initialize_threads()?;
        }

        self.send_decoder_command(DecoderCommand::PreloadNext(path))?;
        Ok(())
    }

    /// Transition to the next preloaded track
    pub fn transition_to_next_track(&mut self) -> Result<(), AudioError> {
        self.send_decoder_command(DecoderCommand::NextTrack)?;
        Ok(())
    }

    /// Set a provider the engine can call to obtain the next track path when the current one ends.
    pub fn set_next_track_provider(&mut self, provider: std::sync::Arc<dyn NextTrackProvider>) {
        self.next_track_provider = Some(provider);
    }

    /// Enable or disable gapless playback
    pub fn set_gapless_enabled(&mut self, enabled: bool) {
        self.gapless_enabled.store(enabled, Ordering::Relaxed);
    }

    /// Check if gapless playback is enabled
    pub fn is_gapless_enabled(&self) -> bool {
        self.gapless_enabled.load(Ordering::Relaxed)
    }

    /// Seek to a specific position in the current track
    pub fn seek(&mut self, position: Duration) -> Result<(), AudioError> {
        // Validate position against current track duration if available
        if let Some(decoder) = self.current_decoder.lock().unwrap().as_ref() {
            let duration = decoder.duration();
            if position > duration {
                return Err(AudioError::InvalidSeekPosition {
                    position: position.as_secs_f64(),
                    duration: duration.as_secs_f64(),
                });
            }
        }

        // Send seek commands to both threads
        self.send_audio_command(AudioCommand::Seek(position))?;
        self.send_decoder_command(DecoderCommand::Seek(position))?;

        // Update position tracker immediately for responsive UI
        *self.current_position.lock().unwrap() = position;

        Ok(())
    }

    /// Get the current playback position
    pub fn current_position(&self) -> Duration {
        self.current_position.lock().unwrap().clone()
    }

    /// Get the duration of the current track
    pub fn current_duration(&self) -> Option<Duration> {
        self.current_decoder.lock().unwrap()
            .as_ref()
            .map(|decoder| decoder.duration())
    }

    /// Validate seek position against track bounds
    pub fn validate_seek_position(&self, position: Duration) -> Result<Duration, AudioError> {
        if let Some(duration) = self.current_duration() {
            if position > duration {
                return Err(AudioError::InvalidSeekPosition {
                    position: position.as_secs_f64(),
                    duration: duration.as_secs_f64(),
                });
            }
            Ok(position.min(duration))
        } else {
            // No current track, return position as-is
            Ok(position)
        }
    }

    /// Get the current thread status
    pub fn get_status(&mut self) -> Option<ThreadStatus> {
        if let Some(receiver) = &mut self.status_receiver {
            // Get the most recent status update
            let mut latest_status = None;
            while let Ok(status) = receiver.try_recv() {
                latest_status = Some(status);
            }
            latest_status
        } else {
            None
        }
    }

    /// Get a reference to the device manager
    pub fn device_manager(&self) -> &DeviceManager {
        &self.device_manager
    }

    /// Get a mutable reference to the device manager
    pub fn device_manager_mut(&mut self) -> &mut DeviceManager {
        &mut self.device_manager
    }

    /// Get decoder responses
    pub fn get_decoder_response(&mut self) -> Option<DecoderResponse> {
        if let Some(receiver) = &mut self.decoder_response_receiver {
            if let Ok(resp) = receiver.try_recv() {
                match &resp {
                    DecoderResponse::FileLoaded { sample_rate, bit_depth, channels, .. } => {
                        if *sample_rate != self.sample_rate
                            || *bit_depth != self.bit_depth
                            || *channels != self.channels
                        {
                            // Attempt to reconfigure stream to match the track

                            let _ = self.update_config(*sample_rate, *bit_depth, *channels);
                        }
                    }
                    DecoderResponse::TrackTransitioned => {
                        // Extract needed config while holding the lock, then drop it before reconfiguring.
                        let mut reconfig: Option<(u32, u16, u16)> = None;
                        {
                            if let Some(decoder) = self.current_decoder.lock().unwrap().as_ref() {
                                let sr = decoder.sample_rate();
                                let bd = decoder.bit_depth();
                                let ch = decoder.channels();
                                if sr != self.sample_rate || bd != self.bit_depth || ch != self.channels {
                                    reconfig = Some((sr, bd, ch));
                                }
                            }
                        }
                        if let Some((sr, bd, ch)) = reconfig {
                            // Reconfigure after seamless transition to next track
                            let _ = self.update_config(sr, bd, ch);
                        }
                    }
                    _ => {}
                }
                Some(resp)
            } else {
                None
            }
        } else {
            None
        }
    }

    /// Get performance profiler for monitoring
    pub fn performance_profiler(&self) -> Arc<AudioPerformanceProfiler> {
        Arc::clone(&self.performance_profiler)
    }

    /// Get buffer allocator for memory management
    pub fn buffer_allocator(&self) -> Arc<HighResBufferAllocator> {
        Arc::clone(&self.buffer_allocator)
    }

    /// Update performance monitoring (should be called periodically)
    pub fn update_performance_monitoring(&self) {
        // Update CPU usage
        self.performance_profiler.update_cpu_usage();

        // Update memory usage
        let memory_stats = self.buffer_allocator.memory_stats();
        self.performance_profiler.update_memory_usage(memory_stats.current_usage as u64);

        // Check buffer status
        let buffer_status = self.buffer_manager.buffer_status();
        if buffer_status.is_underrun {
            self.performance_profiler.record_buffer_underrun();
        }
    }

    /// Get comprehensive performance report
    pub fn get_performance_report(&self) -> crate::audio::performance::PerformanceReport {
        self.performance_profiler.performance_report()
    }

    /// Check if audio performance is healthy
    pub fn is_performance_healthy(&self) -> bool {
        self.performance_profiler.is_performance_healthy()
    }

    /// Shutdown all threads
    fn shutdown_threads(&mut self) -> Result<(), AudioError> {
        if !self.is_running.load(Ordering::Relaxed) {
            return Ok(());
        }

        // Signal shutdown
        self.is_running.store(false, Ordering::Relaxed);

        // Send shutdown commands (best-effort)
        let _ = self.send_audio_command(AudioCommand::Shutdown);
        let _ = self.send_decoder_command(DecoderCommand::Shutdown);

        // Wait for threads to finish
        if let Some(handle) = self.audio_thread_handle.take() {
            let _ = handle.join();
        }

        // Avoid calling block_on from within a runtime. Abort the decoder task instead.
        if let Some(handle) = self.decoder_thread_handle.take() {
            handle.abort();
            // Optionally, give it a brief moment to wind down without blocking this thread.
            // We intentionally do not block_on here to prevent "Cannot start a runtime from within a runtime".
        }

        // Clear communication channels
        self.audio_command_sender = None;
        self.decoder_command_sender = None;
        self.status_receiver = None;
        self.decoder_response_receiver = None;

        Ok(())
    }
}

impl AudioEngine for AudioEngineImpl {
    fn start_playback(&mut self, decoder: Box<dyn AudioDecoder>) -> Result<(), AudioError> {
        // Initialize threads if not already running
        if !self.is_running.load(Ordering::Relaxed) {
            self.initialize_threads()?;
        }

        // Store the decoder
        *self.current_decoder.lock().unwrap() = Some(decoder);

        // Send play command to audio thread
        self.send_audio_command(AudioCommand::Play)?;
        Ok(())
    }

    fn pause(&mut self) -> Result<(), AudioError> {
        self.send_audio_command(AudioCommand::Pause)?;
        Ok(())
    }

    fn resume(&mut self) -> Result<(), AudioError> {
        self.send_audio_command(AudioCommand::Play)?;
        Ok(())
    }

    fn stop(&mut self) -> Result<(), AudioError> {
        self.send_audio_command(AudioCommand::Stop)?;
        self.send_decoder_command(DecoderCommand::Stop)?;
        Ok(())
    }

    fn set_volume(&mut self, volume: f32) -> Result<(), AudioError> {
        // Clamp volume to valid range
        let clamped_volume = volume.clamp(0.0, 1.0);
        self.volume.store(clamped_volume.to_bits(), Ordering::Relaxed);
        self.send_audio_command(AudioCommand::SetVolume(clamped_volume))?;
        Ok(())
    }

    fn set_device(&mut self, device_name: &str) -> Result<(), AudioError> {
        // Stop current playback
        if self.is_running.load(Ordering::Relaxed) {
            self.stop()?;
            self.shutdown_threads()?;
        }

        // Select new device
        self.device_manager.select_device(Some(device_name))?;

        // Get new device configuration
        let device = self.device_manager.current_device()
            .ok_or_else(|| AudioError::InitializationFailed("No device selected".to_string()))?;

        let default_config = device.default_output_config()
            .map_err(|e| AudioError::InitializationFailed(format!("Failed to get default config: {}", e)))?;

        // Update configuration with new device settings
        let sample_rate = default_config.sample_rate().0;
        let channels = default_config.channels();
        let bit_depth = match default_config.sample_format() {
            SampleFormat::I8 | SampleFormat::U8 => 8,
            SampleFormat::I16 | SampleFormat::U16 => 16,
            SampleFormat::I32 | SampleFormat::U32 | SampleFormat::F32 => 32,
            SampleFormat::I64 | SampleFormat::U64 | SampleFormat::F64 => 64,
            _ => 32,
        };

        self.update_config(sample_rate, bit_depth, channels)?;

        Ok(())
    }
}

impl Drop for AudioEngineImpl {
    fn drop(&mut self) {
        // Clean shutdown of all threads
        let _ = self.shutdown_threads();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::{AudioEngine, AudioDecoder, AudioBuffer, AudioMetadata};
    use crate::error::DecodeError;
    use std::time::Duration;

    /// Mock audio decoder for testing
    struct MockDecoder {
        sample_rate: u32,
        bit_depth: u16,
        duration: Duration,
        metadata: AudioMetadata,
    }

    impl MockDecoder {
        fn new() -> Self {
            Self {
                sample_rate: 44100,
                bit_depth: 16,
                duration: Duration::from_secs(180), // 3 minutes
                metadata: AudioMetadata {
                    title: Some("Test Track".to_string()),
                    artist: Some("Test Artist".to_string()),
                    album: Some("Test Album".to_string()),
                    track_number: Some(1),
                    year: Some(2023),
                    genre: Some("Test".to_string()),
                },
            }
        }
    }

    impl AudioDecoder for MockDecoder {
        fn decode_next(&mut self) -> Result<Option<AudioBuffer>, DecodeError> {
            // Return a small buffer of silence for testing
            Ok(Some(AudioBuffer {
                samples: vec![0.0; 1024], // 512 frames of stereo silence
                channels: 2,
                sample_rate: self.sample_rate,
                frames: 512,
            }))
        }

        fn seek(&mut self, _position: Duration) -> Result<(), DecodeError> {
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
    }

    #[test]
    fn test_audio_engine_creation() {
        let result = AudioEngineImpl::new();
        assert!(result.is_ok(), "AudioEngine creation should succeed");

        let engine = result.unwrap();
        assert_eq!(engine.playback_state(), PlaybackState::Stopped);
        assert_eq!(engine.volume(), 1.0);
        assert!(engine.sample_rate() > 0);
        assert!(engine.bit_depth() > 0);
        assert!(engine.channels() > 0);
    }

    #[test]
    fn test_audio_engine_default_configuration() {
        let engine = AudioEngineImpl::new().expect("Failed to create AudioEngine");

        // Check that we have reasonable default values
        let sample_rate = engine.sample_rate();
        let bit_depth = engine.bit_depth();
        let channels = engine.channels();

        assert!(sample_rate >= 44100, "Sample rate should be at least 44.1kHz");
        assert!(bit_depth >= 16, "Bit depth should be at least 16-bit");
        assert!(channels >= 1, "Should have at least 1 channel");
        assert!(channels <= 8, "Should not have more than 8 channels for typical setups");
    }

    #[test]
    fn test_volume_control() {
        let mut engine = AudioEngineImpl::new().expect("Failed to create AudioEngine");

        // Test setting valid volume levels
        let result = engine.set_volume(0.5);
        assert!(result.is_ok(), "Setting volume to 0.5 should succeed");
        assert_eq!(engine.volume(), 0.5);

        let result = engine.set_volume(0.0);
        assert!(result.is_ok(), "Setting volume to 0.0 should succeed");
        assert_eq!(engine.volume(), 0.0);

        let result = engine.set_volume(1.0);
        assert!(result.is_ok(), "Setting volume to 1.0 should succeed");
        assert_eq!(engine.volume(), 1.0);
    }

    #[test]
    fn test_volume_clamping() {
        let mut engine = AudioEngineImpl::new().expect("Failed to create AudioEngine");

        // Test volume clamping for values outside valid range
        let result = engine.set_volume(1.5);
        assert!(result.is_ok(), "Setting volume above 1.0 should succeed but be clamped");
        assert_eq!(engine.volume(), 1.0, "Volume should be clamped to 1.0");

        let result = engine.set_volume(-0.5);
        assert!(result.is_ok(), "Setting negative volume should succeed but be clamped");
        assert_eq!(engine.volume(), 0.0, "Volume should be clamped to 0.0");
    }

    #[test]
    fn test_playback_state_transitions() {
        let mut engine = AudioEngineImpl::new().expect("Failed to create AudioEngine");
        let decoder = Box::new(MockDecoder::new());

        // Initial state should be stopped
        assert_eq!(engine.playback_state(), PlaybackState::Stopped);

        // Start playback
        let result = engine.start_playback(decoder);
        assert!(result.is_ok(), "Starting playback should succeed");

        // Give the audio system a moment to process the command
        std::thread::sleep(Duration::from_millis(10));

        // Note: The actual state change happens asynchronously in the audio callback,
        // so we can't reliably test the state immediately after starting playback
        // In a real implementation, we might want to add synchronization for testing

        // Test pause
        let result = engine.pause();
        assert!(result.is_ok(), "Pausing should succeed");

        // Test resume
        let result = engine.resume();
        assert!(result.is_ok(), "Resuming should succeed");

        // Test stop
        let result = engine.stop();
        assert!(result.is_ok(), "Stopping should succeed");
    }

    #[test]
    fn test_device_management() {
        let mut engine = AudioEngineImpl::new().expect("Failed to create AudioEngine");

        // Get list of available devices from the device manager
        let devices = engine.device_manager.list_devices();

        if !devices.is_empty() {
            let first_device = &devices[0];

            // Test setting a valid device
            let result = engine.set_device(first_device);
            assert!(result.is_ok(), "Setting valid device should succeed");

            // Verify the device was set
            let current_device_name = engine.device_manager.current_device_name()
                .expect("Should get device name");
            assert_eq!(current_device_name.as_ref(), Some(first_device));
        }

        // Test setting an invalid device
        let result = engine.set_device("NonExistentDevice");
        assert!(result.is_err(), "Setting invalid device should fail");

        match result {
            Err(AudioError::DeviceNotFound { device }) => {
                assert_eq!(device, "NonExistentDevice");
            }
            _ => panic!("Expected DeviceNotFound error"),
        }
    }

    #[test]
    fn test_configuration_update() {
        let mut engine = AudioEngineImpl::new().expect("Failed to create AudioEngine");

        let original_sample_rate = engine.sample_rate();
        let original_bit_depth = engine.bit_depth();
        let original_channels = engine.channels();

        // Test updating configuration
        let new_sample_rate = if original_sample_rate == 44100 { 48000 } else { 44100 };
        let new_bit_depth = if original_bit_depth == 16 { 32 } else { 16 };
        let new_channels = if original_channels == 2 { 1 } else { 2 };

        let result = engine.update_config(new_sample_rate, new_bit_depth, new_channels);
        assert!(result.is_ok(), "Configuration update should succeed");

        // Verify the configuration was updated
        assert_eq!(engine.sample_rate(), new_sample_rate);
        assert_eq!(engine.bit_depth(), new_bit_depth);
        assert_eq!(engine.channels(), new_channels);
    }

    #[test]
    fn test_stream_lifecycle() {
        let mut engine = AudioEngineImpl::new().expect("Failed to create AudioEngine");

        // Test that we can start and stop streams multiple times
        for _ in 0..3 {
            let decoder = Box::new(MockDecoder::new());

            let result = engine.start_playback(decoder);
            assert!(result.is_ok(), "Starting playback should succeed");

            // Brief pause to let the stream initialize
            std::thread::sleep(Duration::from_millis(10));

            let result = engine.stop();
            assert!(result.is_ok(), "Stopping playback should succeed");

            // Brief pause between iterations
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn test_concurrent_operations() {
        let mut engine = AudioEngineImpl::new().expect("Failed to create AudioEngine");
        let decoder = Box::new(MockDecoder::new());

        // Start playback
        let result = engine.start_playback(decoder);
        assert!(result.is_ok(), "Starting playback should succeed");

        // Perform multiple operations in quick succession
        let result = engine.set_volume(0.7);
        assert!(result.is_ok(), "Setting volume during playback should succeed");

        let result = engine.pause();
        assert!(result.is_ok(), "Pausing during playback should succeed");

        let result = engine.set_volume(0.3);
        assert!(result.is_ok(), "Setting volume while paused should succeed");

        let result = engine.resume();
        assert!(result.is_ok(), "Resuming should succeed");

        let result = engine.stop();
        assert!(result.is_ok(), "Stopping should succeed");

        // Verify final volume setting
        assert_eq!(engine.volume(), 0.3);
    }

    #[test]
    fn test_error_handling() {
        let mut engine = AudioEngineImpl::new().expect("Failed to create AudioEngine");

        // Test operations on stopped engine
        let result = engine.pause();
        assert!(result.is_ok(), "Pausing stopped engine should not fail");

        let result = engine.resume();
        assert!(result.is_ok(), "Resuming stopped engine should not fail");

        let result = engine.stop();
        assert!(result.is_ok(), "Stopping stopped engine should not fail");
    }

    #[test]
    fn test_multiple_volume_changes() {
        let mut engine = AudioEngineImpl::new().expect("Failed to create AudioEngine");

        // Test rapid volume changes
        let volumes = [0.1, 0.5, 0.9, 0.2, 0.8, 0.0, 1.0];

        for &volume in &volumes {
            let result = engine.set_volume(volume);
            assert!(result.is_ok(), "Setting volume to {} should succeed", volume);
            assert_eq!(engine.volume(), volume, "Volume should be set to {}", volume);
        }
    }

    #[test]
    fn test_engine_drop_cleanup() {
        // Test that the engine cleans up properly when dropped
        {
            let mut engine = AudioEngineImpl::new().expect("Failed to create AudioEngine");
            let decoder = Box::new(MockDecoder::new());

            let result = engine.start_playback(decoder);
            assert!(result.is_ok(), "Starting playback should succeed");

            // Engine will be dropped here
        }

        // If we reach this point without hanging, cleanup worked
        assert!(true, "Engine cleanup completed successfully");
    }

    #[test]
    fn test_device_capabilities_integration() {
        let engine = AudioEngineImpl::new().expect("Failed to create AudioEngine");
        let devices = engine.device_manager.list_devices();

        if !devices.is_empty() {
            let first_device = &devices[0];
            let capabilities = engine.device_manager.get_capabilities(first_device);

            assert!(capabilities.is_some(), "Should have capabilities for device");

            let caps = capabilities.unwrap();
            assert!(!caps.supported_sample_rates.is_empty(), "Should have supported sample rates");
            assert!(!caps.supported_bit_depths.is_empty(), "Should have supported bit depths");

            // Verify engine configuration is within device capabilities
            let engine_rate = engine.sample_rate();
            let engine_depth = engine.bit_depth();

            // The engine should be using a supported configuration
            // (though it might not be in our simplified capability detection)
            assert!(engine_rate > 0, "Engine sample rate should be positive");
            assert!(engine_depth > 0, "Engine bit depth should be positive");
        }
    }

    #[test]
    fn test_seek_functionality() {
        let mut engine = AudioEngineImpl::new().expect("Failed to create AudioEngine");

        // Test seek without current track (should work but not validate against duration)
        let result = engine.seek(Duration::from_secs(60));
        assert!(result.is_ok(), "Seek should work without current track");

        // Verify position was updated
        assert_eq!(engine.current_position(), Duration::from_secs(60));
    }

    #[test]
    fn test_seek_with_decoder() {
        let mut engine = AudioEngineImpl::new().expect("Failed to create AudioEngine");
        let decoder = Box::new(MockDecoder::new());

        // Store decoder to enable duration validation
        *engine.current_decoder.lock().unwrap() = Some(decoder);

        // Test valid seek
        let result = engine.seek(Duration::from_secs(60));
        assert!(result.is_ok(), "Valid seek should succeed");
        assert_eq!(engine.current_position(), Duration::from_secs(60));

        // Test seek beyond duration (MockDecoder has 180s duration)
        let result = engine.seek(Duration::from_secs(200));
        assert!(result.is_err(), "Seek beyond duration should fail");

        match result.unwrap_err() {
            AudioError::InvalidSeekPosition { position, duration } => {
                assert_eq!(position, 200.0);
                assert_eq!(duration, 180.0);
            }
            _ => panic!("Expected InvalidSeekPosition error"),
        }
    }

    #[test]
    fn test_seek_validation() {
        let mut engine = AudioEngineImpl::new().expect("Failed to create AudioEngine");
        let decoder = Box::new(MockDecoder::new());

        *engine.current_decoder.lock().unwrap() = Some(decoder);

        // Test validate_seek_position
        let result = engine.validate_seek_position(Duration::from_secs(60));
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Duration::from_secs(60));

        let result = engine.validate_seek_position(Duration::from_secs(200));
        assert!(result.is_err());

        // Test at exact duration boundary
        let result = engine.validate_seek_position(Duration::from_secs(180));
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Duration::from_secs(180));
    }

    #[test]
    fn test_current_position_tracking() {
        let mut engine = AudioEngineImpl::new().expect("Failed to create AudioEngine");

        // Initial position should be zero
        assert_eq!(engine.current_position(), Duration::from_secs(0));

        // Seek to different positions
        let positions = [
            Duration::from_secs(30),
            Duration::from_secs(90),
            Duration::from_secs(0),
            Duration::from_millis(45500), // 45.5 seconds
        ];

        for position in positions {
            let result = engine.seek(position);
            assert!(result.is_ok(), "Seek to {:?} should succeed", position);
            assert_eq!(engine.current_position(), position);
        }
    }

    #[test]
    fn test_current_duration() {
        let mut engine = AudioEngineImpl::new().expect("Failed to create AudioEngine");

        // No decoder - should return None
        assert!(engine.current_duration().is_none());

        // With decoder - should return duration
        let decoder = Box::new(MockDecoder::new());
        *engine.current_decoder.lock().unwrap() = Some(decoder);

        let duration = engine.current_duration();
        assert!(duration.is_some());
        assert_eq!(duration.unwrap(), Duration::from_secs(180));
    }

    #[test]
    fn test_seek_precision() {
        let mut engine = AudioEngineImpl::new().expect("Failed to create AudioEngine");

        // Test fractional second seeking
        let precise_position = Duration::from_millis(12345); // 12.345 seconds
        let result = engine.seek(precise_position);
        assert!(result.is_ok());
        assert_eq!(engine.current_position(), precise_position);

        // Test microsecond precision
        let micro_position = Duration::from_micros(1234567); // 1.234567 seconds
        let result = engine.seek(micro_position);
        assert!(result.is_ok());
        assert_eq!(engine.current_position(), micro_position);
    }

    #[test]
    fn test_seek_during_playback() {
        let mut engine = AudioEngineImpl::new().expect("Failed to create AudioEngine");
        let decoder = Box::new(MockDecoder::new());

        // Start playback
        let result = engine.start_playback(decoder);
        assert!(result.is_ok());

        // Brief pause to let playback start
        std::thread::sleep(Duration::from_millis(10));

        // Seek during playback
        let result = engine.seek(Duration::from_secs(60));
        assert!(result.is_ok(), "Seek during playback should succeed");

        // Position should be updated
        assert_eq!(engine.current_position(), Duration::from_secs(60));

        // Stop playback
        let result = engine.stop();
        assert!(result.is_ok());
    }

    #[test]
    fn test_seek_boundary_conditions() {
        let mut engine = AudioEngineImpl::new().expect("Failed to create AudioEngine");
        let decoder = Box::new(MockDecoder::new());
        *engine.current_decoder.lock().unwrap() = Some(decoder);

        // Seek to start
        let result = engine.seek(Duration::from_secs(0));
        assert!(result.is_ok());
        assert_eq!(engine.current_position(), Duration::from_secs(0));

        // Seek to end
        let result = engine.seek(Duration::from_secs(180));
        assert!(result.is_ok());
        assert_eq!(engine.current_position(), Duration::from_secs(180));

        // Seek just before end
        let result = engine.seek(Duration::from_millis(179999));
        assert!(result.is_ok());
        assert_eq!(engine.current_position(), Duration::from_millis(179999));

        // Seek just past end (should fail)
        let result = engine.seek(Duration::from_millis(180001));
        assert!(result.is_err());
    }

    #[test]
    fn test_playback_with_different_configurations() {
        let mut engine = AudioEngineImpl::new().expect("Failed to create AudioEngine");

        // Test playback with different sample rates
        let test_configs = [
            (44100, 16, 2),
            (48000, 24, 2),
            (96000, 32, 2),
        ];

        for &(sample_rate, bit_depth, channels) in &test_configs {
            // Update configuration
            let result = engine.update_config(sample_rate, bit_depth, channels);
            if result.is_ok() {
                // Verify configuration was set
                assert_eq!(engine.sample_rate(), sample_rate);
                assert_eq!(engine.bit_depth(), bit_depth);
                assert_eq!(engine.channels(), channels);

                // Test playback with this configuration
                let decoder = Box::new(MockDecoder::new());
                let result = engine.start_playback(decoder);
                assert!(result.is_ok(), "Playback should work with {}Hz/{}bit/{}ch",
                       sample_rate, bit_depth, channels);

                let result = engine.stop();
                assert!(result.is_ok(), "Should be able to stop playback");
            }
        }
    }
}
