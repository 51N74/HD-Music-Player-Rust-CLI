use std::collections::VecDeque;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use crate::error::PlaylistError;
use crate::models::TrackInfo;

/// Supported playlist formats
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PlaylistFormat {
    M3u,
    Pls,
}

impl PlaylistFormat {
    /// Get the file extension for this format
    pub fn extension(&self) -> &'static str {
        match self {
            PlaylistFormat::M3u => "m3u",
            PlaylistFormat::Pls => "pls",
        }
    }

    /// Detect format from file extension
    pub fn from_extension(extension: &str) -> Option<Self> {
        match extension.to_lowercase().as_str() {
            "m3u" | "m3u8" => Some(PlaylistFormat::M3u),
            "pls" => Some(PlaylistFormat::Pls),
            _ => None,
        }
    }

    /// Get the MIME type for this format
    pub fn mime_type(&self) -> &'static str {
        match self {
            PlaylistFormat::M3u => "audio/x-mpegurl",
            PlaylistFormat::Pls => "audio/x-scpls",
        }
    }
}

/// Playlist manager for saving and loading playlists
pub struct PlaylistManager {
    playlist_directory: PathBuf,
}

impl PlaylistManager {
    /// Create a new playlist manager with the specified directory
    pub fn new(playlist_directory: PathBuf) -> Result<Self, PlaylistError> {
        // Create the playlist directory if it doesn't exist
        if !playlist_directory.exists() {
            fs::create_dir_all(&playlist_directory)?;
        }

        Ok(Self {
            playlist_directory,
        })
    }

    /// Save a queue as a playlist
    pub fn save_playlist(
        &self,
        name: &str,
        queue: &VecDeque<TrackInfo>,
        format: PlaylistFormat,
    ) -> Result<(), PlaylistError> {
        if queue.is_empty() {
            return Err(PlaylistError::InvalidFormat("Cannot save empty playlist".to_string()));
        }

        let filename = format!("{}.{}", name, format.extension());
        let playlist_path = self.playlist_directory.join(filename);

        match format {
            PlaylistFormat::M3u => self.save_m3u(&playlist_path, queue),
            PlaylistFormat::Pls => self.save_pls(&playlist_path, queue),
        }
    }

    /// Load a playlist into a queue
    pub fn load_playlist(&self, name: &str) -> Result<VecDeque<TrackInfo>, PlaylistError> {
        // Try different extensions
        let extensions = ["m3u", "m3u8", "pls"];
        
        for ext in &extensions {
            let filename = format!("{}.{}", name, ext);
            let playlist_path = self.playlist_directory.join(filename);
            
            if playlist_path.exists() {
                let format = PlaylistFormat::from_extension(ext)
                    .ok_or_else(|| PlaylistError::InvalidFormat(format!("Unknown extension: {}", ext)))?;
                
                return match format {
                    PlaylistFormat::M3u => self.load_m3u(&playlist_path),
                    PlaylistFormat::Pls => self.load_pls(&playlist_path),
                };
            }
        }

        Err(PlaylistError::PlaylistNotFound {
            name: name.to_string(),
        })
    }

    /// List available playlists
    pub fn list_playlists(&self) -> Result<Vec<String>, PlaylistError> {
        let mut playlists = Vec::new();

        if !self.playlist_directory.exists() {
            return Ok(playlists);
        }

        let entries = fs::read_dir(&self.playlist_directory)?;

        for entry in entries {
            let entry = entry?;
            let path = entry.path();

            if path.is_file() {
                if let Some(extension) = path.extension().and_then(|ext| ext.to_str()) {
                    if PlaylistFormat::from_extension(extension).is_some() {
                        if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                            playlists.push(stem.to_string());
                        }
                    }
                }
            }
        }

        playlists.sort();
        playlists.dedup(); // Remove duplicates (same name with different extensions)
        Ok(playlists)
    }

    /// Delete a playlist
    pub fn delete_playlist(&self, name: &str) -> Result<(), PlaylistError> {
        let extensions = ["m3u", "m3u8", "pls"];
        let mut found = false;

        for ext in &extensions {
            let filename = format!("{}.{}", name, ext);
            let playlist_path = self.playlist_directory.join(filename);

            if playlist_path.exists() {
                fs::remove_file(playlist_path)?;
                found = true;
            }
        }

        if !found {
            return Err(PlaylistError::PlaylistNotFound {
                name: name.to_string(),
            });
        }

        Ok(())
    }

    /// Save playlist in M3U format
    fn save_m3u(&self, path: &Path, queue: &VecDeque<TrackInfo>) -> Result<(), PlaylistError> {
        let mut file = fs::File::create(path)?;
        
        // Write M3U header
        writeln!(file, "#EXTM3U")?;

        for track in queue {
            // Write extended info line
            let duration_seconds = track.duration.as_secs() as i32;
            let artist = track.metadata.artist.as_deref().unwrap_or("Unknown Artist");
            let title = track.metadata.title.as_deref()
                .unwrap_or_else(|| {
                    track.path.file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("Unknown")
                });

            writeln!(file, "#EXTINF:{},{} - {}", duration_seconds, artist, title)?;
            
            // Write file path (convert to string, handling potential UTF-8 issues)
            let path_str = track.path.to_string_lossy();
            writeln!(file, "{}", path_str)?;
        }

        Ok(())
    }

    /// Save playlist in PLS format
    fn save_pls(&self, path: &Path, queue: &VecDeque<TrackInfo>) -> Result<(), PlaylistError> {
        let mut file = fs::File::create(path)?;
        
        // Write PLS header
        writeln!(file, "[playlist]")?;
        writeln!(file, "NumberOfEntries={}", queue.len())?;
        writeln!(file)?;

        for (index, track) in queue.iter().enumerate() {
            let entry_num = index + 1;
            
            // File path
            let path_str = track.path.to_string_lossy();
            writeln!(file, "File{}={}", entry_num, path_str)?;
            
            // Title
            let artist = track.metadata.artist.as_deref().unwrap_or("Unknown Artist");
            let title = track.metadata.title.as_deref()
                .unwrap_or_else(|| {
                    track.path.file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("Unknown")
                });
            writeln!(file, "Title{}={} - {}", entry_num, artist, title)?;
            
            // Length (in seconds)
            let duration_seconds = track.duration.as_secs();
            writeln!(file, "Length{}={}", entry_num, duration_seconds)?;
            writeln!(file)?;
        }

        // Write version
        writeln!(file, "Version=2")?;

        Ok(())
    }

    /// Load playlist from M3U format
    fn load_m3u(&self, path: &Path) -> Result<VecDeque<TrackInfo>, PlaylistError> {
        let file = fs::File::open(path)?;
        let reader = BufReader::new(file);
        let mut queue = VecDeque::new();

        for line in reader.lines() {
            let line = line?;
            let line = line.trim();

            // Skip empty lines and comments (except #EXTINF which we ignore for now)
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            // This should be a file path
            let track_path = self.resolve_path(path, line)?;
            
            if let Ok(track_info) = self.create_track_info_from_path(&track_path) {
                queue.push_back(track_info);
            }
            // Silently skip files that can't be loaded (they might have been moved/deleted)
        }

        Ok(queue)
    }

    /// Load playlist from PLS format
    fn load_pls(&self, path: &Path) -> Result<VecDeque<TrackInfo>, PlaylistError> {
        let file = fs::File::open(path)?;
        let reader = BufReader::new(file);
        let mut queue = VecDeque::new();
        let mut file_entries = std::collections::HashMap::new();

        for line in reader.lines() {
            let line = line?;
            let line = line.trim();

            // Skip empty lines and section headers
            if line.is_empty() || line.starts_with('[') || line.starts_with("NumberOfEntries") || line.starts_with("Version") {
                continue;
            }

            // Parse File entries
            if let Some(file_line) = line.strip_prefix("File") {
                if let Some(equals_pos) = file_line.find('=') {
                    let entry_part = &file_line[..equals_pos];
                    let path_part = &file_line[equals_pos + 1..];
                    
                    if let Ok(entry_num) = entry_part.parse::<usize>() {
                        file_entries.insert(entry_num, path_part.to_string());
                    }
                }
            }
            // We ignore Title and Length entries for now, as we extract metadata from files directly
        }

        // Sort entries by number and add to queue
        let mut sorted_entries: Vec<_> = file_entries.into_iter().collect();
        sorted_entries.sort_by_key(|(num, _)| *num);

        for (_, file_path) in sorted_entries {
            let track_path = self.resolve_path(path, &file_path)?;
            
            if let Ok(track_info) = self.create_track_info_from_path(&track_path) {
                queue.push_back(track_info);
            }
            // Silently skip files that can't be loaded
        }

        Ok(queue)
    }

    /// Resolve a file path relative to the playlist file
    fn resolve_path(&self, playlist_path: &Path, file_path: &str) -> Result<PathBuf, PlaylistError> {
        let path = Path::new(file_path);
        
        if path.is_absolute() {
            Ok(path.to_path_buf())
        } else {
            // Resolve relative to the playlist file's directory
            if let Some(playlist_dir) = playlist_path.parent() {
                Ok(playlist_dir.join(path))
            } else {
                Ok(path.to_path_buf())
            }
        }
    }

    /// Create TrackInfo from a file path (simplified version for playlist loading)
    fn create_track_info_from_path(&self, path: &Path) -> Result<TrackInfo, PlaylistError> {
        use crate::queue::QueueManagerImpl;
        
        // Use the existing create_track_info method from QueueManagerImpl
        // This reuses the existing metadata extraction logic
        QueueManagerImpl::create_track_info(path)
            .map_err(|e| PlaylistError::InvalidFormat(format!("Failed to load track: {}", e)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::AudioMetadata;
    use std::time::Duration;
    use tempfile::TempDir;

    fn create_test_track(name: &str, artist: &str, duration_secs: u64) -> TrackInfo {
        let metadata = AudioMetadata {
            title: Some(name.to_string()),
            artist: Some(artist.to_string()),
            album: Some("Test Album".to_string()),
            track_number: Some(1),
            year: Some(2023),
            genre: Some("Test".to_string()),
        };

        TrackInfo::new(
            PathBuf::from(format!("/test/path/{}.flac", name.to_lowercase().replace(' ', "_"))),
            metadata,
            Duration::from_secs(duration_secs),
            1024 * 1024, // 1MB
        )
    }

    fn create_test_queue() -> VecDeque<TrackInfo> {
        let mut queue = VecDeque::new();
        queue.push_back(create_test_track("Song One", "Artist A", 180));
        queue.push_back(create_test_track("Song Two", "Artist B", 240));
        queue.push_back(create_test_track("Song Three", "Artist A", 200));
        queue
    }

    #[test]
    fn test_playlist_format_extension() {
        assert_eq!(PlaylistFormat::M3u.extension(), "m3u");
        assert_eq!(PlaylistFormat::Pls.extension(), "pls");
    }

    #[test]
    fn test_playlist_format_from_extension() {
        assert_eq!(PlaylistFormat::from_extension("m3u"), Some(PlaylistFormat::M3u));
        assert_eq!(PlaylistFormat::from_extension("M3U"), Some(PlaylistFormat::M3u));
        assert_eq!(PlaylistFormat::from_extension("m3u8"), Some(PlaylistFormat::M3u));
        assert_eq!(PlaylistFormat::from_extension("pls"), Some(PlaylistFormat::Pls));
        assert_eq!(PlaylistFormat::from_extension("PLS"), Some(PlaylistFormat::Pls));
        assert_eq!(PlaylistFormat::from_extension("txt"), None);
        assert_eq!(PlaylistFormat::from_extension(""), None);
    }

    #[test]
    fn test_playlist_format_mime_type() {
        assert_eq!(PlaylistFormat::M3u.mime_type(), "audio/x-mpegurl");
        assert_eq!(PlaylistFormat::Pls.mime_type(), "audio/x-scpls");
    }

    #[test]
    fn test_playlist_manager_creation() {
        let temp_dir = TempDir::new().unwrap();
        let playlist_dir = temp_dir.path().join("playlists");

        let manager = PlaylistManager::new(playlist_dir.clone()).unwrap();
        
        // Directory should be created
        assert!(playlist_dir.exists());
        assert!(playlist_dir.is_dir());
    }

    #[test]
    fn test_playlist_manager_creation_existing_dir() {
        let temp_dir = TempDir::new().unwrap();
        let playlist_dir = temp_dir.path().join("playlists");
        
        // Create directory first
        fs::create_dir(&playlist_dir).unwrap();
        
        let manager = PlaylistManager::new(playlist_dir.clone()).unwrap();
        
        // Should not fail if directory already exists
        assert!(playlist_dir.exists());
    }

    #[test]
    fn test_save_m3u_playlist() {
        let temp_dir = TempDir::new().unwrap();
        let manager = PlaylistManager::new(temp_dir.path().to_path_buf()).unwrap();
        let queue = create_test_queue();

        let result = manager.save_playlist("test_playlist", &queue, PlaylistFormat::M3u);
        assert!(result.is_ok());

        // Check that file was created
        let playlist_path = temp_dir.path().join("test_playlist.m3u");
        assert!(playlist_path.exists());

        // Check file contents
        let content = fs::read_to_string(playlist_path).unwrap();
        assert!(content.contains("#EXTM3U"));
        assert!(content.contains("#EXTINF:180,Artist A - Song One"));
        assert!(content.contains("#EXTINF:240,Artist B - Song Two"));
        assert!(content.contains("#EXTINF:200,Artist A - Song Three"));
        assert!(content.contains("/test/path/song_one.flac"));
        assert!(content.contains("/test/path/song_two.flac"));
        assert!(content.contains("/test/path/song_three.flac"));
    }

    #[test]
    fn test_save_pls_playlist() {
        let temp_dir = TempDir::new().unwrap();
        let manager = PlaylistManager::new(temp_dir.path().to_path_buf()).unwrap();
        let queue = create_test_queue();

        let result = manager.save_playlist("test_playlist", &queue, PlaylistFormat::Pls);
        assert!(result.is_ok());

        // Check that file was created
        let playlist_path = temp_dir.path().join("test_playlist.pls");
        assert!(playlist_path.exists());

        // Check file contents
        let content = fs::read_to_string(playlist_path).unwrap();
        assert!(content.contains("[playlist]"));
        assert!(content.contains("NumberOfEntries=3"));
        assert!(content.contains("File1=/test/path/song_one.flac"));
        assert!(content.contains("Title1=Artist A - Song One"));
        assert!(content.contains("Length1=180"));
        assert!(content.contains("File2=/test/path/song_two.flac"));
        assert!(content.contains("Title2=Artist B - Song Two"));
        assert!(content.contains("Length2=240"));
        assert!(content.contains("File3=/test/path/song_three.flac"));
        assert!(content.contains("Title3=Artist A - Song Three"));
        assert!(content.contains("Length3=200"));
        assert!(content.contains("Version=2"));
    }

    #[test]
    fn test_save_empty_playlist() {
        let temp_dir = TempDir::new().unwrap();
        let manager = PlaylistManager::new(temp_dir.path().to_path_buf()).unwrap();
        let empty_queue = VecDeque::new();

        let result = manager.save_playlist("empty_playlist", &empty_queue, PlaylistFormat::M3u);
        assert!(result.is_err());
        
        match result.unwrap_err() {
            PlaylistError::InvalidFormat(msg) => {
                assert!(msg.contains("empty playlist"));
            }
            _ => panic!("Expected InvalidFormat error"),
        }
    }

    #[test]
    fn test_list_playlists_empty_directory() {
        let temp_dir = TempDir::new().unwrap();
        let manager = PlaylistManager::new(temp_dir.path().to_path_buf()).unwrap();

        let playlists = manager.list_playlists().unwrap();
        assert!(playlists.is_empty());
    }

    #[test]
    fn test_list_playlists_with_files() {
        let temp_dir = TempDir::new().unwrap();
        let manager = PlaylistManager::new(temp_dir.path().to_path_buf()).unwrap();
        let queue = create_test_queue();

        // Create some playlists
        manager.save_playlist("playlist1", &queue, PlaylistFormat::M3u).unwrap();
        manager.save_playlist("playlist2", &queue, PlaylistFormat::Pls).unwrap();
        manager.save_playlist("playlist3", &queue, PlaylistFormat::M3u).unwrap();

        // Create a non-playlist file (should be ignored)
        fs::write(temp_dir.path().join("not_a_playlist.txt"), "test").unwrap();

        let playlists = manager.list_playlists().unwrap();
        assert_eq!(playlists.len(), 3);
        assert!(playlists.contains(&"playlist1".to_string()));
        assert!(playlists.contains(&"playlist2".to_string()));
        assert!(playlists.contains(&"playlist3".to_string()));
        assert!(!playlists.contains(&"not_a_playlist".to_string()));
    }

    #[test]
    fn test_list_playlists_deduplicated() {
        let temp_dir = TempDir::new().unwrap();
        let manager = PlaylistManager::new(temp_dir.path().to_path_buf()).unwrap();
        let queue = create_test_queue();

        // Create same playlist in different formats
        manager.save_playlist("same_name", &queue, PlaylistFormat::M3u).unwrap();
        manager.save_playlist("same_name", &queue, PlaylistFormat::Pls).unwrap();

        let playlists = manager.list_playlists().unwrap();
        assert_eq!(playlists.len(), 1);
        assert_eq!(playlists[0], "same_name");
    }

    #[test]
    fn test_delete_playlist() {
        let temp_dir = TempDir::new().unwrap();
        let manager = PlaylistManager::new(temp_dir.path().to_path_buf()).unwrap();
        let queue = create_test_queue();

        // Create playlist
        manager.save_playlist("to_delete", &queue, PlaylistFormat::M3u).unwrap();
        
        let playlist_path = temp_dir.path().join("to_delete.m3u");
        assert!(playlist_path.exists());

        // Delete playlist
        let result = manager.delete_playlist("to_delete");
        assert!(result.is_ok());
        assert!(!playlist_path.exists());
    }

    #[test]
    fn test_delete_nonexistent_playlist() {
        let temp_dir = TempDir::new().unwrap();
        let manager = PlaylistManager::new(temp_dir.path().to_path_buf()).unwrap();

        let result = manager.delete_playlist("nonexistent");
        assert!(result.is_err());
        
        match result.unwrap_err() {
            PlaylistError::PlaylistNotFound { name } => {
                assert_eq!(name, "nonexistent");
            }
            _ => panic!("Expected PlaylistNotFound error"),
        }
    }

    #[test]
    fn test_delete_multiple_formats() {
        let temp_dir = TempDir::new().unwrap();
        let manager = PlaylistManager::new(temp_dir.path().to_path_buf()).unwrap();
        let queue = create_test_queue();

        // Create same playlist in multiple formats
        manager.save_playlist("multi_format", &queue, PlaylistFormat::M3u).unwrap();
        manager.save_playlist("multi_format", &queue, PlaylistFormat::Pls).unwrap();
        
        let m3u_path = temp_dir.path().join("multi_format.m3u");
        let pls_path = temp_dir.path().join("multi_format.pls");
        assert!(m3u_path.exists());
        assert!(pls_path.exists());

        // Delete should remove both
        let result = manager.delete_playlist("multi_format");
        assert!(result.is_ok());
        assert!(!m3u_path.exists());
        assert!(!pls_path.exists());
    }

    #[test]
    fn test_load_nonexistent_playlist() {
        let temp_dir = TempDir::new().unwrap();
        let manager = PlaylistManager::new(temp_dir.path().to_path_buf()).unwrap();

        let result = manager.load_playlist("nonexistent");
        assert!(result.is_err());
        
        match result.unwrap_err() {
            PlaylistError::PlaylistNotFound { name } => {
                assert_eq!(name, "nonexistent");
            }
            _ => panic!("Expected PlaylistNotFound error"),
        }
    }

    #[test]
    fn test_resolve_path_absolute() {
        let temp_dir = TempDir::new().unwrap();
        let manager = PlaylistManager::new(temp_dir.path().to_path_buf()).unwrap();
        let playlist_path = temp_dir.path().join("test.m3u");

        let absolute_path = "/absolute/path/to/file.flac";
        let resolved = manager.resolve_path(&playlist_path, absolute_path).unwrap();
        
        assert_eq!(resolved, PathBuf::from(absolute_path));
    }

    #[test]
    fn test_resolve_path_relative() {
        let temp_dir = TempDir::new().unwrap();
        let manager = PlaylistManager::new(temp_dir.path().to_path_buf()).unwrap();
        let playlist_path = temp_dir.path().join("test.m3u");

        let relative_path = "music/file.flac";
        let resolved = manager.resolve_path(&playlist_path, relative_path).unwrap();
        
        let expected = temp_dir.path().join("music/file.flac");
        assert_eq!(resolved, expected);
    }

    // Note: We can't easily test the actual loading of M3U/PLS files without creating real audio files,
    // as the create_track_info_from_path method requires actual files to exist and be valid audio files.
    // In a real implementation, you might want to add a mock or test mode for this.
}