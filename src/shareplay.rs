use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::process::Command;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum RepeatMode {
    Off,
    One,
    All,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueItem {
    pub id: String,
    pub url: String,
    pub title: String,
    pub file_path: Option<String>,      // None while downloading
    pub download_error: Option<String>, // Some(err) if failed
    pub duration_seconds: u64,
    pub download_status: String, // "grabbing", "downloading", "ready", "error"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharePlayState {
    pub queue: Vec<QueueItem>,
    pub current_index: Option<usize>,
    pub status: String,                    // "playing" or "paused"
    pub start_time: Option<DateTime<Utc>>, // When playback started/resumed, for sync
    pub current_position_secs: f64,        // Saved position when paused
    pub repeat_mode: RepeatMode,
    #[serde(skip)]
    pub last_auto_next: Option<DateTime<Utc>>,
}

impl SharePlayState {
    pub fn new() -> Self {
        Self {
            queue: Vec::new(),
            current_index: None,
            status: "paused".to_string(),
            start_time: None,
            current_position_secs: 0.0,
            repeat_mode: RepeatMode::Off,
            last_auto_next: None,
        }
    }

    pub fn add_item(&mut self, url: String) -> String {
        let id = Uuid::new_v4().to_string();
        self.queue.push(QueueItem {
            id: id.clone(),
            url,
            title: "Grabbing...".to_string(),
            file_path: None,
            download_error: None,
            duration_seconds: 0,
            download_status: "grabbing".to_string(),
        });

        // Auto-play if queue was empty or nothing playing
        if self.current_index.is_none() && !self.queue.is_empty() {
            self.current_index = Some(0);
        }

        id
    }

    pub fn update_item_success(
        &mut self,
        id: &str,
        title: String,
        file_path: String,
        duration: u64,
    ) {
        if let Some(item) = self.queue.iter_mut().find(|i| i.id == id) {
            item.title = title;
            item.file_path = Some(file_path);
            item.duration_seconds = duration;
            item.download_status = "ready".to_string();
        }
    }

    pub fn update_item_error(&mut self, id: &str, error: String) {
        if let Some(item) = self.queue.iter_mut().find(|i| i.id == id) {
            item.title = "Error loading song".to_string();
            item.download_error = Some(error);
            item.download_status = "error".to_string();
        }
    }

    /// Update item with metadata from simulation step (grabbing -> downloading)
    pub fn update_item_metadata(&mut self, id: &str, title: String, duration: u64) {
        if let Some(item) = self.queue.iter_mut().find(|i| i.id == id) {
            item.title = title;
            item.duration_seconds = duration;
            item.download_status = "downloading".to_string();
        }
    }

    pub fn play(&mut self) {
        if self.status == "playing" {
            return;
        }
        self.status = "playing".to_string();
        self.start_time = Some(Utc::now());
    }

    pub fn pause(&mut self) {
        if self.status == "paused" {
            return;
        }
        // Calculate accrued time
        if let Some(start) = self.start_time {
            let elapsed = (Utc::now() - start).num_milliseconds() as f64 / 1000.0;
            self.current_position_secs += elapsed;
        }
        self.status = "paused".to_string();
        self.start_time = None;
    }

    pub fn seek(&mut self, timestamp: f64) {
        self.current_position_secs = timestamp;
        if self.status == "playing" {
            self.start_time = Some(Utc::now());
        }
    }

    pub fn next(&mut self) {
        let now = Utc::now();
        // Spam protection: ignore if last auto-next was < 1s ago
        if let Some(last) = self.last_auto_next {
            if (now - last).num_milliseconds() < 1000 {
                return;
            }
        }
        self.last_auto_next = Some(now);

        if self.queue.is_empty() {
            return;
        }

        if let Some(curr) = self.current_index {
            match self.repeat_mode {
                RepeatMode::One => {
                    // Replay current
                    self.current_position_secs = 0.0;
                    if self.status == "playing" {
                        self.start_time = Some(now);
                    }
                }
                RepeatMode::All => {
                    let next_idx = (curr + 1) % self.queue.len();
                    self.set_track(next_idx);
                }
                RepeatMode::Off => {
                    if curr + 1 < self.queue.len() {
                        self.set_track(curr + 1);
                    } else {
                        // End of queue
                        self.status = "paused".to_string();
                        self.start_time = None;
                        self.current_position_secs = 0.0;
                        self.current_index = Some(0); // Reset to start or None? resetting to start is common
                    }
                }
            }
        }
    }

    pub fn prev(&mut self) {
        if self.queue.is_empty() {
            return;
        }
        // If > 3 seconds in, restart track
        let pos = self.get_current_position();
        if pos > 3.0 {
            self.seek(0.0);
            return;
        }

        if let Some(curr) = self.current_index {
            if curr > 0 {
                self.set_track(curr - 1);
            } else if self.repeat_mode == RepeatMode::All {
                self.set_track(self.queue.len() - 1);
            } else {
                self.seek(0.0);
            }
        }
    }

    pub fn set_track(&mut self, index: usize) {
        if index < self.queue.len() {
            self.current_index = Some(index);
            self.current_position_secs = 0.0;
            // Auto play on track change
            self.status = "playing".to_string();
            self.start_time = Some(Utc::now());
        }
    }

    pub fn toggle_repeat(&mut self) {
        self.repeat_mode = match self.repeat_mode {
            RepeatMode::Off => RepeatMode::All,
            RepeatMode::All => RepeatMode::One,
            RepeatMode::One => RepeatMode::Off,
        };
    }

    pub fn remove_item(&mut self, index: usize) {
        if index >= self.queue.len() {
            return;
        }

        // Delete the file if it exists
        let item = &self.queue[index];
        if let Some(file_path) = &item.file_path {
            let _ = std::fs::remove_file(file_path);
        }

        // Also try to delete by ID pattern
        let temp_dir = std::path::PathBuf::from("temp");
        if temp_dir.exists() {
            if let Ok(entries) = std::fs::read_dir(&temp_dir) {
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if name.starts_with(&item.id) {
                        let _ = std::fs::remove_file(entry.path());
                    }
                }
            }
        }

        self.queue.remove(index);

        // Adjust current_index
        if let Some(curr) = self.current_index {
            if self.queue.is_empty() {
                self.current_index = None;
                self.status = "paused".to_string();
                self.start_time = None;
            } else if index == curr {
                // If removing current track, stay at same index (next track slides down)
                // but if we're at the end, go to previous
                if curr >= self.queue.len() {
                    self.current_index = Some(self.queue.len() - 1);
                }
                self.current_position_secs = 0.0;
                if self.status == "playing" {
                    self.start_time = Some(Utc::now());
                }
            } else if index < curr {
                // Decrement current_index since items shifted
                self.current_index = Some(curr - 1);
            }
        }
    }

    fn get_current_position(&self) -> f64 {
        if self.status == "paused" {
            self.current_position_secs
        } else if let Some(start) = self.start_time {
            let elapsed = (Utc::now() - start).num_milliseconds() as f64 / 1000.0;
            self.current_position_secs + elapsed
        } else {
            self.current_position_secs
        }
    }
}

/// Downloads audio from a URL using yt-dlp in two steps:
/// 1. Simulate to get metadata (title, duration) - updates status to "downloading"
/// 2. Actual download with audio extraction - updates status to "ready"
/// Determines if the input looks like a URL or a search term.
fn is_url(input: &str) -> bool {
    let input_lower = input.to_lowercase();
    // Check for common URL schemes
    if input_lower.starts_with("http://") || input_lower.starts_with("https://") {
        return true;
    }
    // Check for common video site patterns (without scheme)
    let video_patterns = [
        "youtube.com",
        "youtu.be",
        "vimeo.com",
        "dailymotion.com",
        "twitch.tv",
        "soundcloud.com",
        "bandcamp.com",
    ];
    for pattern in video_patterns {
        if input_lower.contains(pattern) {
            return true;
        }
    }
    false
}

pub fn start_download(
    url: String,
    id: String,
    state_addr: actix::Addr<crate::ws::server::ChatServer>,
    channel_id: String,
) {
    std::thread::spawn(move || {
        // Determine if input is a URL or a search term
        let effective_url = if is_url(&url) {
            url.clone()
        } else {
            // Treat as a search term - use ytsearch1: to get first result
            format!("ytsearch1:{}", url)
        };
        log::info!(
            "Input '{}' resolved to effective URL: {}",
            url,
            effective_url
        );

        // Step 1: Simulate to get metadata
        log::info!("Step 1: Getting metadata for url={}", effective_url);

        let sim_output = Command::new("yt-dlp")
            .arg("--simulate")
            .arg("--print-json")
            .arg(&effective_url)
            .output();

        let (title, duration) = match sim_output {
            Ok(out) if out.status.success() => {
                let stdout = String::from_utf8_lossy(&out.stdout);
                let json_line = stdout.lines().last().unwrap_or("{}");

                if let Ok(info) = serde_json::from_str::<serde_json::Value>(json_line) {
                    let title = info["title"]
                        .as_str()
                        .unwrap_or("Unknown Title")
                        .to_string();
                    let duration = info["duration"].as_f64().unwrap_or(0.0) as u64;
                    log::info!(
                        "Metadata extracted: title={}, duration={}s",
                        title,
                        duration
                    );

                    // Send metadata update (grabbing -> downloading)
                    state_addr.do_send(crate::ws::server::SharePlayMetadataResult {
                        channel_id: channel_id.clone(),
                        id: id.clone(),
                        success: true,
                        title: title.clone(),
                        duration,
                        error: None,
                    });

                    (title, duration)
                } else {
                    log::error!("Failed to parse metadata JSON: {}", json_line);
                    state_addr.do_send(crate::ws::server::SharePlayMetadataResult {
                        channel_id,
                        id,
                        success: false,
                        title: "".to_string(),
                        duration: 0,
                        error: Some("Failed to parse metadata".to_string()),
                    });
                    return;
                }
            }
            Ok(out) => {
                let stderr = String::from_utf8_lossy(&out.stderr);
                log::error!("yt-dlp simulate failed: {}", stderr);
                state_addr.do_send(crate::ws::server::SharePlayMetadataResult {
                    channel_id,
                    id,
                    success: false,
                    title: "".to_string(),
                    duration: 0,
                    error: Some(format!("Failed to get metadata: {}", stderr)),
                });
                return;
            }
            Err(e) => {
                log::error!("Failed to execute yt-dlp: {}", e);
                state_addr.do_send(crate::ws::server::SharePlayMetadataResult {
                    channel_id,
                    id,
                    success: false,
                    title: "".to_string(),
                    duration: 0,
                    error: Some(format!("yt-dlp execution failed: {}", e)),
                });
                return;
            }
        };

        // Step 2: Actual download
        log::info!("Step 2: Downloading audio for url={}", effective_url);

        let temp_dir = std::path::PathBuf::from("temp");
        if let Err(e) = std::fs::create_dir_all(&temp_dir) {
            log::error!("Failed to create temp directory: {}", e);
        }

        let output_template = temp_dir.join(format!("{}.%(ext)s", id));

        let download_output = Command::new("yt-dlp")
            .arg("-x") // Extract audio
            .arg("--audio-format")
            .arg("opus")
            .arg("--audio-quality")
            .arg("0") // Best quality
            //.arg("--extractor-args")
            //.arg("youtube:player_client=default,-android_sdkless")
            .arg("--cookies-from-browser")
            .arg("firefox")
            .arg("-o")
            .arg(output_template.to_str().unwrap())
            .arg(&effective_url)
            .output();

        match download_output {
            Ok(out) if out.status.success() => {
                let file_path = find_downloaded_file(&temp_dir, &id);
                log::info!("Download success: file={:?}", file_path);

                if let Some(path) = file_path {
                    state_addr.do_send(crate::ws::server::SharePlayDownloadResult {
                        channel_id,
                        id,
                        success: true,
                        title,
                        file_path: Some(path),
                        duration,
                        error: None,
                    });
                } else {
                    log::error!("yt-dlp reported success but file not found in temp/");
                    state_addr.do_send(crate::ws::server::SharePlayDownloadResult {
                        channel_id,
                        id,
                        success: false,
                        title: "".to_string(),
                        file_path: None,
                        duration: 0,
                        error: Some("Downloaded file not found".to_string()),
                    });
                }
            }
            Ok(out) => {
                let stderr = String::from_utf8_lossy(&out.stderr);
                log::error!("yt-dlp download failed: {}", stderr);
                state_addr.do_send(crate::ws::server::SharePlayDownloadResult {
                    channel_id,
                    id,
                    success: false,
                    title: "".to_string(),
                    file_path: None,
                    duration: 0,
                    error: Some(format!("Download failed: {}", stderr)),
                });
            }
            Err(e) => {
                log::error!("Failed to execute yt-dlp: {}", e);
                state_addr.do_send(crate::ws::server::SharePlayDownloadResult {
                    channel_id,
                    id,
                    success: false,
                    title: "".to_string(),
                    file_path: None,
                    duration: 0,
                    error: Some(format!("yt-dlp execution failed: {}", e)),
                });
            }
        }
    });
}

/// Find the downloaded file by ID in the temp directory.
/// yt-dlp may change the extension, so we search for files starting with the ID.
fn find_downloaded_file(temp_dir: &std::path::Path, id: &str) -> Option<String> {
    if let Ok(entries) = std::fs::read_dir(temp_dir) {
        for entry in entries.flatten() {
            let file_name = entry.file_name();
            let file_name_str = file_name.to_string_lossy();
            if file_name_str.starts_with(id) {
                if let Some(path_str) = entry.path().to_str() {
                    return Some(path_str.to_string());
                }
            }
        }
    }
    None
}

/// Delete all downloaded files for a SharePlay state
pub fn cleanup_channel_files(state: &SharePlayState) {
    let temp_dir = std::path::PathBuf::from("temp");
    for item in &state.queue {
        if let Some(file_path) = &item.file_path {
            if let Err(e) = std::fs::remove_file(file_path) {
                log::warn!("Failed to delete SharePlay file {}: {}", file_path, e);
            } else {
                log::info!("Deleted SharePlay file: {}", file_path);
            }
        }
        // Also try to delete by ID pattern (for partially downloaded files or if file_path was wrong)
        if temp_dir.exists() {
            if let Ok(entries) = std::fs::read_dir(&temp_dir) {
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if name.starts_with(&item.id) {
                        let _ = std::fs::remove_file(entry.path());
                    }
                }
            }
        }
    }
}
