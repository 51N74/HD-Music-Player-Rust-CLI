use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::fs;
use std::time::Duration;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use symphonia::default::get_probe;
use crate::error::{QueueError, PlaylistError};
use crate::models::{TrackInfo, AudioMetadata, AudioCodec};
use crate::queue::playlist::{PlaylistManager, PlaylistFormat};

/// Core trait for queue management functionality
pub trait QueueManager: Send {
    /// Add a file to the queue
    fn add_file(&mut self, path: &Path) -> Result<(), QueueError>;
    
    /// Add all audio files from a directory recursively
    fn add_directory(&mut self, path: &Path) -> Result<(), QueueError>;
    
    /// Get the next track in the queue
    fn next_track(&mut self) -> Option<&TrackInfo>;
    
    /// Get the previous track in the queue
    fn previous_track(&mut self) -> Option<&TrackInfo>;
    
    /// Clear all tracks from the queue
    fn clear(&mut self);
    
    /// Get the current queue as a list
    fn list(&self) -> &VecDeque<TrackInfo>;
    
    /// Get the current track index
    fn current_index(&self) -> usize;
    
    /// Get the current track (if any)
    fn current_track(&self) -> Option<&TrackInfo>;
    
    /// Get the total number of tracks in the queue
    fn len(&self) -> usize;
    
    /// Check if the queue is empty
    fn is_empty(&self) -> bool;
    
    /// Remove a track at the specified index
    fn remove(&mut self, index: usize) -> Result<TrackInfo, QueueError>;
    
    /// Jump to a specific track by index
    fn jump_to(&mut self, index: usize) -> Result<&TrackInfo, QueueError>;
    
    /// Save the current queue as a playlist
    fn save_playlist(&self, name: &str, format: PlaylistFormat) -> Result<(), PlaylistError>;
    
    /// Load a playlist into the current queue
    fn load_playlist(&mut self, name: &str) -> Result<(), PlaylistError>;
    
    /// List available playlists
    fn list_playlists(&self) -> Result<Vec<String>, PlaylistError>;
    
    /// Delete a playlist
    fn delete_playlist(&self, name: &str) -> Result<(), PlaylistError>;
}

pub mod playlist;

/// Queue manager implementation with VecDeque for efficient queue operations
pub struct QueueManagerImpl {
    current_queue: VecDeque<TrackInfo>,
    current_index: usize,
    playlist_manager: PlaylistManager,
}

impl QueueManagerImpl {
    pub fn new() -> Self {
        // Use default playlist directory in user's home config
        let playlist_dir = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("hires-player")
            .join("playlists");
        
        let playlist_manager = PlaylistManager::new(playlist_dir)
            .expect("Failed to create playlist manager");
        
        Self {
            current_queue: VecDeque::new(),
            current_index: 0,
            playlist_manager,
        }
    }
    
    pub fn with_playlist_directory(playlist_dir: PathBuf) -> Result<Self, PlaylistError> {
        let playlist_manager = PlaylistManager::new(playlist_dir)?;
        
        Ok(Self {
            current_queue: VecDeque::new(),
            current_index: 0,
            playlist_manager,
        })
    }

    /// Check if a file extension is supported
    fn is_supported_format(extension: &str) -> bool {
        let ext = extension.to_lowercase();
        matches!(
            ext.as_str(),
            "flac" | "wav" | "wave" | "m4a" | "alac" | "mp3" | "ogg" | "oga"
        )
    }

    /// Get the audio codec from file extension
    fn codec_from_extension(extension: &str) -> Option<AudioCodec> {
        let ext = extension.to_lowercase();
        match ext.as_str() {
            "flac" => Some(AudioCodec::Flac),
            "wav" | "wave" => Some(AudioCodec::Wav),
            "m4a" | "alac" => Some(AudioCodec::Alac),
            "mp3" => Some(AudioCodec::Mp3),
            "ogg" | "oga" => Some(AudioCodec::OggVorbis),
            _ => None,
        }
    }

    /// Extract metadata and create TrackInfo from a file path
    pub fn create_track_info(path: &Path) -> Result<TrackInfo, QueueError> {
        // Check if file exists
        if !path.exists() {
            return Err(QueueError::FileNotFound {
                path: path.to_string_lossy().to_string(),
            });
        }

        // Check if it's a supported format
        let extension = path
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("");

        if !Self::is_supported_format(extension) {
            return Err(QueueError::InvalidFormat {
                path: path.to_string_lossy().to_string(),
            });
        }

        // Get file size
        let file_size = fs::metadata(path)
            .map_err(|_| QueueError::FileNotFound {
                path: path.to_string_lossy().to_string(),
            })?
            .len();

        // Try to extract metadata using symphonia
        let (metadata, duration) = Self::extract_metadata_and_duration(path)
            .unwrap_or_else(|_| {
                // Fallback to basic metadata if extraction fails
                let mut basic_metadata = AudioMetadata::new();
                if let Some(filename) = path.file_stem().and_then(|s| s.to_str()) {
                    basic_metadata.title = Some(filename.to_string());
                }
                (basic_metadata, Duration::from_secs(0))
            });

        Ok(TrackInfo::new(path.to_path_buf(), metadata, duration, file_size))
    }

    /// Extract metadata and duration using symphonia
    fn extract_metadata_and_duration(path: &Path) -> Result<(AudioMetadata, Duration), Box<dyn std::error::Error>> {
        let file = std::fs::File::open(path)?;
        let mss = MediaSourceStream::new(Box::new(file), Default::default());

        let mut hint = Hint::new();
        if let Some(extension) = path.extension().and_then(|ext| ext.to_str()) {
            hint.with_extension(extension);
        }

        let meta_opts: MetadataOptions = Default::default();
        let fmt_opts = symphonia::core::formats::FormatOptions::default();

        let probed = get_probe().format(&hint, mss, &fmt_opts, &meta_opts)?;
        let mut format = probed.format;

        let mut metadata = AudioMetadata::new();
        let mut duration = Duration::from_secs(0);

        // Extract metadata from the format
        if let Some(metadata_rev) = format.metadata().current() {
            for tag in metadata_rev.tags() {
                match tag.key.as_str() {
                    "TITLE" | "TIT2" => metadata.title = Some(tag.value.to_string()),
                    "ARTIST" | "TPE1" => metadata.artist = Some(tag.value.to_string()),
                    "ALBUM" | "TALB" => metadata.album = Some(tag.value.to_string()),
                    "TRACKNUMBER" | "TRCK" => {
                        if let Ok(track_num) = tag.value.to_string().parse::<u32>() {
                            metadata.track_number = Some(track_num);
                        }
                    }
                    "DATE" | "YEAR" | "TYER" => {
                        if let Ok(year) = tag.value.to_string().parse::<u32>() {
                            metadata.year = Some(year);
                        }
                    }
                    "GENRE" | "TCON" => metadata.genre = Some(tag.value.to_string()),
                    _ => {}
                }
            }
        }

        // Try to get duration from the first track
        if let Some(track) = format.tracks().first() {
            if let Some(time_base) = track.codec_params.time_base {
                if let Some(n_frames) = track.codec_params.n_frames {
                    let seconds = (n_frames as f64) * time_base.numer as f64 / time_base.denom as f64;
                    duration = Duration::from_secs_f64(seconds);
                }
            }
        }

        Ok((metadata, duration))
    }

    /// Recursively scan directory for audio files
    fn scan_directory(dir: &Path) -> Result<Vec<PathBuf>, QueueError> {
        let mut audio_files = Vec::new();

        if !dir.is_dir() {
            return Err(QueueError::FileNotFound {
                path: dir.to_string_lossy().to_string(),
            });
        }

        let entries = fs::read_dir(dir).map_err(|_| QueueError::FileNotFound {
            path: dir.to_string_lossy().to_string(),
        })?;

        for entry in entries {
            let entry = entry.map_err(|_| QueueError::FileNotFound {
                path: dir.to_string_lossy().to_string(),
            })?;

            let path = entry.path();

            if path.is_dir() {
                // Recursively scan subdirectories
                let mut sub_files = Self::scan_directory(&path)?;
                audio_files.append(&mut sub_files);
            } else if path.is_file() {
                // Check if it's a supported audio file
                if let Some(extension) = path.extension().and_then(|ext| ext.to_str()) {
                    if Self::is_supported_format(extension) {
                        audio_files.push(path);
                    }
                }
            }
        }

        // Sort files for consistent ordering
        audio_files.sort();
        Ok(audio_files)
    }
}

impl Default for QueueManagerImpl {
    fn default() -> Self {
        Self::new()
    }
}

impl QueueManager for QueueManagerImpl {
    fn add_file(&mut self, path: &Path) -> Result<(), QueueError> {
        let track_info = Self::create_track_info(path)?;
        self.current_queue.push_back(track_info);
        Ok(())
    }

    fn add_directory(&mut self, path: &Path) -> Result<(), QueueError> {
        let audio_files = Self::scan_directory(path)?;
        
        for file_path in audio_files {
            // Try to add each file, but don't fail the entire operation if one file fails
            if let Ok(track_info) = Self::create_track_info(&file_path) {
                self.current_queue.push_back(track_info);
            }
        }
        
        Ok(())
    }

    fn next_track(&mut self) -> Option<&TrackInfo> {
        if self.current_queue.is_empty() {
            return None;
        }

        if self.current_index + 1 < self.current_queue.len() {
            self.current_index += 1;
        } else {
            // Wrap around to the beginning
            self.current_index = 0;
        }

        self.current_queue.get(self.current_index)
    }

    fn previous_track(&mut self) -> Option<&TrackInfo> {
        if self.current_queue.is_empty() {
            return None;
        }

        if self.current_index > 0 {
            self.current_index -= 1;
        } else {
            // Wrap around to the end
            self.current_index = self.current_queue.len() - 1;
        }

        self.current_queue.get(self.current_index)
    }

    fn clear(&mut self) {
        self.current_queue.clear();
        self.current_index = 0;
    }

    fn list(&self) -> &VecDeque<TrackInfo> {
        &self.current_queue
    }

    fn current_index(&self) -> usize {
        self.current_index
    }

    fn current_track(&self) -> Option<&TrackInfo> {
        self.current_queue.get(self.current_index)
    }

    fn len(&self) -> usize {
        self.current_queue.len()
    }

    fn is_empty(&self) -> bool {
        self.current_queue.is_empty()
    }

    fn remove(&mut self, index: usize) -> Result<TrackInfo, QueueError> {
        if index >= self.current_queue.len() {
            return Err(QueueError::InvalidIndex { index });
        }

        let removed_track = self.current_queue.remove(index)
            .ok_or(QueueError::InvalidIndex { index })?;

        // Adjust current index if necessary
        if index < self.current_index {
            self.current_index -= 1;
        } else if index == self.current_index && self.current_index >= self.current_queue.len() {
            // If we removed the current track and it was the last one, move to the previous track
            if !self.current_queue.is_empty() {
                self.current_index = self.current_queue.len() - 1;
            } else {
                self.current_index = 0;
            }
        }

        Ok(removed_track)
    }

    fn jump_to(&mut self, index: usize) -> Result<&TrackInfo, QueueError> {
        if index >= self.current_queue.len() {
            return Err(QueueError::InvalidIndex { index });
        }

        self.current_index = index;
        Ok(self.current_queue.get(self.current_index).unwrap())
    }
    
    fn save_playlist(&self, name: &str, format: PlaylistFormat) -> Result<(), PlaylistError> {
        self.playlist_manager.save_playlist(name, &self.current_queue, format)
    }
    
    fn load_playlist(&mut self, name: &str) -> Result<(), PlaylistError> {
        let loaded_queue = self.playlist_manager.load_playlist(name)?;
        self.current_queue = loaded_queue;
        self.current_index = 0;
        Ok(())
    }
    
    fn list_playlists(&self) -> Result<Vec<String>, PlaylistError> {
        self.playlist_manager.list_playlists()
    }
    
    fn delete_playlist(&self, name: &str) -> Result<(), PlaylistError> {
        self.playlist_manager.delete_playlist(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn create_test_audio_file(dir: &Path, name: &str, extension: &str) -> PathBuf {
        let file_path = dir.join(format!("{}.{}", name, extension));
        // Create a dummy file (not a real audio file, but sufficient for testing file operations)
        fs::write(&file_path, b"dummy audio data").unwrap();
        file_path
    }

    fn create_test_directory_structure() -> TempDir {
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path();

        // Create some test audio files
        create_test_audio_file(root, "song1", "flac");
        create_test_audio_file(root, "song2", "mp3");
        create_test_audio_file(root, "song3", "wav");
        
        // Create a subdirectory with more files
        let subdir = root.join("subdir");
        fs::create_dir(&subdir).unwrap();
        create_test_audio_file(&subdir, "song4", "ogg");
        create_test_audio_file(&subdir, "song5", "m4a");
        
        // Create a non-audio file (should be ignored)
        fs::write(root.join("readme.txt"), b"This is not an audio file").unwrap();

        temp_dir
    }

    #[test]
    fn test_queue_manager_creation() {
        let temp_dir = TempDir::new().unwrap();
        let queue_manager = QueueManagerImpl::with_playlist_directory(temp_dir.path().to_path_buf()).unwrap();
        assert!(queue_manager.is_empty());
        assert_eq!(queue_manager.len(), 0);
        assert_eq!(queue_manager.current_index(), 0);
        assert!(queue_manager.current_track().is_none());
    }

    #[test]
    fn test_is_supported_format() {
        assert!(QueueManagerImpl::is_supported_format("flac"));
        assert!(QueueManagerImpl::is_supported_format("FLAC"));
        assert!(QueueManagerImpl::is_supported_format("wav"));
        assert!(QueueManagerImpl::is_supported_format("wave"));
        assert!(QueueManagerImpl::is_supported_format("mp3"));
        assert!(QueueManagerImpl::is_supported_format("m4a"));
        assert!(QueueManagerImpl::is_supported_format("alac"));
        assert!(QueueManagerImpl::is_supported_format("ogg"));
        assert!(QueueManagerImpl::is_supported_format("oga"));

        assert!(!QueueManagerImpl::is_supported_format("txt"));
        assert!(!QueueManagerImpl::is_supported_format("pdf"));
        assert!(!QueueManagerImpl::is_supported_format("jpg"));
        assert!(!QueueManagerImpl::is_supported_format(""));
    }

    #[test]
    fn test_codec_from_extension() {
        assert_eq!(QueueManagerImpl::codec_from_extension("flac"), Some(AudioCodec::Flac));
        assert_eq!(QueueManagerImpl::codec_from_extension("FLAC"), Some(AudioCodec::Flac));
        assert_eq!(QueueManagerImpl::codec_from_extension("wav"), Some(AudioCodec::Wav));
        assert_eq!(QueueManagerImpl::codec_from_extension("wave"), Some(AudioCodec::Wav));
        assert_eq!(QueueManagerImpl::codec_from_extension("mp3"), Some(AudioCodec::Mp3));
        assert_eq!(QueueManagerImpl::codec_from_extension("m4a"), Some(AudioCodec::Alac));
        assert_eq!(QueueManagerImpl::codec_from_extension("alac"), Some(AudioCodec::Alac));
        assert_eq!(QueueManagerImpl::codec_from_extension("ogg"), Some(AudioCodec::OggVorbis));
        assert_eq!(QueueManagerImpl::codec_from_extension("oga"), Some(AudioCodec::OggVorbis));

        assert_eq!(QueueManagerImpl::codec_from_extension("txt"), None);
        assert_eq!(QueueManagerImpl::codec_from_extension(""), None);
    }

    #[test]
    fn test_add_file_nonexistent() {
        let temp_dir = TempDir::new().unwrap();
        let mut queue_manager = QueueManagerImpl::with_playlist_directory(temp_dir.path().to_path_buf()).unwrap();
        let result = queue_manager.add_file(Path::new("/nonexistent/file.flac"));
        
        assert!(result.is_err());
        match result.unwrap_err() {
            QueueError::FileNotFound { path } => {
                assert!(path.contains("nonexistent"));
            }
            _ => panic!("Expected FileNotFound error"),
        }
    }

    #[test]
    fn test_add_file_unsupported_format() {
        let temp_dir = TempDir::new().unwrap();
        let txt_file = temp_dir.path().join("test.txt");
        fs::write(&txt_file, b"This is not an audio file").unwrap();

        let mut queue_manager = QueueManagerImpl::with_playlist_directory(temp_dir.path().to_path_buf()).unwrap();
        let result = queue_manager.add_file(&txt_file);
        
        assert!(result.is_err());
        match result.unwrap_err() {
            QueueError::InvalidFormat { path } => {
                assert!(path.contains("test.txt"));
            }
            _ => panic!("Expected InvalidFormat error"),
        }
    }

    #[test]
    fn test_add_file_success() {
        let temp_dir = TempDir::new().unwrap();
        let audio_file = create_test_audio_file(temp_dir.path(), "test", "flac");

        let mut queue_manager = QueueManagerImpl::with_playlist_directory(temp_dir.path().to_path_buf()).unwrap();
        let result = queue_manager.add_file(&audio_file);
        
        assert!(result.is_ok());
        assert_eq!(queue_manager.len(), 1);
        assert!(!queue_manager.is_empty());
        
        let track = queue_manager.current_track().unwrap();
        assert_eq!(track.path, audio_file);
    }

    #[test]
    fn test_add_directory_success() {
        let temp_dir = create_test_directory_structure();
        let mut queue_manager = QueueManagerImpl::with_playlist_directory(temp_dir.path().to_path_buf()).unwrap();
        
        let result = queue_manager.add_directory(temp_dir.path());
        assert!(result.is_ok());
        
        // Should have found 5 audio files (3 in root + 2 in subdir)
        assert_eq!(queue_manager.len(), 5);
        
        // Files should be sorted
        let tracks: Vec<_> = queue_manager.list().iter().collect();
        let file_names: Vec<String> = tracks.iter()
            .map(|t| t.path.file_name().unwrap().to_string_lossy().to_string())
            .collect();
        let mut sorted_names = file_names.clone();
        sorted_names.sort();
        
        // The files should be in sorted order
        assert_eq!(file_names.len(), 5);
    }

    #[test]
    fn test_add_directory_nonexistent() {
        let temp_dir = TempDir::new().unwrap();
        let mut queue_manager = QueueManagerImpl::with_playlist_directory(temp_dir.path().to_path_buf()).unwrap();
        let result = queue_manager.add_directory(Path::new("/nonexistent/directory"));
        
        assert!(result.is_err());
        match result.unwrap_err() {
            QueueError::FileNotFound { path } => {
                assert!(path.contains("nonexistent"));
            }
            _ => panic!("Expected FileNotFound error"),
        }
    }

    #[test]
    fn test_queue_navigation() {
        let temp_dir = TempDir::new().unwrap();
        let file1 = create_test_audio_file(temp_dir.path(), "song1", "flac");
        let file2 = create_test_audio_file(temp_dir.path(), "song2", "mp3");
        let file3 = create_test_audio_file(temp_dir.path(), "song3", "wav");

        let mut queue_manager = QueueManagerImpl::with_playlist_directory(temp_dir.path().to_path_buf()).unwrap();
        queue_manager.add_file(&file1).unwrap();
        queue_manager.add_file(&file2).unwrap();
        queue_manager.add_file(&file3).unwrap();

        // Should start at index 0
        assert_eq!(queue_manager.current_index(), 0);
        assert_eq!(queue_manager.current_track().unwrap().path, file1);

        // Move to next track
        let next_track = queue_manager.next_track().unwrap();
        assert_eq!(next_track.path, file2);
        assert_eq!(queue_manager.current_index(), 1);

        // Move to next track again
        let next_track = queue_manager.next_track().unwrap();
        assert_eq!(next_track.path, file3);
        assert_eq!(queue_manager.current_index(), 2);

        // Move to next track (should wrap around)
        let next_track = queue_manager.next_track().unwrap();
        assert_eq!(next_track.path, file1);
        assert_eq!(queue_manager.current_index(), 0);

        // Move to previous track (should wrap around to end)
        let prev_track = queue_manager.previous_track().unwrap();
        assert_eq!(prev_track.path, file3);
        assert_eq!(queue_manager.current_index(), 2);

        // Move to previous track
        let prev_track = queue_manager.previous_track().unwrap();
        assert_eq!(prev_track.path, file2);
        assert_eq!(queue_manager.current_index(), 1);
    }

    #[test]
    fn test_queue_navigation_empty() {
        let temp_dir = TempDir::new().unwrap();
        let mut queue_manager = QueueManagerImpl::with_playlist_directory(temp_dir.path().to_path_buf()).unwrap();
        
        assert!(queue_manager.next_track().is_none());
        assert!(queue_manager.previous_track().is_none());
    }

    #[test]
    fn test_queue_navigation_single_track() {
        let temp_dir = TempDir::new().unwrap();
        let file1 = create_test_audio_file(temp_dir.path(), "song1", "flac");

        let mut queue_manager = QueueManagerImpl::with_playlist_directory(temp_dir.path().to_path_buf()).unwrap();
        queue_manager.add_file(&file1).unwrap();

        // Should stay on the same track when navigating
        let next_track = queue_manager.next_track().unwrap();
        assert_eq!(next_track.path, file1);
        assert_eq!(queue_manager.current_index(), 0);

        let prev_track = queue_manager.previous_track().unwrap();
        assert_eq!(prev_track.path, file1);
        assert_eq!(queue_manager.current_index(), 0);
    }

    #[test]
    fn test_clear_queue() {
        let temp_dir = TempDir::new().unwrap();
        let file1 = create_test_audio_file(temp_dir.path(), "song1", "flac");
        let file2 = create_test_audio_file(temp_dir.path(), "song2", "mp3");

        let mut queue_manager = QueueManagerImpl::with_playlist_directory(temp_dir.path().to_path_buf()).unwrap();
        queue_manager.add_file(&file1).unwrap();
        queue_manager.add_file(&file2).unwrap();

        assert_eq!(queue_manager.len(), 2);
        assert!(!queue_manager.is_empty());

        queue_manager.clear();

        assert_eq!(queue_manager.len(), 0);
        assert!(queue_manager.is_empty());
        assert_eq!(queue_manager.current_index(), 0);
        assert!(queue_manager.current_track().is_none());
    }

    #[test]
    fn test_remove_track() {
        let temp_dir = TempDir::new().unwrap();
        let file1 = create_test_audio_file(temp_dir.path(), "song1", "flac");
        let file2 = create_test_audio_file(temp_dir.path(), "song2", "mp3");
        let file3 = create_test_audio_file(temp_dir.path(), "song3", "wav");

        let mut queue_manager = QueueManagerImpl::with_playlist_directory(temp_dir.path().to_path_buf()).unwrap();
        queue_manager.add_file(&file1).unwrap();
        queue_manager.add_file(&file2).unwrap();
        queue_manager.add_file(&file3).unwrap();

        // Remove middle track
        let removed = queue_manager.remove(1).unwrap();
        assert_eq!(removed.path, file2);
        assert_eq!(queue_manager.len(), 2);

        // Current index should still be 0
        assert_eq!(queue_manager.current_index(), 0);
        assert_eq!(queue_manager.current_track().unwrap().path, file1);

        // Remove invalid index
        let result = queue_manager.remove(10);
        assert!(result.is_err());
        match result.unwrap_err() {
            QueueError::InvalidIndex { index } => assert_eq!(index, 10),
            _ => panic!("Expected InvalidIndex error"),
        }
    }

    #[test]
    fn test_remove_current_track() {
        let temp_dir = TempDir::new().unwrap();
        let file1 = create_test_audio_file(temp_dir.path(), "song1", "flac");
        let file2 = create_test_audio_file(temp_dir.path(), "song2", "mp3");

        let mut queue_manager = QueueManagerImpl::with_playlist_directory(temp_dir.path().to_path_buf()).unwrap();
        queue_manager.add_file(&file1).unwrap();
        queue_manager.add_file(&file2).unwrap();

        // Move to second track
        queue_manager.next_track();
        assert_eq!(queue_manager.current_index(), 1);

        // Remove current track (last track)
        let removed = queue_manager.remove(1).unwrap();
        assert_eq!(removed.path, file2);
        assert_eq!(queue_manager.len(), 1);

        // Current index should be adjusted to 0 (the only remaining track)
        assert_eq!(queue_manager.current_index(), 0);
        assert_eq!(queue_manager.current_track().unwrap().path, file1);
    }

    #[test]
    fn test_jump_to_track() {
        let temp_dir = TempDir::new().unwrap();
        let file1 = create_test_audio_file(temp_dir.path(), "song1", "flac");
        let file2 = create_test_audio_file(temp_dir.path(), "song2", "mp3");
        let file3 = create_test_audio_file(temp_dir.path(), "song3", "wav");

        let mut queue_manager = QueueManagerImpl::with_playlist_directory(temp_dir.path().to_path_buf()).unwrap();
        queue_manager.add_file(&file1).unwrap();
        queue_manager.add_file(&file2).unwrap();
        queue_manager.add_file(&file3).unwrap();

        // Jump to track 2
        let track = queue_manager.jump_to(2).unwrap();
        assert_eq!(track.path, file3);
        assert_eq!(queue_manager.current_index(), 2);

        // Jump to track 0
        let track = queue_manager.jump_to(0).unwrap();
        assert_eq!(track.path, file1);
        assert_eq!(queue_manager.current_index(), 0);

        // Jump to invalid index
        let result = queue_manager.jump_to(10);
        assert!(result.is_err());
        match result.unwrap_err() {
            QueueError::InvalidIndex { index } => assert_eq!(index, 10),
            _ => panic!("Expected InvalidIndex error"),
        }
    }

    #[test]
    fn test_queue_list() {
        let temp_dir = TempDir::new().unwrap();
        let file1 = create_test_audio_file(temp_dir.path(), "song1", "flac");
        let file2 = create_test_audio_file(temp_dir.path(), "song2", "mp3");

        let mut queue_manager = QueueManagerImpl::with_playlist_directory(temp_dir.path().to_path_buf()).unwrap();
        queue_manager.add_file(&file1).unwrap();
        queue_manager.add_file(&file2).unwrap();

        let queue_list = queue_manager.list();
        assert_eq!(queue_list.len(), 2);
        assert_eq!(queue_list[0].path, file1);
        assert_eq!(queue_list[1].path, file2);
    }

    #[test]
    fn test_scan_directory_recursive() {
        let temp_dir = create_test_directory_structure();
        let audio_files = QueueManagerImpl::scan_directory(temp_dir.path()).unwrap();
        
        // Should find 5 audio files total
        assert_eq!(audio_files.len(), 5);
        
        // Should not include the txt file
        let has_txt = audio_files.iter().any(|p| p.extension().and_then(|e| e.to_str()) == Some("txt"));
        assert!(!has_txt);
        
        // Should include files from subdirectory
        let has_subdir_files = audio_files.iter().any(|p| p.to_string_lossy().contains("subdir"));
        assert!(has_subdir_files);
    }

    #[test]
    fn test_default_implementation() {
        let queue_manager = QueueManagerImpl::default();
        assert!(queue_manager.is_empty());
        assert_eq!(queue_manager.len(), 0);
    }
    
    #[test]
    fn test_playlist_integration() {
        let temp_dir = TempDir::new().unwrap();
        let file1 = create_test_audio_file(temp_dir.path(), "song1", "flac");
        let file2 = create_test_audio_file(temp_dir.path(), "song2", "mp3");
        
        let mut queue_manager = QueueManagerImpl::with_playlist_directory(temp_dir.path().to_path_buf()).unwrap();
        
        // Add files to queue
        queue_manager.add_file(&file1).unwrap();
        queue_manager.add_file(&file2).unwrap();
        assert_eq!(queue_manager.len(), 2);
        
        // Save playlist
        let result = queue_manager.save_playlist("test_playlist", crate::queue::playlist::PlaylistFormat::M3u);
        assert!(result.is_ok());
        
        // List playlists
        let playlists = queue_manager.list_playlists().unwrap();
        assert_eq!(playlists.len(), 1);
        assert_eq!(playlists[0], "test_playlist");
        
        // Clear queue and load playlist
        queue_manager.clear();
        assert!(queue_manager.is_empty());
        
        let result = queue_manager.load_playlist("test_playlist");
        assert!(result.is_ok());
        assert_eq!(queue_manager.len(), 2);
        
        // Delete playlist
        let result = queue_manager.delete_playlist("test_playlist");
        assert!(result.is_ok());
        
        let playlists = queue_manager.list_playlists().unwrap();
        assert!(playlists.is_empty());
    }
    
    #[test]
    fn test_playlist_load_nonexistent() {
        let temp_dir = TempDir::new().unwrap();
        let mut queue_manager = QueueManagerImpl::with_playlist_directory(temp_dir.path().to_path_buf()).unwrap();
        
        let result = queue_manager.load_playlist("nonexistent");
        assert!(result.is_err());
        
        match result.unwrap_err() {
            PlaylistError::PlaylistNotFound { name } => {
                assert_eq!(name, "nonexistent");
            }
            _ => panic!("Expected PlaylistNotFound error"),
        }
    }
    
    #[test]
    fn test_playlist_save_empty_queue() {
        let temp_dir = TempDir::new().unwrap();
        let queue_manager = QueueManagerImpl::with_playlist_directory(temp_dir.path().to_path_buf()).unwrap();
        
        let result = queue_manager.save_playlist("empty", crate::queue::playlist::PlaylistFormat::M3u);
        assert!(result.is_err());
        
        match result.unwrap_err() {
            PlaylistError::InvalidFormat(msg) => {
                assert!(msg.contains("empty"));
            }
            _ => panic!("Expected InvalidFormat error"),
        }
    }
}