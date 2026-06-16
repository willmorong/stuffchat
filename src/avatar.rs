use crate::errors::ApiError;
use image::ImageEncoder;
use image::imageops::FilterType;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const AVATAR_SIZE: u32 = 1024;
pub const AVATAR_ORIGINAL_NAME: &str = "avatar.avif";
pub const AVATAR_MIME_TYPE: &str = "image/avif";
const ANIMATED_AVATAR_TIMESCALE: u64 = 1000;
const DEFAULT_FRAME_DURATION_MS: u64 = 100;
const MAX_FRAME_DURATION_MS: u64 = 60_000;
const AVATAR_FILTER_RGBA: &str =
    "scale=1024:1024:force_original_aspect_ratio=increase,crop=1024:1024,setsar=1,format=rgba";

pub struct ProcessedAvatar {
    pub bytes: Vec<u8>,
}

pub fn process_avatar_upload(
    bytes: &[u8],
    original_filename: &str,
) -> Result<ProcessedAvatar, ApiError> {
    if bytes.is_empty() {
        return Err(ApiError::BadRequest("empty avatar file".into()));
    }

    if is_animated(bytes, original_filename)? {
        process_animated_avatar(bytes, original_filename)
    } else {
        process_still_avatar(bytes, original_filename)
    }
}

fn process_still_avatar(
    bytes: &[u8],
    original_filename: &str,
) -> Result<ProcessedAvatar, ApiError> {
    let img = match image::load_from_memory(bytes) {
        Ok(img) => img,
        Err(_) => return process_still_avatar_with_ffmpeg(bytes, original_filename),
    };
    let avatar = img.resize_to_fill(AVATAR_SIZE, AVATAR_SIZE, FilterType::Lanczos3);
    let rgba = avatar.to_rgba8();

    let mut output = Vec::new();
    {
        let encoder = image::codecs::avif::AvifEncoder::new_with_speed_quality(&mut output, 6, 80);
        encoder
            .write_image(
                rgba.as_raw(),
                AVATAR_SIZE,
                AVATAR_SIZE,
                image::ExtendedColorType::Rgba8,
            )
            .map_err(|err| {
                log::error!("avatar avif encode failed: {err:?}");
                ApiError::Internal
            })?;
    }

    Ok(ProcessedAvatar { bytes: output })
}

fn process_still_avatar_with_ffmpeg(
    bytes: &[u8],
    original_filename: &str,
) -> Result<ProcessedAvatar, ApiError> {
    let temp = AvatarTempFiles::new(original_filename);
    std::fs::write(&temp.input, bytes).map_err(|err| {
        log::error!("failed to write temporary avatar input: {err:?}");
        ApiError::Internal
    })?;

    let output = Command::new("ffmpeg")
        .arg("-hide_banner")
        .arg("-loglevel")
        .arg("error")
        .arg("-y")
        .arg("-i")
        .arg(&temp.input)
        .arg("-frames:v")
        .arg("1")
        .arg("-vf")
        .arg("scale=1024:1024:force_original_aspect_ratio=increase,crop=1024:1024,setsar=1,format=yuv420p")
        .arg("-an")
        .arg("-c:v")
        .arg("libaom-av1")
        .arg("-still-picture")
        .arg("1")
        .arg("-crf")
        .arg("30")
        .arg("-cpu-used")
        .arg("6")
        .arg("-f")
        .arg("avif")
        .arg(&temp.output)
        .output()
        .map_err(|err| {
            log::error!("failed to run ffmpeg for still avatar: {err:?}");
            ApiError::BadRequest("invalid image file".into())
        })?;

    if !output.status.success() {
        log::error!(
            "ffmpeg still avatar conversion failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        return Err(ApiError::BadRequest("invalid image file".into()));
    }

    let output = std::fs::read(&temp.output).map_err(|err| {
        log::error!("failed to read temporary avatar avif: {err:?}");
        ApiError::Internal
    })?;
    if output.is_empty() {
        return Err(ApiError::Internal);
    }

    Ok(ProcessedAvatar { bytes: output })
}

fn process_animated_avatar(
    bytes: &[u8],
    original_filename: &str,
) -> Result<ProcessedAvatar, ApiError> {
    ensure_tool_available("ffmpeg")?;
    ensure_tool_available("ffprobe")?;
    ensure_animation_tool_available("avifenc")?;

    let temp = AvatarTempFiles::new(original_filename);
    std::fs::write(&temp.input, bytes).map_err(|err| {
        log::error!("failed to write temporary avatar input: {err:?}");
        ApiError::Internal
    })?;

    let durations = probe_frame_durations(&temp.input);
    let frame_paths = extract_animation_frames(&temp.input, &temp.frame_dir)?;
    let output = encode_animated_avif(&frame_paths, &durations, &temp.output)?;

    if !is_supported_avif_image_container(&output) {
        log::error!("libavif produced an unsupported avatar container");
        return Err(ApiError::Internal);
    }

    Ok(ProcessedAvatar { bytes: output })
}

fn extract_animation_frames(input: &Path, frame_dir: &Path) -> Result<Vec<PathBuf>, ApiError> {
    std::fs::create_dir_all(frame_dir).map_err(|err| {
        log::error!("failed to create temporary avatar frame directory: {err:?}");
        ApiError::Internal
    })?;

    let frame_pattern = frame_dir.join("frame-%06d.png");
    let output = Command::new("ffmpeg")
        .arg("-hide_banner")
        .arg("-loglevel")
        .arg("error")
        .arg("-y")
        .arg("-i")
        .arg(input)
        .arg("-vsync")
        .arg("0")
        .arg("-vf")
        .arg(AVATAR_FILTER_RGBA)
        .arg("-an")
        .arg(&frame_pattern)
        .output()
        .map_err(|err| {
            log::error!("failed to run ffmpeg for animated avatar frames: {err:?}");
            ApiError::Internal
        })?;

    if !output.status.success() {
        log::error!(
            "ffmpeg animated avatar frame extraction failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        return Err(ApiError::BadRequest("invalid animated avatar image".into()));
    }

    let mut frames = std::fs::read_dir(frame_dir)
        .map_err(|err| {
            log::error!("failed to read temporary avatar frame directory: {err:?}");
            ApiError::Internal
        })?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("png"))
        .collect::<Vec<_>>();
    frames.sort();

    if frames.len() < 2 {
        log::error!("animated avatar produced fewer than two frames");
        return Err(ApiError::BadRequest("invalid animated avatar image".into()));
    }

    Ok(frames)
}

fn encode_animated_avif(
    frame_paths: &[PathBuf],
    durations: &[u64],
    output_path: &Path,
) -> Result<Vec<u8>, ApiError> {
    let durations = durations_for_frame_count(durations, frame_paths.len());

    let mut command = Command::new("avifenc");
    command
        .arg("--qcolor")
        .arg("80")
        .arg("--qalpha")
        .arg("80")
        .arg("--speed")
        .arg("6")
        .arg("--depth")
        .arg("8")
        .arg("--yuv")
        .arg("420")
        .arg("--timescale")
        .arg(ANIMATED_AVATAR_TIMESCALE.to_string())
        .arg("--repetition-count")
        .arg("infinite")
        .arg("--ignore-exif")
        .arg("--ignore-xmp")
        .arg("--ignore-profile");

    for (path, duration) in frame_paths.iter().zip(durations.iter()) {
        command
            .arg("--duration")
            .arg(duration.to_string())
            .arg(path);
    }
    command.arg(output_path);

    let output = command.output().map_err(|err| {
        log::error!("failed to run avifenc for animated avatar: {err:?}");
        ApiError::Internal
    })?;

    if !output.status.success() {
        log::error!(
            "avifenc animated avatar conversion failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        return Err(ApiError::BadRequest("invalid animated avatar image".into()));
    }

    let bytes = std::fs::read(output_path).map_err(|err| {
        log::error!("failed to read temporary animated avatar avif: {err:?}");
        ApiError::Internal
    })?;
    if bytes.is_empty() {
        return Err(ApiError::Internal);
    }
    Ok(bytes)
}

#[derive(Deserialize)]
struct ProbeJson {
    #[serde(default)]
    streams: Vec<ProbeStream>,
    #[serde(default)]
    frames: Vec<ProbeFrame>,
}

#[derive(Deserialize)]
struct ProbeStream {
    duration: Option<String>,
    nb_frames: Option<String>,
    avg_frame_rate: Option<String>,
    r_frame_rate: Option<String>,
}

#[derive(Deserialize)]
struct ProbeFrame {
    best_effort_timestamp_time: Option<String>,
    pkt_duration_time: Option<String>,
}

fn probe_frame_durations(input: &Path) -> Vec<u64> {
    let output = Command::new("ffprobe")
        .arg("-v")
        .arg("error")
        .arg("-select_streams")
        .arg("v:0")
        .arg("-show_entries")
        .arg("stream=duration,nb_frames,avg_frame_rate,r_frame_rate:frame=best_effort_timestamp_time,pkt_duration_time")
        .arg("-of")
        .arg("json")
        .arg(input)
        .output();

    let Ok(output) = output else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }

    let Ok(probe) = serde_json::from_slice::<ProbeJson>(&output.stdout) else {
        return Vec::new();
    };

    let mut durations = probe
        .frames
        .iter()
        .map(|frame| frame.pkt_duration_time.as_deref().and_then(duration_ms))
        .collect::<Option<Vec<_>>>()
        .unwrap_or_default();
    if !durations.is_empty() {
        return durations;
    }

    let timestamps = probe
        .frames
        .iter()
        .filter_map(|frame| {
            frame
                .best_effort_timestamp_time
                .as_deref()
                .and_then(|value| value.parse::<f64>().ok())
        })
        .collect::<Vec<_>>();
    if timestamps.len() > 1 {
        durations = timestamps
            .windows(2)
            .filter_map(|pair| duration_ms_from_seconds(pair[1] - pair[0]))
            .collect();

        let stream_duration = probe
            .streams
            .first()
            .and_then(|stream| stream.duration.as_deref())
            .and_then(|value| value.parse::<f64>().ok());
        let last_duration = stream_duration
            .and_then(|duration| {
                timestamps
                    .last()
                    .and_then(|last| duration_ms_from_seconds(duration - last))
            })
            .or_else(|| durations.last().copied())
            .unwrap_or(DEFAULT_FRAME_DURATION_MS);
        durations.push(last_duration);
        return durations;
    }

    let Some(stream) = probe.streams.first() else {
        return Vec::new();
    };
    let Some(frame_count) = stream
        .nb_frames
        .as_deref()
        .and_then(|value| value.parse::<usize>().ok())
    else {
        return Vec::new();
    };
    let duration = stream
        .duration
        .as_deref()
        .and_then(duration_ms)
        .and_then(|duration| duration.checked_div(frame_count as u64))
        .or_else(|| {
            stream
                .avg_frame_rate
                .as_deref()
                .or(stream.r_frame_rate.as_deref())
                .and_then(duration_ms_from_rate)
        })
        .unwrap_or(DEFAULT_FRAME_DURATION_MS);
    vec![duration; frame_count]
}

fn duration_ms(value: &str) -> Option<u64> {
    value.parse::<f64>().ok().and_then(duration_ms_from_seconds)
}

fn duration_ms_from_seconds(value: f64) -> Option<u64> {
    if !value.is_finite() || value <= 0.0 {
        return None;
    }
    Some((value * ANIMATED_AVATAR_TIMESCALE as f64).round() as u64)
}

fn duration_ms_from_rate(value: &str) -> Option<u64> {
    let (num, den) = value.split_once('/')?;
    let num = num.parse::<f64>().ok()?;
    let den = den.parse::<f64>().ok()?;
    if !num.is_finite() || !den.is_finite() || num <= 0.0 || den <= 0.0 {
        return None;
    }
    Some(((den / num) * ANIMATED_AVATAR_TIMESCALE as f64).round() as u64)
}

fn durations_for_frame_count(durations: &[u64], frame_count: usize) -> Vec<u64> {
    let mut out = durations
        .iter()
        .copied()
        .map(|duration| duration.clamp(1, MAX_FRAME_DURATION_MS))
        .collect::<Vec<_>>();

    if out.is_empty() {
        out.push(DEFAULT_FRAME_DURATION_MS);
    }
    out.truncate(frame_count);
    while out.len() < frame_count {
        out.push(*out.last().unwrap_or(&DEFAULT_FRAME_DURATION_MS));
    }
    out
}

fn is_supported_avif_image_container(bytes: &[u8]) -> bool {
    let Some(brand) = bytes.get(4..12) else {
        return false;
    };
    if brand != b"ftypavif" && brand != b"ftypavis" {
        return false;
    }

    let header_len = bytes.len().min(4096);
    let header = &bytes[..header_len];
    let has_image_meta = header.windows(4).any(|window| window == b"meta")
        && header
            .windows(4)
            .any(|window| window == b"pitm" || window == b"pict");
    let has_mp4_video_brand = header
        .windows(4)
        .any(|window| window == b"mp41" || window == b"mp42");
    has_image_meta && !has_mp4_video_brand
}

fn is_animated(bytes: &[u8], original_filename: &str) -> Result<bool, ApiError> {
    if !looks_animation_capable(bytes, original_filename) {
        return Ok(false);
    }

    ensure_tool_available("ffprobe")?;

    let temp = AvatarTempFiles::new(original_filename);
    std::fs::write(&temp.input, bytes).map_err(|err| {
        log::error!("failed to write temporary avatar probe input: {err:?}");
        ApiError::Internal
    })?;

    let output = Command::new("ffprobe")
        .arg("-v")
        .arg("error")
        .arg("-select_streams")
        .arg("v:0")
        .arg("-count_frames")
        .arg("-show_entries")
        .arg("stream=nb_read_frames")
        .arg("-of")
        .arg("default=nokey=1:noprint_wrappers=1")
        .arg(&temp.input)
        .output()
        .map_err(|err| {
            log::error!("failed to run ffprobe for avatar: {err:?}");
            ApiError::Internal
        })?;

    if !output.status.success() {
        return Ok(false);
    }

    let text = String::from_utf8_lossy(&output.stdout);
    let frames = text.trim().parse::<u32>().unwrap_or(1);
    Ok(frames > 1)
}

fn looks_animation_capable(bytes: &[u8], original_filename: &str) -> bool {
    let lower = original_filename.to_ascii_lowercase();
    lower.ends_with(".gif")
        || lower.ends_with(".webp")
        || lower.ends_with(".apng")
        || lower.ends_with(".avif")
        || bytes.starts_with(b"GIF87a")
        || bytes.starts_with(b"GIF89a")
        || bytes.windows(4).any(|window| window == b"acTL")
        || bytes.windows(4).any(|window| window == b"ANIM")
        || bytes.windows(4).any(|window| window == b"avis")
}

fn ensure_tool_available(tool: &str) -> Result<(), ApiError> {
    let status = Command::new(tool).arg("-version").output().map_err(|err| {
        log::error!("{tool} is required for animated avatar processing: {err:?}");
        ApiError::BadRequest("animated avatars require ffmpeg and ffprobe".into())
    })?;

    if status.status.success() {
        Ok(())
    } else {
        log::error!("{tool} is required for animated avatar processing");
        Err(ApiError::BadRequest(
            "animated avatars require ffmpeg and ffprobe".into(),
        ))
    }
}

fn ensure_animation_tool_available(tool: &str) -> Result<(), ApiError> {
    let status = Command::new(tool)
        .arg("--version")
        .output()
        .map_err(|err| {
            log::error!("{tool} is required for animated avatar processing: {err:?}");
            ApiError::BadRequest("animated avatars require ffmpeg, ffprobe, and avifenc".into())
        })?;

    if status.status.success() {
        Ok(())
    } else {
        log::error!("{tool} is required for animated avatar processing");
        Err(ApiError::BadRequest(
            "animated avatars require ffmpeg, ffprobe, and avifenc".into(),
        ))
    }
}

struct AvatarTempFiles {
    input: PathBuf,
    output: PathBuf,
    frame_dir: PathBuf,
}

impl AvatarTempFiles {
    fn new(original_filename: &str) -> Self {
        let id = uuid::Uuid::new_v4();
        let ext = Path::new(original_filename)
            .extension()
            .and_then(|value| value.to_str())
            .filter(|value| !value.is_empty())
            .unwrap_or("img");
        let dir = std::env::temp_dir();
        Self {
            input: dir.join(format!("stuffchat-avatar-{id}.{ext}")),
            output: dir.join(format!("stuffchat-avatar-{id}.avif")),
            frame_dir: dir.join(format!("stuffchat-avatar-{id}-frames")),
        }
    }
}

impl Drop for AvatarTempFiles {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.input);
        let _ = std::fs::remove_file(&self.output);
        let _ = std::fs::remove_dir_all(&self.frame_dir);
    }
}
