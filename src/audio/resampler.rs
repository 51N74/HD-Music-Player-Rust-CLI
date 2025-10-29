/*!
A simple, streaming, linear resampler for interleaved f32 audio.

- Converts from an input (source) sample rate to an output (destination) sample rate.
- Operates on interleaved frames (LRLR...) for an arbitrary number of channels.
- Maintains streaming continuity across successive process() calls (no clicks at chunk boundaries).
- Zero external dependencies; uses linear interpolation (good quality and minimal CPU use).

Typical usage:

    use crate::audio::resampler::LinearResampler;

    let mut rs = LinearResampler::new(44_100, 48_000, 2);
    let output = rs.process(&input_interleaved_f32);
    // 'output' is now at 48kHz with the same channel count.

If you work with the project's AudioBuffer type:

    use crate::models::AudioBuffer;
    let out_buf = rs.process_audio_buffer(&in_buf);

Notes:
- If your output device forces a fixed sample rate (e.g. 48 kHz), you can instantiate this with
  src_rate = decoded file sample rate, dst_rate = device stream rate, and pass the decoded f32
  blocks through process() before writing to the ring buffer.
*/

use std::cmp::min;

#[derive(Debug, Clone)]
pub struct LinearResampler {
    src_rate: u32,
    dst_rate: u32,
    channels: usize,

    // Derived
    step: f64, // how many source frames per 1 output frame (src/dst)

    // Streaming state
    pos: f64,              // current source position (in frames) relative to the start of 'prev' frame
    prev_frame: Vec<f32>,  // last source frame from the previous call, length == channels
}

impl LinearResampler {
    /// Create a new resampler.
    /// - src_rate: source/decoded sample rate (Hz)
    /// - dst_rate: destination/output sample rate (Hz)
    /// - channels: number of interleaved channels (e.g., 1 mono, 2 stereo)
    pub fn new(src_rate: u32, dst_rate: u32, channels: usize) -> Self {
        let step = if dst_rate == 0 { 0.0 } else { src_rate as f64 / dst_rate as f64 };
        Self {
            src_rate,
            dst_rate,
            channels,
            step,
            pos: 0.0,
            prev_frame: Vec::new(),
        }
    }

    /// Reset the streaming state (phase and history).
    pub fn reset(&mut self) {
        self.pos = 0.0;
        self.prev_frame.clear();
    }

    /// Change the source and destination rates. Keeps the streaming phase unless `reset_state` is true.
    pub fn set_rates(&mut self, src_rate: u32, dst_rate: u32, reset_state: bool) {
        self.src_rate = src_rate;
        self.dst_rate = dst_rate;
        self.step = if dst_rate == 0 { 0.0 } else { src_rate as f64 / dst_rate as f64 };
        if reset_state {
            self.reset();
        }
    }

    /// Change channel count. Keeps state unless `reset_state` is true.
    pub fn set_channels(&mut self, channels: usize, reset_state: bool) {
        self.channels = channels;
        if reset_state {
            self.reset();
        } else {
            // Ensure prev_frame length matches channels
            self.prev_frame.resize(self.channels, 0.0);
        }
    }

    /// Get current configuration.
    pub fn config(&self) -> (u32, u32, usize) {
        (self.src_rate, self.dst_rate, self.channels)
    }

    /// Resample interleaved f32 samples from src_rate to dst_rate, preserving state across calls.
    ///
    /// Input:
    /// - `input` is &[f32] with interleaved channels, length is frames * channels.
    ///
    /// Output:
    /// - Vec<f32> at dst_rate with the same number of channels, interleaved.
    ///
    /// This function is streaming-safe: call it repeatedly with sequential input blocks and
    /// it produces a continuous resampled output without audible discontinuities.
    pub fn process(&mut self, input: &[f32]) -> Vec<f32> {
        if self.channels == 0 || self.dst_rate == 0 || self.src_rate == 0 {
            return Vec::new();
        }

        let ch = self.channels;
        let in_frames = input.len() / ch;

        // Build working buffer with a 1-frame prefix from the previous call to allow interpolation
        // across chunk boundaries: [prev_frame, input_frames...].
        let mut work = Vec::with_capacity((in_frames + 1) * ch);
        if self.prev_frame.len() == ch {
            work.extend_from_slice(&self.prev_frame);
        } else if in_frames > 0 {
            // If no prev frame but we have input, synthesize a zero frame (avoids special cases).
            work.extend(std::iter::repeat(0.0).take(ch));
        } else {
            // No prev and no input: nothing to do.
            return Vec::new();
        }
        work.extend_from_slice(input);

        let total_frames = work.len() / ch;

        // Estimate output size to reduce reallocations.
        // Expected out_frames ≈ in_frames * (dst/src). Add a small margin.
        let expected_out_frames = ((in_frames as f64) * (self.dst_rate as f64 / self.src_rate as f64)).ceil() as usize + 4;
        let mut out = Vec::with_capacity(expected_out_frames * ch);

        // Generate output frames until we don't have at least (i+1) frames for interpolation.
        // We require (pos + 1.0) < total_frames => pos < total_frames - 1
        while self.pos + 1.0 <= (total_frames as f64 - 1.0) {
            let i = self.pos.floor() as usize;
            let frac = (self.pos - i as f64) as f32;

            let base0 = i * ch;
            let base1 = (i + 1) * ch;

            // Linear interpolation per channel
            out.extend((0..ch).map(|c| {
                let s0 = work[base0 + c];
                let s1 = work[base1 + c];
                s0 + (s1 - s0) * frac
            }));

            self.pos += self.step;
        }

        // Prepare state for next call:
        // Keep the last frame of 'work' as prev_frame; shift pos so that next time
        // pos is relative to that last frame (which will be prepended at index 0).
        if total_frames > 0 {
            let last_base = (total_frames - 1) * ch;
            if self.prev_frame.len() != ch {
                self.prev_frame.resize(ch, 0.0);
            }
            for c in 0..ch {
                self.prev_frame[c] = work[last_base + c];
            }

            // Translate position so that the last frame becomes index 0 next time.
            // New pos = old pos - (total_frames - 1)
            let shift = (total_frames as f64 - 1.0).max(0.0);
            self.pos -= shift;
            if self.pos < 0.0 {
                // Numerical safety
                self.pos = 0.0;
            }
        } else {
            self.prev_frame.clear();
            self.pos = 0.0;
        }

        out
    }

    /// Convenience: resample an AudioBuffer (from this project) to the destination rate.
    /// Preserves channel count, updates sample_rate and frames.
    pub fn process_audio_buffer(&mut self, input: &crate::models::AudioBuffer) -> crate::models::AudioBuffer {
        if input.channels as usize != self.channels {
            // Simple channel guard: if mismatch, do a best-effort (truncate or pad) to match
            // the resampler's channel count before processing.
            let ch_in = input.channels as usize;
            let ch_out = self.channels;
            let frames = input.frames;

            let mut remapped = Vec::with_capacity(frames * ch_out);
            if ch_out <= ch_in {
                // Truncate extra channels
                for f in 0..frames {
                    let base = f * ch_in;
                    remapped.extend_from_slice(&input.samples[base..base + ch_out]);
                }
            } else {
                // Copy available channels and pad the rest with silence
                for f in 0..frames {
                    let base = f * ch_in;
                    remapped.extend_from_slice(&input.samples[base..base + ch_in]);
                    remapped.extend(std::iter::repeat(0.0).take(ch_out - ch_in));
                }
            }
            let tmp = crate::models::AudioBuffer {
                samples: remapped,
                channels: ch_out as u16,
                sample_rate: input.sample_rate,
                frames,
            };
            return self.process_audio_buffer(&tmp);
        }

        self.set_rates(input.sample_rate, self.dst_rate, false);

        let out_samples = self.process(&input.samples);
        let out_frames = out_samples.len() / self.channels;
        crate::models::AudioBuffer {
            samples: out_samples,
            channels: self.channels as u16,
            sample_rate: self.dst_rate,
            frames: out_frames,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gen_sine(f_hz: f32, sr: u32, frames: usize, ch: usize) -> Vec<f32> {
        let mut out = Vec::with_capacity(frames * ch);
        for n in 0..frames {
            let t = n as f32 / sr as f32;
            let s = (2.0 * std::f32::consts::PI * f_hz * t).sin();
            for _ in 0..ch {
                out.push(s);
            }
        }
        out
    }

    #[test]
    fn resample_length_mono_44k1_to_48k() {
        let src = 44_100;
        let dst = 48_000;
        let ch = 1usize;

        let in_frames = 4410; // 0.1s
        let input = gen_sine(1000.0, src, in_frames, ch);

        let mut rs = LinearResampler::new(src, dst, ch);
        let out = rs.process(&input);

        let out_frames = out.len() / ch;
        let expected = (in_frames as f64 * (dst as f64 / src as f64)).round() as isize;
        let actual = out_frames as isize;
        assert!((actual - expected).abs() <= 2, "expected ~{}, got {}", expected, actual);
    }

    #[test]
    fn streaming_consistency_split_buffers() {
        let src = 44_100;
        let dst = 48_000;
        let ch = 2usize;

        let in_frames = 10_000;
        let input = gen_sine(440.0, src, in_frames, ch);

        // One-shot
        let mut one = LinearResampler::new(src, dst, ch);
        let out_one = one.process(&input);

        // Split into chunks and stream
        let mut two = LinearResampler::new(src, dst, ch);
        let mut out_streamed = Vec::new();

        let mut idx = 0usize;
        let chunk_frames = 777; // odd chunk size to exercise boundaries
        while idx < in_frames {
            let remain = in_frames - idx;
            let take_frames = min(remain, chunk_frames);
            let base = idx * ch;
            let end = base + take_frames * ch;
            let piece = &input[base..end];
            let block = two.process(piece);
            out_streamed.extend_from_slice(&block);
            idx += take_frames;
        }

        // Allow small tolerance for last frame rounding differences
        assert!((out_one.len() as isize - out_streamed.len() as isize).abs() <= 4);

        // Spot check a few samples within range (up to min length)
        let common = min(out_one.len(), out_streamed.len());
        let step = common / 10;
        for k in (0..common).step_by(step.max(1)) {
            let a = out_one[k];
            let b = out_streamed[k];
            assert!((a - b).abs() < 1e-3, "mismatch at {}, {} vs {}", k, a, b);
        }
    }

    #[test]
    fn resample_stereo_no_crash() {
        let src = 48_000;
        let dst = 44_100;
        let ch = 2usize;

        let in_frames = 5000;
        let input = gen_sine(880.0, src, in_frames, ch);
        let mut rs = LinearResampler::new(src, dst, ch);
        let _out = rs.process(&input);
    }
}
