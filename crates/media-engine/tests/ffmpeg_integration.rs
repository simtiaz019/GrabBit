#![cfg(unix)]

use std::{
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

use media_engine::{MediaDownloadRequest, MediaOutput, MediaStage, ToolConfig, YtDlpAdapter};
use sha2::{Digest, Sha256};
use tempfile::tempdir;
use tokio::{fs, process::Command, time::sleep};
use tokio_util::sync::CancellationToken;

fn tool(name: &str) -> Option<PathBuf> {
    std::env::var_os(format!("GRABBIT_{}", name.to_ascii_uppercase()))
        .map(PathBuf::from)
        .filter(|path| path.is_file())
        .or_else(|| {
            ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin"]
                .iter()
                .map(|root| Path::new(root).join(name))
                .find(|path| path.is_file())
        })
}

async fn run(path: &Path, args: &[&str]) -> bool {
    Command::new(path)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await
        .is_ok_and(|status| status.success())
}

async fn probe(ffprobe: &Path, input: &Path) -> String {
    let output = Command::new(ffprobe)
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=format_name,duration:stream=codec_name,codec_type,width,height",
            "-of",
            "json",
        ])
        .arg(input)
        .output()
        .await
        .expect("ffprobe starts");
    assert!(
        output.status.success(),
        "ffprobe rejected {}",
        input.display()
    );
    String::from_utf8(output.stdout).expect("UTF-8 probe output")
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn creates_and_verifies_local_media_matrix() {
    let (Some(ffmpeg), Some(ffprobe)) = (tool("ffmpeg"), tool("ffprobe")) else {
        eprintln!("SKIP: FFmpeg/FFprobe are not installed");
        return;
    };
    let temp = tempdir().expect("tempdir");
    let root = temp.path();
    let mp4 = root.join("sample.mp4");
    assert!(
        run(
            &ffmpeg,
            &[
                "-y",
                "-f",
                "lavfi",
                "-i",
                "testsrc=size=160x90:rate=15",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=880:sample_rate=48000",
                "-t",
                "1",
                "-c:v",
                "mpeg4",
                "-pix_fmt",
                "yuv420p",
                "-c:a",
                "aac",
                "-shortest",
                mp4.to_str().expect("path")
            ]
        )
        .await
    );
    let webm = root.join("sample.webm");
    assert!(
        run(
            &ffmpeg,
            &[
                "-y",
                "-f",
                "lavfi",
                "-i",
                "testsrc=size=160x90:rate=15",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:sample_rate=48000",
                "-t",
                "1",
                "-c:v",
                "libvpx-vp9",
                "-c:a",
                "libopus",
                "-shortest",
                webm.to_str().expect("path")
            ]
        )
        .await
    );
    let m4a = root.join("audio.m4a");
    assert!(
        run(
            &ffmpeg,
            &[
                "-y",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=330:sample_rate=48000",
                "-t",
                "1",
                "-c:a",
                "aac",
                m4a.to_str().expect("path")
            ]
        )
        .await
    );
    let opus = root.join("audio.opus");
    assert!(
        run(
            &ffmpeg,
            &[
                "-y",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=550:sample_rate=48000",
                "-t",
                "1",
                "-c:a",
                "libopus",
                opus.to_str().expect("path")
            ]
        )
        .await
    );
    let video_only = root.join("video-only.mp4");
    let audio_only = root.join("audio-only.m4a");
    assert!(
        run(
            &ffmpeg,
            &[
                "-y",
                "-i",
                mp4.to_str().expect("path"),
                "-an",
                "-c:v",
                "copy",
                video_only.to_str().expect("path")
            ]
        )
        .await
    );
    assert!(
        run(
            &ffmpeg,
            &[
                "-y",
                "-i",
                mp4.to_str().expect("path"),
                "-vn",
                "-c:a",
                "copy",
                audio_only.to_str().expect("path")
            ]
        )
        .await
    );
    let merged = root.join("merged.mp4");
    assert!(
        run(
            &ffmpeg,
            &[
                "-y",
                "-i",
                video_only.to_str().expect("path"),
                "-i",
                audio_only.to_str().expect("path"),
                "-c",
                "copy",
                "-shortest",
                merged.to_str().expect("path")
            ]
        )
        .await
    );
    let hls = root.join("hls");
    fs::create_dir(&hls).await.expect("hls dir");
    assert!(
        run(
            &ffmpeg,
            &[
                "-y",
                "-i",
                mp4.to_str().expect("path"),
                "-c",
                "copy",
                "-hls_time",
                "0.4",
                "-hls_list_size",
                "0",
                hls.join("index.m3u8").to_str().expect("path")
            ]
        )
        .await
    );
    let dash = root.join("dash");
    fs::create_dir(&dash).await.expect("dash dir");
    assert!(
        run(
            &ffmpeg,
            &[
                "-y",
                "-i",
                mp4.to_str().expect("path"),
                "-map",
                "0:v",
                "-map",
                "0:a",
                "-c",
                "copy",
                "-f",
                "dash",
                dash.join("manifest.mpd").to_str().expect("path")
            ]
        )
        .await
    );

    for file in [&mp4, &webm, &m4a, &opus, &video_only, &audio_only, &merged] {
        let metadata = probe(&ffprobe, file).await;
        assert!(metadata.contains("codec_name"));
        let bytes = fs::read(file).await.expect("fixture bytes");
        assert_ne!(bytes.len(), 0);
        assert_eq!(sha256(&bytes).len(), 64);
    }
    assert!(
        fs::read_to_string(hls.join("index.m3u8"))
            .await
            .expect("HLS manifest")
            .contains("#EXTM3U")
    );
    assert!(
        fs::read_to_string(dash.join("manifest.mpd"))
            .await
            .expect("DASH manifest")
            .contains("<MPD")
    );

    let corrupt = root.join("corrupt.mp4");
    fs::write(&corrupt, b"not media")
        .await
        .expect("corrupt fixture");
    assert!(!run(&ffprobe, &["-v", "error", corrupt.to_str().expect("path")]).await);
    assert!(
        !run(
            &ffmpeg,
            &[
                "-y",
                "-i",
                opus.to_str().expect("path"),
                root.join("invalid.xyz").to_str().expect("path")
            ]
        )
        .await
    );
}

#[tokio::test]
async fn adapter_reports_progress_and_cancels_child_process() {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempdir().expect("tempdir");
    let source = temp.path().join("source.mp4");
    fs::write(&source, b"synthetic-media")
        .await
        .expect("source");
    let yt_dlp = temp.path().join("yt-dlp");
    let script = format!(
        r#"#!/bin/bash
if [[ "$1" == "--version" ]]; then echo test-yt-dlp; exit 0; fi
out=""; last=""; js_runtime=""
for ((i=1;i<=$#;i++)); do arg="${{!i}}"; if [[ "$last" == "--output" ]]; then out="$arg"; fi; if [[ "$last" == "--js-runtimes" ]]; then js_runtime="$arg"; fi; last="$arg"; done
if [[ "$js_runtime" != deno:* ]]; then echo 'missing bundled JavaScript runtime' >&2; exit 7; fi
url="${{!#}}"
if [[ "$url" == *slow* ]]; then /bin/sleep 10; fi
out="${{out//%(ext)s/mp4}}"
echo 'grabbit-progress|download|7|15|NA|7|1'
/bin/sleep 0.05
echo 'grabbit-progress|download|15|15|NA|8|0'
echo 'grabbit-progress|postprocess|NA|NA|NA|NA|NA'
/bin/cp '{}' "$out"
"#,
        source.display()
    );
    fs::write(&yt_dlp, script).await.expect("script");
    let mut permissions = fs::metadata(&yt_dlp).await.expect("metadata").permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(&yt_dlp, permissions)
        .await
        .expect("permissions");
    let helper = temp.path().join("helper");
    fs::write(&helper, b"#!/bin/sh\nexit 0\n")
        .await
        .expect("helper");
    let mut permissions = fs::metadata(&helper).await.expect("metadata").permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(&helper, permissions)
        .await
        .expect("permissions");
    let adapter = YtDlpAdapter::new(
        ToolConfig::from_paths(yt_dlp, helper.clone(), helper.clone()).with_js_runtime(helper),
    )
    .expect("adapter");
    let output = temp.path().join("result.mp4");
    let (sender, mut receiver) = tokio::sync::mpsc::channel(8);
    adapter
        .download(
            &MediaDownloadRequest {
                url: "https://media.example/test".into(),
                format_ids: vec!["v1".into()],
                output: MediaOutput::Mp4,
                output_path: output.clone(),
                authorization_confirmed: true,
            },
            CancellationToken::new(),
            Some(sender),
        )
        .await
        .expect("download");
    let mut stages = Vec::new();
    while let Some(event) = receiver.recv().await {
        stages.push(event.stage);
    }
    assert!(stages.contains(&MediaStage::Downloading));
    assert!(stages.contains(&MediaStage::PostProcessing));
    assert_eq!(fs::read(&output).await.expect("output"), b"synthetic-media");

    let cancellation = CancellationToken::new();
    let cancel_clone = cancellation.clone();
    let request = MediaDownloadRequest {
        url: "https://media.example/slow".into(),
        format_ids: vec!["v1".into()],
        output: MediaOutput::Mp4,
        output_path: temp.path().join("cancelled.mp4"),
        authorization_confirmed: true,
    };
    let task = tokio::spawn(async move { adapter.download(&request, cancellation, None).await });
    sleep(Duration::from_millis(100)).await;
    cancel_clone.cancel();
    assert!(matches!(
        task.await.expect("join"),
        Err(media_engine::MediaError::Cancelled)
    ));
}
