use actix_multipart::Multipart;
use actix_web::{web, App, HttpResponse, HttpServer, Result};
use futures_util::stream::StreamExt;
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::Path;
use std::process::Stdio;
use tempfile::NamedTempFile;
use tokio::process::Command;
use uuid::Uuid;
use std::time::Instant;

#[derive(Serialize)]
struct HealthResponse {
    status: String,
    version: String,
}

#[derive(Serialize)]
struct UploadResponse {
    job_id: String,
    status: String,
}

#[derive(Serialize)]
struct StatusResponse {
    status: String,
    progress: u8,
    detected_green: Option<String>,
    error: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
struct JobStatus {
    status: String,
    progress: u8,
    result_path: Option<String>,
    detected_green: Option<String>,
    error: Option<String>,
}

#[derive(Deserialize, Clone)]
struct ConversionOptions {
    #[serde(default = "default_crf")]
    crf: u8,
    #[serde(default = "default_audio_bitrate")]
    audio_bitrate: String,
    #[serde(default)]
    detect_green: bool,
}

fn default_crf() -> u8 {
    30
}

fn default_audio_bitrate() -> String {
    "128k".to_string()
}

fn get_job_path(job_id: &str) -> String {
    format!("/tmp/jobs/{}.json", job_id)
}

async fn save_job_status(job_id: &str, status: &JobStatus) -> Result<(), std::io::Error> {
    let path = get_job_path(job_id);
    let json = serde_json::to_string(status)?;
    tokio::fs::write(&path, json).await?;
    Ok(())
}

async fn load_job_status(job_id: &str) -> Result<Option<JobStatus>, std::io::Error> {
    let path = get_job_path(job_id);
    
    if !Path::new(&path).exists() {
        return Ok(None);
    }
    
    match tokio::fs::read_to_string(&path).await {
        Ok(json) => {
            match serde_json::from_str::<JobStatus>(&json) {
                Ok(status) => Ok(Some(status)),
                Err(_) => Ok(None)
            }
        }
        Err(_) => Ok(None)
    }
}

async fn update_job_progress(job_id: &str, progress: u8) -> Result<(), std::io::Error> {
    if let Some(mut status) = load_job_status(job_id).await? {
        status.progress = progress;
        save_job_status(job_id, &status).await?;
    }
    Ok(())
}

#[actix_web::get("/health")]
async fn health() -> Result<HttpResponse> {
    Ok(HttpResponse::Ok().json(HealthResponse {
        status: "healthy".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    }))
}

async fn save_upload(
    mut payload: Multipart,
) -> Result<(NamedTempFile, ConversionOptions), actix_web::Error> {
    let mut temp_file = NamedTempFile::new().map_err(|e| {
        actix_web::error::ErrorInternalServerError(format!("Failed to create temp file: {}", e))
    })?;

    let mut options = ConversionOptions {
        crf: default_crf(),
        audio_bitrate: default_audio_bitrate(),
        detect_green: false,
    };

    while let Some(item) = payload.next().await {
        let mut field = item?;
        let content_disposition = field.content_disposition();
        let field_name = content_disposition.get_name().unwrap_or("");

        match field_name {
            "file" => {
                while let Some(chunk) = field.next().await {
                    let data = chunk?;
                    temp_file.write_all(&data).map_err(|e| {
                        actix_web::error::ErrorInternalServerError(format!(
                            "Failed to write chunk: {}",
                            e
                        ))
                    })?;
                }
            }
            "crf" => {
                let mut value = String::new();
                while let Some(chunk) = field.next().await {
                    let data = chunk?;
                    value.push_str(&String::from_utf8_lossy(&data));
                }
                if let Ok(crf) = value.parse::<u8>() {
                    options.crf = crf.clamp(0, 63);
                }
            }
            "audio_bitrate" => {
                let mut value = String::new();
                while let Some(chunk) = field.next().await {
                    let data = chunk?;
                    value.push_str(&String::from_utf8_lossy(&data));
                }
                options.audio_bitrate = value;
            }
            "detect_green" => {
                let mut value = String::new();
                while let Some(chunk) = field.next().await {
                    let data = chunk?;
                    value.push_str(&String::from_utf8_lossy(&data));
                }
                options.detect_green = value.trim() == "true";
            }
            _ => {}
        }
    }

    temp_file.flush().map_err(|e| {
        actix_web::error::ErrorInternalServerError(format!("Failed to flush temp file: {}", e))
    })?;

    Ok((temp_file, options))
}

async fn detect_green_color(input_path: &str) -> Result<String, std::io::Error> {
    let probe_output = Command::new("ffprobe")
        .args(&[
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=width,height",
            "-of",
            "csv=p=0",
            input_path,
        ])
        .output()
        .await?;

    let dimensions = String::from_utf8_lossy(&probe_output.stdout);
    let parts: Vec<&str> = dimensions.trim().split(',').collect();

    if parts.len() < 2 {
        return Ok("0x00FF00".to_string());
    }

    let width: i32 = parts[0].parse().unwrap_or(1920);
    let height: i32 = parts[1].parse().unwrap_or(1080);

    let patch_size: i32 = 20;
    let margin: i32 = 10;

    let corners = [
        (margin, margin),
        (width - patch_size - margin, margin),
        (margin, height - patch_size - margin),
        (width - patch_size - margin, height - patch_size - margin),
    ];

    let mut r_total: u64 = 0;
    let mut g_total: u64 = 0;
    let mut b_total: u64 = 0;
    let mut pixel_count: u64 = 0;

    for &(x, y) in &corners {
        let vf = format!("crop={}:{}:{}:{}", patch_size, patch_size, x, y);

        let output = Command::new("ffmpeg")
            .args(&[
                "-i",
                input_path,
                "-vf",
                &vf,
                "-vframes",
                "1",
                "-f",
                "rawvideo",
                "-pix_fmt",
                "rgb24",
                "-",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output()
            .await?;

        let expected_bytes = (patch_size * patch_size * 3) as usize;
        if output.stdout.len() >= expected_bytes {
            let data = &output.stdout[..expected_bytes];
            for chunk in data.chunks_exact(3) {
                r_total += chunk[0] as u64;
                g_total += chunk[1] as u64;
                b_total += chunk[2] as u64;
            }
            pixel_count += (expected_bytes / 3) as u64;
        }
    }

    if pixel_count > 0 {
        let r = (r_total / pixel_count) as u8;
        let g = (g_total / pixel_count) as u8;
        let b = (b_total / pixel_count) as u8;
        println!("Detected green: R={}, G={}, B={}", r, g, b);
        return Ok(format!("0x{:02X}{:02X}{:02X}", r, g, b));
    }

    Ok("0x00FF00".to_string())
}

async fn convert_to_webm(
    input_path: &str,
    output_path: &str,
    options: &ConversionOptions,
    job_id: Option<String>,
) -> Result<(), std::io::Error> {
    let crf_string = options.crf.to_string();
    println!("Converting to VP9 WebM (CRF: {})", options.crf);
    let duration = get_video_duration(input_path).await.unwrap_or(0.0);
    let mut child = Command::new("ffmpeg")
        .args(&[
            "-i",
            input_path,
            "-c:v",
            "libvpx-vp9",
            "-crf",
            &crf_string,
            "-b:v",
            "0",
            "-cpu-used",
            "5",
            "-deadline",
            "realtime",
            "-row-mt",
            "1",
            "-threads",
            "2",
            "-c:a",
            "libopus",
            "-b:a",
            &options.audio_bitrate,
            "-progress",
            "pipe:2",
            "-f",
            "webm",
            "-y",
            output_path,
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    if let Some(job_id) = job_id {
        if duration > 0.0 {
            if let Some(stderr) = child.stderr.take() {
                use tokio::io::{AsyncBufReadExt, BufReader};
                let reader = BufReader::new(stderr);
                let mut lines = reader.lines();

                tokio::spawn(async move {
                    while let Ok(Some(line)) = lines.next_line().await {
                        if line.starts_with("out_time_ms=") {
                            if let Some(time_str) = line.strip_prefix("out_time_ms=") {
                                if let Ok(time_ms) = time_str.parse::<f64>() {
                                    let current_seconds = time_ms / 1_000_000.0;
                                    let progress = ((current_seconds / duration) * 100.0).min(99.0) as u8;
                                    let mapped_progress = 30 + ((progress * 69) / 100);
                                    let _ = update_job_progress(&job_id, mapped_progress).await;
                                }
                            }
                        }
                    }
                });
            }
        }
    }

    let output = child.wait_with_output().await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        eprintln!("FFmpeg error: {}", stderr);
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("FFmpeg failed: {}", stderr),
        ));
    }

    let metadata = tokio::fs::metadata(output_path).await?;
    println!("Conversion complete - Size: {} KB", metadata.len() / 1024);

    Ok(())
}

async fn get_video_duration(input_path: &str) -> Result<f64, std::io::Error> {
    let output = Command::new("ffprobe")
        .args(&[
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "default=noprint_wrappers=1:nokey=1",
            input_path,
        ])
        .output()
        .await?;

    let duration_str = String::from_utf8_lossy(&output.stdout);
    duration_str
        .trim()
        .parse::<f64>()
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid duration"))
}

async fn process_video(
    job_id: String,
    input_path: String,
    options: ConversionOptions,
) {
    let start_time = Instant::now();
    println!("[{}] Processing started", job_id);

    let _ = update_job_progress(&job_id, 10).await;

    let detected_green = if options.detect_green {
        println!("[{}] Starting green detection", job_id);
        let _ = update_job_progress(&job_id, 20).await;

        let detect_start = Instant::now();
        match detect_green_color(&input_path).await {
            Ok(color) => {
                println!(
                    "[{}] Green detected in {:?}: {}",
                    job_id,
                    detect_start.elapsed(),
                    color
                );
                Some(color)
            }
            Err(e) => {
                eprintln!("[{}] Green detection failed: {}", job_id, e);
                None
            }
        }
    } else {
        None
    };

    if let Ok(Some(mut status)) = load_job_status(&job_id).await {
        status.progress = 30;
        status.detected_green = detected_green.clone();
        let _ = save_job_status(&job_id, &status).await;
    }

    let output_path = format!("/tmp/results/{}.webm", job_id);
    let convert_start = Instant::now();

    match convert_to_webm(&input_path, &output_path, &options, Some(job_id.clone())).await {
        Ok(_) => {
            let total_time = start_time.elapsed();
            let convert_time = convert_start.elapsed();
            println!("[{}] === CONVERSION SUCCESS ===", job_id);
            println!(
                "[{}] Total time: {:?}, Conversion: {:?}",
                job_id, total_time, convert_time
            );

            if let Ok(Some(mut status)) = load_job_status(&job_id).await {
                status.status = "complete".to_string();
                status.progress = 100;
                status.result_path = Some(output_path);
                status.detected_green = detected_green;
                let _ = save_job_status(&job_id, &status).await;
            }
        }
        Err(e) => {
            eprintln!(
                "[{}] === CONVERSION FAILED after {:?} ===",
                job_id,
                start_time.elapsed()
            );
            eprintln!("[{}] Error: {}", job_id, e);

            if let Ok(Some(mut status)) = load_job_status(&job_id).await {
                status.status = "failed".to_string();
                status.error = Some(e.to_string());
                let _ = save_job_status(&job_id, &status).await;
            }
        }
    }

    let _ = tokio::fs::remove_file(&input_path).await;
}

#[actix_web::post("/upload")]
async fn upload_video(
    payload: Multipart,
) -> Result<HttpResponse> {
    let start_time = Instant::now();
    println!("=== UPLOAD STARTED ===");

    let (temp_file, options) = save_upload(payload).await?;
    println!("Upload saved in {:?}", start_time.elapsed());

    let job_id = Uuid::new_v4().to_string();

    let persistent_path = format!("/tmp/inputs/input_{}.tmp", job_id);
    tokio::fs::copy(temp_file.path(), &persistent_path)
        .await
        .map_err(|e| {
            actix_web::error::ErrorInternalServerError(format!("Failed to save file: {}", e))
        })?;

    let job_status = JobStatus {
        status: "processing".to_string(),
        progress: 5,
        result_path: None,
        detected_green: None,
        error: None,
    };
    
    save_job_status(&job_id, &job_status).await.map_err(|e| {
        actix_web::error::ErrorInternalServerError(format!("Failed to create job: {}", e))
    })?;

    let response_job_id = job_id.clone();

    tokio::spawn(async move {
        process_video(job_id, persistent_path, options).await;
    });

    Ok(HttpResponse::Ok().json(UploadResponse {
        job_id: response_job_id,
        status: "processing".to_string(),
    }))
}

#[actix_web::get("/status/{job_id}")]
async fn check_status(
    job_id: web::Path<String>,
) -> Result<HttpResponse> {
    match load_job_status(job_id.as_str()).await {
        Ok(Some(job)) => {
            Ok(HttpResponse::Ok().json(StatusResponse {
                status: job.status,
                progress: job.progress,
                detected_green: job.detected_green,
                error: job.error,
            }))
        }
        Ok(None) => {
            Ok(HttpResponse::NotFound().json(serde_json::json!({
                "error": "Job not found"
            })))
        }
        Err(e) => {
            eprintln!("Error loading job status: {}", e);
            Ok(HttpResponse::InternalServerError().json(serde_json::json!({
                "error": "Failed to load job status"
            })))
        }
    }
}

#[actix_web::get("/download/{job_id}")]
async fn download_result(
    job_id: web::Path<String>,
) -> Result<HttpResponse> {
    match load_job_status(job_id.as_str()).await {
        Ok(Some(job)) => {
            if job.status == "complete" {
                if let Some(path) = &job.result_path {
                    match tokio::fs::read(path).await {
                        Ok(data) => {
                            let path_clone = path.clone();
                            let job_id_clone = job_id.to_string();
                            tokio::spawn(async move {
                                let _ = tokio::fs::remove_file(&path_clone).await;
                                let _ = tokio::fs::remove_file(&get_job_path(&job_id_clone)).await;
                            });

                            let mut builder = HttpResponse::Ok();
                            builder.content_type("video/webm");

                            if let Some(green) = &job.detected_green {
                                builder.insert_header(("X-Detected-Green", green.clone()));
                            }

                            return Ok(builder.body(data));
                        }
                        Err(e) => {
                            return Ok(HttpResponse::InternalServerError().json(serde_json::json!({
                                "error": "Failed to read result file",
                                "details": e.to_string()
                            })));
                        }
                    }
                }
            }

            Ok(HttpResponse::BadRequest().json(serde_json::json!({
                "error": "Video not ready",
                "status": job.status,
                "progress": job.progress
            })))
        }
        Ok(None) => {
            Ok(HttpResponse::NotFound().json(serde_json::json!({
                "error": "Job not found"
            })))
        }
        Err(e) => {
            eprintln!("Error loading job: {}", e);
            Ok(HttpResponse::InternalServerError().json(serde_json::json!({
                "error": "Failed to load job"
            })))
        }
    }
}



#[actix_web::main]
async fn main() -> std::io::Result<()> {
    std::fs::create_dir_all("/tmp/jobs")?;
    std::fs::create_dir_all("/tmp/inputs")?;
    std::fs::create_dir_all("/tmp/results")?;

    let port = std::env::var("PORT")
        .unwrap_or_else(|_| "8666".to_string())
        .parse::<u16>()
        .unwrap_or(8666);

    println!("Video converter server starting on port {}", port);
    println!("Async job processing enabled");
    println!("Endpoints:");
    println!("  POST /upload - Upload video and get job_id");
    println!("  GET  /status/{{job_id}} - Check conversion status");
    println!("  GET  /download/{{job_id}} - Download converted video");

    HttpServer::new(move || {
        App::new()
            .service(health)
            .service(upload_video)
            .service(check_status)
            .service(download_result)
    })
    .workers(4)
    .bind(("0.0.0.0", port))?
    .run()
    .await
}