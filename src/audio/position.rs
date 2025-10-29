use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::time::interval;
use crate::models::{PlayerStatus, PlaybackState};

/// Real-time position tracker for audio playback
#[derive(Debug, Clone)]
pub struct PositionTracker {
    inner: Arc<Mutex<PositionTrackerInner>>,
}

#[derive(Debug)]
struct PositionTrackerInner {
    /// Current playback position
    position: Duration,
    /// When the position was last updated
    last_update: Instant,
    /// Current playback state
    state: PlaybackState,
    /// Track duration for bounds checking
    duration: Duration,
    /// Whether position tracking is active
    active: bool,
}

impl PositionTracker {
    /// Create a new position tracker
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(PositionTrackerInner {
                position: Duration::from_secs(0),
                last_update: Instant::now(),
                state: PlaybackState::Stopped,
                duration: Duration::from_secs(0),
                active: false,
            })),
        }
    }

    /// Start tracking position for a new track
    pub fn start_tracking(&self, initial_position: Duration, duration: Duration) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.position = initial_position;
            inner.duration = duration;
            inner.last_update = Instant::now();
            inner.state = PlaybackState::Playing;
            inner.active = true;
        }
    }

    /// Stop position tracking
    pub fn stop_tracking(&self) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.active = false;
            inner.state = PlaybackState::Stopped;
            inner.position = Duration::from_secs(0);
        }
    }

    /// Pause position tracking
    pub fn pause(&self) {
        if let Ok(mut inner) = self.inner.lock() {
            // Update position before pausing
            if inner.state == PlaybackState::Playing {
                let elapsed = inner.last_update.elapsed();
                inner.position = inner.position.saturating_add(elapsed);
                inner.position = inner.position.min(inner.duration);
            }
            inner.state = PlaybackState::Paused;
            inner.last_update = Instant::now();
        }
    }

    /// Resume position tracking
    pub fn resume(&self) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.state = PlaybackState::Playing;
            inner.last_update = Instant::now();
        }
    }

    /// Seek to a specific position
    pub fn seek(&self, position: Duration) -> Result<(), String> {
        if let Ok(mut inner) = self.inner.lock() {
            // For zero-duration tracks, only allow seeking to position 0
            if inner.duration.as_secs() == 0 && position.as_secs() > 0 {
                return Err(format!(
                    "Cannot seek to position {:.2}s in zero-duration track",
                    position.as_secs_f64()
                ));
            }
            
            // For tracks with known duration, validate against it
            if inner.duration.as_secs() > 0 && position > inner.duration {
                return Err(format!(
                    "Seek position {:.2}s exceeds track duration {:.2}s",
                    position.as_secs_f64(),
                    inner.duration.as_secs_f64()
                ));
            }
            
            inner.position = position.min(inner.duration);
            inner.last_update = Instant::now();
            Ok(())
        } else {
            Err("Failed to acquire position tracker lock".to_string())
        }
    }

    /// Seek to a specific position with validation
    pub fn seek_validated(&self, position: Duration) -> Result<Duration, String> {
        if let Ok(inner) = self.inner.lock() {
            let duration = inner.duration;
            drop(inner); // Release lock before calling seek
            
            // For zero-duration tracks, only allow seeking to position 0
            if duration.as_secs() == 0 && position.as_secs() > 0 {
                return Err(format!(
                    "Cannot seek to position {:.2}s in zero-duration track",
                    position.as_secs_f64()
                ));
            }
            
            // For tracks with known duration, validate against it
            if duration.as_secs() > 0 && position > duration {
                return Err(format!(
                    "Seek position {:.2}s exceeds track duration {:.2}s",
                    position.as_secs_f64(),
                    duration.as_secs_f64()
                ));
            }
            
            let clamped_position = position.min(duration);
            self.seek(clamped_position)?;
            Ok(clamped_position)
        } else {
            Err("Failed to acquire position tracker lock".to_string())
        }
    }

    /// Get current position (calculated in real-time)
    pub fn current_position(&self) -> Duration {
        if let Ok(mut inner) = self.inner.lock() {
            if inner.state == PlaybackState::Playing && inner.active {
                let elapsed = inner.last_update.elapsed();
                inner.position = inner.position.saturating_add(elapsed);
                inner.position = inner.position.min(inner.duration);
                inner.last_update = Instant::now();
            }
            inner.position
        } else {
            Duration::from_secs(0)
        }
    }

    /// Get current playback state
    pub fn current_state(&self) -> PlaybackState {
        if let Ok(inner) = self.inner.lock() {
            inner.state
        } else {
            PlaybackState::Stopped
        }
    }

    /// Get track duration
    pub fn duration(&self) -> Duration {
        if let Ok(inner) = self.inner.lock() {
            inner.duration
        } else {
            Duration::from_secs(0)
        }
    }

    /// Check if tracking is active
    pub fn is_active(&self) -> bool {
        if let Ok(inner) = self.inner.lock() {
            inner.active
        } else {
            false
        }
    }

    /// Update the player status with current position
    pub fn update_status(&self, status: &mut PlayerStatus) {
        status.position = self.current_position();
        status.state = self.current_state();
    }

    /// Calculate progress as a percentage (0.0 to 1.0)
    pub fn progress(&self) -> f32 {
        let position = self.current_position();
        let duration = self.duration();
        
        if duration.as_secs() > 0 {
            position.as_secs_f32() / duration.as_secs_f32()
        } else {
            0.0
        }
    }

    /// Get remaining time
    pub fn remaining_time(&self) -> Duration {
        let position = self.current_position();
        let duration = self.duration();
        duration.saturating_sub(position)
    }

    /// Check if playback has reached the end
    pub fn is_finished(&self) -> bool {
        let position = self.current_position();
        let duration = self.duration();
        
        duration.as_secs() > 0 && position >= duration
    }

    /// Start a background task for periodic position updates
    pub async fn start_update_task(
        &self,
        mut status_callback: impl FnMut(Duration, PlaybackState) + Send + 'static,
    ) {
        let tracker = self.clone();
        let mut interval = interval(Duration::from_millis(100)); // Update every 100ms
        
        tokio::spawn(async move {
            loop {
                interval.tick().await;
                
                if !tracker.is_active() {
                    tokio::time::sleep(Duration::from_millis(500)).await;
                    continue;
                }
                
                let position = tracker.current_position();
                let state = tracker.current_state();
                
                status_callback(position, state);
                
                // Check if playback finished
                if tracker.is_finished() && state == PlaybackState::Playing {
                    // Notify that track finished
                    status_callback(tracker.duration(), PlaybackState::Stopped);
                    break;
                }
            }
        });
    }
}

impl Default for PositionTracker {
    fn default() -> Self {
        Self::new()
    }
}

/// Position update event for callbacks
#[derive(Debug, Clone)]
pub struct PositionUpdate {
    pub position: Duration,
    pub state: PlaybackState,
    pub progress: f32,
    pub remaining: Duration,
}

impl PositionUpdate {
    pub fn new(position: Duration, state: PlaybackState, duration: Duration) -> Self {
        let progress = if duration.as_secs() > 0 {
            position.as_secs_f32() / duration.as_secs_f32()
        } else {
            0.0
        };
        
        let remaining = duration.saturating_sub(position);
        
        Self {
            position,
            state,
            progress,
            remaining,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::{sleep, Duration as TokioDuration};

    #[test]
    fn test_position_tracker_creation() {
        let tracker = PositionTracker::new();
        
        assert_eq!(tracker.current_position(), Duration::from_secs(0));
        assert_eq!(tracker.current_state(), PlaybackState::Stopped);
        assert_eq!(tracker.duration(), Duration::from_secs(0));
        assert!(!tracker.is_active());
    }

    #[test]
    fn test_start_tracking() {
        let tracker = PositionTracker::new();
        let initial_position = Duration::from_secs(30);
        let duration = Duration::from_secs(180);
        
        tracker.start_tracking(initial_position, duration);
        
        assert_eq!(tracker.current_state(), PlaybackState::Playing);
        assert_eq!(tracker.duration(), duration);
        assert!(tracker.is_active());
        
        // Position should be close to initial position (may have small elapsed time)
        let current_pos = tracker.current_position();
        assert!(current_pos >= initial_position);
        assert!(current_pos <= initial_position + Duration::from_millis(100));
    }

    #[test]
    fn test_pause_and_resume() {
        let tracker = PositionTracker::new();
        tracker.start_tracking(Duration::from_secs(0), Duration::from_secs(180));
        
        // Let it play for a bit
        std::thread::sleep(std::time::Duration::from_millis(50));
        
        tracker.pause();
        assert_eq!(tracker.current_state(), PlaybackState::Paused);
        
        let paused_position = tracker.current_position();
        
        // Wait while paused - position shouldn't change
        std::thread::sleep(std::time::Duration::from_millis(50));
        assert_eq!(tracker.current_position(), paused_position);
        
        tracker.resume();
        assert_eq!(tracker.current_state(), PlaybackState::Playing);
    }

    #[test]
    fn test_seek() {
        let tracker = PositionTracker::new();
        tracker.start_tracking(Duration::from_secs(0), Duration::from_secs(180));
        
        let seek_position = Duration::from_secs(60);
        let result = tracker.seek(seek_position);
        assert!(result.is_ok());
        
        let current_pos = tracker.current_position();
        assert!(current_pos >= seek_position);
        assert!(current_pos <= seek_position + Duration::from_millis(100));
    }

    #[test]
    fn test_seek_beyond_duration() {
        let tracker = PositionTracker::new();
        let duration = Duration::from_secs(180);
        tracker.start_tracking(Duration::from_secs(0), duration);
        
        // Try to seek beyond duration - should return error
        let result = tracker.seek(Duration::from_secs(300));
        assert!(result.is_err());
        
        // Position should remain unchanged
        let current_pos = tracker.current_position();
        assert!(current_pos <= Duration::from_secs(1)); // Should be close to start
    }

    #[test]
    fn test_seek_validated() {
        let tracker = PositionTracker::new();
        let duration = Duration::from_secs(180);
        tracker.start_tracking(Duration::from_secs(0), duration);
        
        // Test valid seek
        let result = tracker.seek_validated(Duration::from_secs(60));
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Duration::from_secs(60));
        
        // Test seek beyond duration
        let result = tracker.seek_validated(Duration::from_secs(300));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("exceeds track duration"));
        
        // Test seek at exact duration
        let result = tracker.seek_validated(duration);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), duration);
    }

    #[test]
    fn test_seek_precision() {
        let tracker = PositionTracker::new();
        tracker.start_tracking(Duration::from_secs(0), Duration::from_secs(180));
        
        // Test fractional second seeking
        let seek_position = Duration::from_millis(30500); // 30.5 seconds
        let result = tracker.seek(seek_position);
        assert!(result.is_ok());
        
        let current_pos = tracker.current_position();
        assert!(current_pos >= seek_position);
        assert!(current_pos <= seek_position + Duration::from_millis(100));
    }

    #[test]
    fn test_seek_zero_duration() {
        let tracker = PositionTracker::new();
        tracker.start_tracking(Duration::from_secs(0), Duration::from_secs(0));
        
        // Seeking in zero-duration track should work for position 0
        let result = tracker.seek(Duration::from_secs(0));
        assert!(result.is_ok());
        
        // But not for any positive position
        let result = tracker.seek(Duration::from_secs(1));
        assert!(result.is_err());
    }

    #[test]
    fn test_seek_during_playback_states() {
        let tracker = PositionTracker::new();
        tracker.start_tracking(Duration::from_secs(0), Duration::from_secs(180));
        
        // Seek while playing
        assert_eq!(tracker.current_state(), PlaybackState::Playing);
        let result = tracker.seek(Duration::from_secs(60));
        assert!(result.is_ok());
        assert_eq!(tracker.current_state(), PlaybackState::Playing); // State should remain
        
        // Seek while paused
        tracker.pause();
        assert_eq!(tracker.current_state(), PlaybackState::Paused);
        let result = tracker.seek(Duration::from_secs(90));
        assert!(result.is_ok());
        assert_eq!(tracker.current_state(), PlaybackState::Paused); // State should remain
        
        // Seek while stopped
        tracker.stop_tracking();
        assert_eq!(tracker.current_state(), PlaybackState::Stopped);
        let result = tracker.seek(Duration::from_secs(30));
        assert!(result.is_ok()); // Should still work even when stopped
    }

    #[test]
    fn test_stop_tracking() {
        let tracker = PositionTracker::new();
        tracker.start_tracking(Duration::from_secs(30), Duration::from_secs(180));
        
        assert!(tracker.is_active());
        
        tracker.stop_tracking();
        
        assert!(!tracker.is_active());
        assert_eq!(tracker.current_state(), PlaybackState::Stopped);
        assert_eq!(tracker.current_position(), Duration::from_secs(0));
    }

    #[test]
    fn test_progress_calculation() {
        let tracker = PositionTracker::new();
        let duration = Duration::from_secs(100);
        tracker.start_tracking(Duration::from_secs(25), duration);
        
        let progress = tracker.progress();
        assert!((progress - 0.25).abs() < 0.01); // Should be approximately 25%
        
        tracker.seek(Duration::from_secs(50));
        let progress = tracker.progress();
        assert!((progress - 0.5).abs() < 0.01); // Should be approximately 50%
    }

    #[test]
    fn test_remaining_time() {
        let tracker = PositionTracker::new();
        let duration = Duration::from_secs(180);
        tracker.start_tracking(Duration::from_secs(60), duration);
        
        let remaining = tracker.remaining_time();
        assert!(remaining <= Duration::from_secs(120));
        assert!(remaining >= Duration::from_secs(119)); // Account for small elapsed time
    }

    #[test]
    fn test_is_finished() {
        let tracker = PositionTracker::new();
        let duration = Duration::from_secs(100);
        tracker.start_tracking(Duration::from_secs(0), duration);
        
        assert!(!tracker.is_finished());
        
        tracker.seek(duration);
        assert!(tracker.is_finished());
    }

    #[test]
    fn test_update_status() {
        let tracker = PositionTracker::new();
        tracker.start_tracking(Duration::from_secs(30), Duration::from_secs(180));
        
        let mut status = PlayerStatus::new();
        tracker.update_status(&mut status);
        
        assert_eq!(status.state, PlaybackState::Playing);
        assert!(status.position >= Duration::from_secs(30));
        assert!(status.position <= Duration::from_secs(31)); // Small tolerance for elapsed time
    }

    #[test]
    fn test_position_update_creation() {
        let position = Duration::from_secs(60);
        let state = PlaybackState::Playing;
        let duration = Duration::from_secs(180);
        
        let update = PositionUpdate::new(position, state, duration);
        
        assert_eq!(update.position, position);
        assert_eq!(update.state, state);
        assert!((update.progress - (1.0/3.0)).abs() < 0.01); // 60/180 = 1/3
        assert_eq!(update.remaining, Duration::from_secs(120));
    }

    #[tokio::test]
    async fn test_position_tracking_over_time() {
        let tracker = PositionTracker::new();
        tracker.start_tracking(Duration::from_secs(0), Duration::from_secs(10));
        
        let initial_position = tracker.current_position();
        
        // Wait for some time
        sleep(TokioDuration::from_millis(100)).await;
        
        let later_position = tracker.current_position();
        
        // Position should have advanced
        assert!(later_position > initial_position);
        assert!(later_position <= Duration::from_millis(150)); // Should be reasonable
    }

    #[test]
    fn test_concurrent_access() {
        use std::sync::Arc;
        use std::thread;
        
        let tracker = Arc::new(PositionTracker::new());
        tracker.start_tracking(Duration::from_secs(0), Duration::from_secs(100));
        
        let tracker_clone = tracker.clone();
        let handle = thread::spawn(move || {
            for i in 0..10 {
                tracker_clone.seek(Duration::from_secs(i * 10));
                thread::sleep(std::time::Duration::from_millis(10));
            }
        });
        
        // Access tracker from main thread while other thread is modifying
        for _ in 0..10 {
            let _position = tracker.current_position();
            let _progress = tracker.progress();
            thread::sleep(std::time::Duration::from_millis(10));
        }
        
        handle.join().unwrap();
        
        // Should not panic and should have reasonable final state
        assert!(tracker.current_position() <= Duration::from_secs(100));
    }

    #[test]
    fn test_zero_duration_handling() {
        let tracker = PositionTracker::new();
        tracker.start_tracking(Duration::from_secs(0), Duration::from_secs(0));
        
        assert_eq!(tracker.progress(), 0.0);
        assert_eq!(tracker.remaining_time(), Duration::from_secs(0));
        // Zero duration should not be considered finished unless explicitly at the end
        // This is because zero duration means unknown duration, not a finished track
        assert!(!tracker.is_finished());
    }
}