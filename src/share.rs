use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::net::{TcpListener, TcpStream, UdpSocket};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::gui::views::player_view::format_file_size;

fn copy_to_clipboard(text: &str) {
    let t = text.to_string();
    std::thread::spawn(move || {
        let Ok(mut clipboard) = arboard::Clipboard::new() else { return };
        // On Linux the copied text lives in this process: keep serving it until
        // something else is copied.
        #[cfg(target_os = "linux")]
        {
            use arboard::SetExtLinux;
            let _ = clipboard.set().wait().text(t);
        }
        #[cfg(not(target_os = "linux"))]
        let _ = clipboard.set_text(t);
    });
}

#[allow(dead_code)]
fn get_local_ip() -> Option<String> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    let addr = socket.local_addr().ok()?;
    Some(addr.ip().to_string())
}

#[derive(Clone, Debug, Default)]
pub struct ShareState {
    pub is_sharing: bool,
    pub sharing_path: Option<PathBuf>,
    pub filename: String,
    pub file_size_str: String,
    pub public_url: Option<String>,
    pub local_url: Option<String>,
    pub last_shared_url: Option<String>,
    pub status_message: Option<String>,
    pub port: Option<u16>,
}

pub static GLOBAL_SHARE_STATE: std::sync::LazyLock<Arc<Mutex<ShareState>>> =
    std::sync::LazyLock::new(|| Arc::new(Mutex::new(ShareState::default())));

static TUNNEL_PROCESS: Mutex<Option<Child>> = Mutex::new(None);
static FASTSTART_TEMP_FILE: Mutex<Option<PathBuf>> = Mutex::new(None);
static SERVER_RUNNING: AtomicBool = AtomicBool::new(false);
static SHARE_SESSION_ID: AtomicU64 = AtomicU64::new(0);

pub fn upload_and_share_clip(path: PathBuf) {
    std::thread::spawn(move || {
        start_live_share_internal(path);
    });
}

pub fn start_live_share(path: PathBuf) {
    std::thread::spawn(move || {
        start_live_share_internal(path);
    });
}

fn prepare_faststart_video(original_path: &Path, session_id: u64) -> Option<PathBuf> {
    let mut head_buf = vec![0u8; 65536];
    let has_fast_moov = std::fs::File::open(original_path)
        .and_then(|mut f| {
            let n = f.read(&mut head_buf)?;
            Ok(head_buf[..n].windows(4).any(|w| w == b"moov"))
        })
        .unwrap_or(false);

    if has_fast_moov {
        return None;
    }

    let tmp_path = std::env::temp_dir().join(format!("lapse_faststart_{}.mp4", session_id));
    let status = crate::utils::ffmpeg_cmd()
        .args([
            "-nostdin",
            "-loglevel",
            "error",
            "-y",
            "-i",
        ])
        .arg(original_path)
        .args([
            "-c",
            "copy",
            "-movflags",
            "+faststart",
        ])
        .arg(&tmp_path)
        .status();

    if status.map(|s| s.success()).unwrap_or(false) && tmp_path.exists() {
        Some(tmp_path)
    } else {
        let _ = std::fs::remove_file(&tmp_path);
        None
    }
}

fn start_live_share_internal(path: PathBuf) {
    stop_live_share();

    let session_id = SHARE_SESSION_ID.fetch_add(1, Ordering::SeqCst) + 1;
    let filename = path.file_name().unwrap_or_default().to_string_lossy().to_string();
    let file_bytes = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    let size_str = format_file_size(file_bytes);

    if let Ok(mut st) = GLOBAL_SHARE_STATE.lock() {
        st.is_sharing = true;
        st.sharing_path = Some(path.clone());
        st.filename = filename.clone();
        st.file_size_str = size_str.clone();
        st.public_url = None;
        st.local_url = None;
        st.last_shared_url = None;
        st.status_message = Some(format!("Starting high-speed stream server & tunnel for {} ({})...", filename, size_str));
        st.port = None;
    }

    let listener = match TcpListener::bind("0.0.0.0:0") {
        Ok(l) => l,
        Err(_) => {
            if let Ok(mut st) = GLOBAL_SHARE_STATE.lock() {
                st.status_message = Some("Failed to bind local stream port".into());
                st.is_sharing = false;
            }
            return;
        }
    };

    let port = match listener.local_addr() {
        Ok(addr) => addr.port(),
        Err(_) => return,
    };

    let _ = listener.set_nonblocking(true);

    if let Ok(mut st) = GLOBAL_SHARE_STATE.lock() {
        st.port = Some(port);
        st.status_message = Some(format!("Connecting high-speed Cloudflare edge tunnel for {} ({})...", filename, size_str));
    }

    crate::utils::send_notification(
        "Lapse - Live Clip Sharing",
        &format!("Connecting high-speed stream proxy for {} ({})...", filename, size_str),
        "emblem-shared",
    );

    SERVER_RUNNING.store(true, Ordering::SeqCst);

    let active_video_path = Arc::new(Mutex::new(path.clone()));
    let fallback_video_path = path.clone();

    let active_for_faststart = Arc::clone(&active_video_path);
    let orig_for_faststart = path.clone();
    std::thread::spawn(move || {
        if let Some(fast_path) = prepare_faststart_video(&orig_for_faststart, session_id) {
            if SHARE_SESSION_ID.load(Ordering::SeqCst) == session_id && SERVER_RUNNING.load(Ordering::SeqCst) {
                if let Ok(mut active) = active_for_faststart.lock() {
                    *active = fast_path.clone();
                }
                if let Ok(mut slot) = FASTSTART_TEMP_FILE.lock() {
                    if let Some(old) = slot.replace(fast_path) {
                        let _ = std::fs::remove_file(old);
                    }
                }
            } else {
                let _ = std::fs::remove_file(fast_path);
            }
        }
    });

    let orig_name = filename.clone();
    let srv_size_str = size_str.clone();
    std::thread::spawn(move || {
        run_stream_server(listener, active_video_path, fallback_video_path, orig_name, srv_size_str);
    });

    std::thread::spawn(move || {
        run_tunnel_client(port, filename);
    });
}

pub fn stop_live_share() {
    SERVER_RUNNING.store(false, Ordering::SeqCst);

    let old_temp = FASTSTART_TEMP_FILE.lock().ok().and_then(|mut g| g.take());
    let mut child = TUNNEL_PROCESS.lock().ok().and_then(|mut g| g.take());

    std::thread::spawn(move || {
        if let Some(ref mut c) = child {
            let _ = c.kill();
            let _ = c.wait();
        }
        if let Some(tmp) = old_temp {
            let _ = std::fs::remove_file(tmp);
        }
    });

    if let Ok(mut st) = GLOBAL_SHARE_STATE.lock() {
        st.is_sharing = false;
        st.sharing_path = None;
        st.filename.clear();
        st.file_size_str.clear();
        st.public_url = None;
        st.local_url = None;
        st.status_message = None;
        st.port = None;
    }
}

/// Stops sharing but leaves the status message visible so the user sees why.
fn stop_live_share_keep_message() {
    let msg = GLOBAL_SHARE_STATE.lock().ok().and_then(|st| st.status_message.clone());
    stop_live_share();
    if let Ok(mut st) = GLOBAL_SHARE_STATE.lock() {
        st.status_message = msg;
    }
}

fn run_stream_server(
    listener: TcpListener,
    active_video_path: Arc<Mutex<PathBuf>>,
    fallback_video_path: PathBuf,
    display_filename: String,
    file_size_str: String,
) {
    while SERVER_RUNNING.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((mut stream, _)) => {
                let _ = stream.set_nodelay(true);
                let v_path = active_video_path
                    .lock()
                    .map(|p| p.clone())
                    .unwrap_or_else(|_| fallback_video_path.clone());
                let fb_path = fallback_video_path.clone();
                let d_name = display_filename.clone();
                let f_size = file_size_str.clone();
                std::thread::spawn(move || {
                    let _ = handle_http_client(&mut stream, &v_path, &fb_path, &d_name, &f_size);
                });
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(2));
            }
            Err(_) => break,
        }
    }
}

fn handle_http_client(
    stream: &mut TcpStream,
    video_path: &Path,
    fallback_path: &Path,
    display_filename: &str,
    file_size_str: &str,
) {
    let cloned = match stream.try_clone() {
        Ok(c) => c,
        Err(_) => return,
    };
    let mut reader = BufReader::new(cloned);
    let mut request_line = String::new();
    if reader.read_line(&mut request_line).is_err() || request_line.is_empty() {
        return;
    }

    let mut range_header: Option<String> = None;
    let mut host_header: String = "localhost".into();
    let mut accept_header: String = String::new();
    let mut user_agent: String = String::new();
    let mut sec_fetch_dest: String = String::new();

    loop {
        let mut header_line = String::new();
        if reader.read_line(&mut header_line).is_err() || header_line.trim().is_empty() {
            break;
        }
        let lower = header_line.to_lowercase();
        if lower.starts_with("range:") {
            range_header = Some(header_line.trim().to_string());
        } else if lower.starts_with("host:") {
            if let Some((_, h)) = header_line.split_once(':') {
                host_header = h.trim().to_string();
            }
        } else if lower.starts_with("accept:") {
            if let Some((_, a)) = header_line.split_once(':') {
                accept_header = a.trim().to_lowercase();
            }
        } else if lower.starts_with("user-agent:") {
            if let Some((_, u)) = header_line.split_once(':') {
                user_agent = u.trim().to_lowercase();
            }
        } else if lower.starts_with("sec-fetch-dest:") {
            if let Some((_, d)) = header_line.split_once(':') {
                sec_fetch_dest = d.trim().to_lowercase();
            }
        }
    }

    let parts: Vec<&str> = request_line.split_whitespace().collect();
    if parts.len() < 2 {
        return;
    }
    let method = parts[0];
    let is_head = method.eq_ignore_ascii_case("HEAD");
    let raw_target = parts[1];
    let (clean_path, query) = raw_target.split_once('?').unwrap_or((raw_target, ""));

    let is_download = query.contains("download=1");
    let force_raw = query.contains("raw=1")
        || is_download
        || clean_path.starts_with("/video")
        || clean_path.starts_with("/stream")
        || sec_fetch_dest == "video"
        || sec_fetch_dest == "audio"
        || sec_fetch_dest == "empty";

    if clean_path == "/player" || clean_path == "/index.html" {
        serve_html_player(stream, display_filename, file_size_str, &host_header, is_head);
        return;
    }

    if force_raw || clean_path.ends_with(".mp4") || clean_path == "/" {
        let is_bot_or_media = user_agent.contains("discord")
            || user_agent.contains("telegram")
            || user_agent.contains("twitter")
            || user_agent.contains("slack")
            || user_agent.contains("whatsapp")
            || user_agent.contains("bot")
            || user_agent.contains("lavf")
            || user_agent.contains("ffmpeg")
            || user_agent.contains("vlc")
            || user_agent.contains("mpv")
            || user_agent.contains("curl");

        let is_browser_document = !force_raw
            && range_header.is_none()
            && !is_head
            && accept_header.contains("text/html")
            && !is_bot_or_media;

        if is_browser_document {
            serve_html_player(stream, display_filename, file_size_str, &host_header, is_head);
        } else {
            serve_video_stream(
                stream,
                video_path,
                fallback_path,
                display_filename,
                range_header,
                is_head,
                is_download,
            );
        }
        return;
    }

    let not_found = "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
    let _ = stream.write_all(not_found.as_bytes());
}

fn serve_html_player(
    stream: &mut TcpStream,
    display_filename: &str,
    file_size_str: &str,
    host: &str,
    is_head: bool,
) {
    let html = render_web_player_html(display_filename, file_size_str, host);
    let response = format!(
        "HTTP/1.1 200 OK\r\n\
        Content-Type: text/html; charset=utf-8\r\n\
        Content-Length: {}\r\n\
        Cache-Control: no-cache\r\n\
        Connection: close\r\n\r\n",
        html.len()
    );

    if is_head {
        let _ = stream.write_all(response.as_bytes());
    } else {
        let mut full = response.into_bytes();
        full.extend_from_slice(html.as_bytes());
        let _ = stream.write_all(&full);
    }
}

fn serve_video_stream(
    stream: &mut TcpStream,
    video_path: &Path,
    fallback_path: &Path,
    display_filename: &str,
    range_header: Option<String>,
    is_head: bool,
    is_download: bool,
) {
    let mut file = match std::fs::File::open(video_path).or_else(|_| std::fs::File::open(fallback_path)) {
        Ok(f) => f,
        Err(_) => {
            let res = "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n";
            let _ = stream.write_all(res.as_bytes());
            return;
        }
    };

    let file_size = match file.metadata() {
        Ok(m) => m.len(),
        Err(_) => return,
    };

    let disposition = if is_download { "attachment" } else { "inline" };

    if let Some(range_str) = range_header {
        if let Some(eq_idx) = range_str.find('=') {
            let spec = &range_str[eq_idx + 1..].trim();
            let dash_parts: Vec<&str> = spec.split('-').collect();
            let start: u64 = dash_parts.first().and_then(|s| s.parse().ok()).unwrap_or(0);
            let end: u64 = dash_parts
                .get(1)
                .and_then(|s| s.parse().ok())
                .unwrap_or(file_size.saturating_sub(1));

            let end = end.min(file_size.saturating_sub(1));
            let chunk_size = if end >= start { end - start + 1 } else { 0 };

            if file.seek(SeekFrom::Start(start)).is_ok() {
                let response_headers = format!(
                    "HTTP/1.1 206 Partial Content\r\n\
                    Content-Type: video/mp4\r\n\
                    Content-Disposition: {}; filename=\"{}\"\r\n\
                    Content-Range: bytes {}-{}/{}\r\n\
                    Content-Length: {}\r\n\
                    Accept-Ranges: bytes\r\n\
                    Cache-Control: public, max-age=86400\r\n\
                    X-Content-Type-Options: nosniff\r\n\
                    Access-Control-Allow-Origin: *\r\n\
                    Connection: close\r\n\r\n",
                    disposition, display_filename, start, end, file_size, chunk_size
                );

                if stream.write_all(response_headers.as_bytes()).is_ok() && !is_head {
                    let mut buffer = vec![0u8; 1024 * 1024];
                    let mut remaining = chunk_size;
                    while remaining > 0 {
                        let to_read = remaining.min(buffer.len() as u64) as usize;
                        match file.read(&mut buffer[..to_read]) {
                            Ok(0) => break,
                            Ok(n) => {
                                if stream.write_all(&buffer[..n]).is_err() {
                                    break;
                                }
                                remaining -= n as u64;
                            }
                            Err(_) => break,
                        }
                    }
                    let _ = stream.flush();
                }
                return;
            }
        }
    }

    let response_headers = format!(
        "HTTP/1.1 200 OK\r\n\
        Content-Type: video/mp4\r\n\
        Content-Disposition: {}; filename=\"{}\"\r\n\
        Content-Length: {}\r\n\
        Accept-Ranges: bytes\r\n\
        Cache-Control: public, max-age=86400\r\n\
        X-Content-Type-Options: nosniff\r\n\
        Access-Control-Allow-Origin: *\r\n\
        Connection: close\r\n\r\n",
        disposition, display_filename, file_size
    );

    if stream.write_all(response_headers.as_bytes()).is_ok() && !is_head {
        let mut buffer = vec![0u8; 1024 * 1024];
        loop {
            match file.read(&mut buffer) {
                Ok(0) => break,
                Ok(n) => {
                    if stream.write_all(&buffer[..n]).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
        let _ = stream.flush();
    }
}

fn render_web_player_html(filename: &str, file_size_str: &str, host: &str) -> String {
    let clean_fname = filename.strip_suffix(".mp4").unwrap_or(filename);
    let proto = if host.starts_with("http://") || host.starts_with("https://") { "" } else { "https://" };
    let base_url = format!("{}{}", proto, host);
    let video_url = format!("{}/video.mp4", base_url);

    format!(
        r###"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>Lapse Clip &bull; {clean_name} ({size_str})</title>

  <meta property="og:type" content="video.other">
  <meta property="og:site_name" content="Lapse Replay">
  <meta property="og:title" content="{clean_name} ({size_str})">
  <meta property="og:description" content="Watch high-definition gameplay clip streamed via Lapse">
  <meta property="og:url" content="{video_url}">
  <meta property="og:video" content="{video_url}">
  <meta property="og:video:url" content="{video_url}">
  <meta property="og:video:secure_url" content="{video_url}">
  <meta property="og:video:type" content="video/mp4">
  <meta property="og:video:width" content="1920">
  <meta property="og:video:height" content="1080">
  <meta name="theme-color" content="#d0bcff">

  <meta name="twitter:card" content="player">
  <meta name="twitter:title" content="{clean_name} ({size_str})">
  <meta name="twitter:description" content="Watch high-definition gameplay clip streamed via Lapse">
  <meta name="twitter:player" content="{video_url}">
  <meta name="twitter:player:width" content="1920">
  <meta name="twitter:player:height" content="1080">
  <meta name="twitter:player:stream" content="{video_url}">
  <meta name="twitter:player:stream:content_type" content="video/mp4">

  <style>
    * {{ box-sizing: border-box; margin: 0; padding: 0; }}
    body {{
      background: #0f1015;
      color: #e4e2e6;
      font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Helvetica, Arial, sans-serif;
      display: flex;
      flex-direction: column;
      align-items: center;
      justify-content: center;
      min-height: 100vh;
      padding: 20px;
    }}
    .container {{
      max-width: 1280px;
      width: 92vw;
      background: #1c1b1f;
      border: 1px solid #49454f;
      border-radius: 20px;
      overflow: hidden;
      box-shadow: 0 12px 36px rgba(0,0,0,0.6);
    }}
    .header {{
      display: flex;
      align-items: center;
      justify-content: space-between;
      padding: 16px 24px;
      background: #2b2930;
      border-bottom: 1px solid #49454f;
    }}
    .title {{ font-size: 16px; font-weight: 700; color: #d0bcff; }}
    .meta-badges {{ display: flex; align-items: center; gap: 10px; }}
    .size-badge {{
      padding: 4px 10px;
      background: #4a4458;
      color: #e8def8;
      border-radius: 10px;
      font-size: 12px;
      font-weight: 700;
    }}
    .badge {{
      display: inline-flex;
      align-items: center;
      gap: 6px;
      padding: 4px 10px;
      background: rgba(100, 220, 140, 0.15);
      color: #64dc8c;
      border-radius: 12px;
      font-size: 11px;
      font-weight: 700;
      letter-spacing: 0.5px;
    }}
    .badge-dot {{ width: 6px; height: 6px; background: #64dc8c; border-radius: 50%; }}
    .video-wrapper {{
      position: relative;
      background: #000;
      display: flex;
      justify-content: center;
      align-items: center;
    }}
    video {{
      width: 100%;
      max-height: 78vh;
      display: block;
      outline: none;
    }}
    .bottom-bar {{
      display: flex;
      align-items: center;
      justify-content: space-between;
      padding: 14px 24px;
      background: #2b2930;
    }}
    .info {{ font-size: 13px; color: #cac4d0; }}
    .btn {{
      display: inline-block;
      background: #d0bcff;
      color: #381e72;
      text-decoration: none;
      padding: 8px 18px;
      border-radius: 16px;
      font-weight: 600;
      font-size: 13px;
      transition: opacity 0.2s;
    }}
    .btn:hover {{ opacity: 0.85; }}
  </style>
</head>
<body>
<div class="container">
  <div class="header">
    <div class="title">Lapse P2P Stream &bull; {clean_name}</div>
    <div class="meta-badges">
      <div class="size-badge">{size_str}</div>
      <div class="badge"><span class="badge-dot"></span> LIVE PROXY</div>
    </div>
  </div>
  <div class="video-wrapper">
    <video controls autoplay playsinline preload="auto" src="/video.mp4">
      <source src="/video.mp4" type="video/mp4">
      Your browser does not support HTML5 video streaming.
    </video>
  </div>
  <div class="bottom-bar">
    <div class="info">Proxy streaming live while Lapse application is running on host ({size_str}).</div>
    <div class="actions">
      <a class="btn" href="/video.mp4?download=1" download="{fname}">Download Clip ({size_str})</a>
    </div>
  </div>
</div>
</body>
</html>"###,
        clean_name = clean_fname,
        size_str = file_size_str,
        video_url = video_url,
        fname = filename
    )
}

fn contains_ipv4_pattern(url: &str) -> bool {
    for sep in [b'-', b'.'] {
        let parts: Vec<&str> = url
            .split(|c: char| c == (sep as char) || c == '/' || c == ':')
            .collect();
        let mut consec_octets = 0;
        for part in parts {
            if !part.is_empty() && part.len() <= 3 && part.bytes().all(|b| b.is_ascii_digit()) {
                if part.parse::<u8>().is_ok() {
                    consec_octets += 1;
                    if consec_octets >= 4 {
                        return true;
                    }
                } else {
                    consec_octets = 0;
                }
            } else {
                consec_octets = 0;
            }
        }
    }
    false
}

/// Lapse's own copy of cloudflared, downloaded the first time a link is shared.
fn own_cloudflared() -> Option<PathBuf> {
    let name = if cfg!(windows) { "cloudflared.exe" } else { "cloudflared" };
    Some(directories::ProjectDirs::from("com", "lapse", "lapse")?.data_dir().join("bin").join(name))
}

/// cloudflared from the system (package manager, PATH) or Lapse's own copy.
fn find_cloudflared() -> Option<PathBuf> {
    let name = if cfg!(windows) { "cloudflared.exe" } else { "cloudflared" };
    let path_dirs = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect::<Vec<_>>()).unwrap_or_default();
    #[cfg(unix)]
    let extra_dirs: Vec<PathBuf> = {
        let home_bin = std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/bin"));
        home_bin.into_iter().chain(["/usr/bin".into(), "/usr/local/bin".into()]).collect()
    };
    #[cfg(not(unix))]
    let extra_dirs: Vec<PathBuf> = Vec::new();

    path_dirs
        .into_iter()
        .chain(extra_dirs)
        .map(|d| d.join(name))
        .chain(own_cloudflared())
        .find(|p| p.is_file())
}

fn set_status(message: &str) {
    if let Ok(mut st) = GLOBAL_SHARE_STATE.lock() {
        st.status_message = Some(message.into());
    }
}

/// Downloads the official cloudflared release from GitHub into Lapse's data folder and
/// checks it against the SHA-256 GitHub publishes for the file.
fn download_cloudflared() -> Result<PathBuf, String> {
    let (os_name, arch, ext) = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => ("windows", "amd64", ".exe"),
        ("windows", "x86") => ("windows", "386", ".exe"),
        ("windows", other) => return Err(format!("no cloudflared build for windows-{}", other)),
        ("linux", "x86_64") => ("linux", "amd64", ""),
        ("linux", "aarch64") => ("linux", "arm64", ""),
        ("linux", "arm") => ("linux", "arm", ""),
        ("linux", "x86") => ("linux", "386", ""),
        (os, arch) => return Err(format!("no cloudflared build for {}-{}", os, arch)),
    };
    let target = own_cloudflared().ok_or("no data folder")?;
    let asset_name = format!("cloudflared-{}-{}{}", os_name, arch, ext);
    set_status("Getting cloudflared for share links (first time only, about 40 MB)...");

    let json = crate::http::get_json("https://api.github.com/repos/cloudflare/cloudflared/releases/latest", &[], 20)
        .map_err(|_| "GitHub didn't answer".to_string())?;
    let asset = json["assets"]
        .as_array()
        .and_then(|a| a.iter().find(|x| x["name"] == asset_name.as_str()))
        .ok_or("the release has no compatible build")?;
    let url = asset["browser_download_url"].as_str().ok_or("no download link")?;
    let digest = asset["digest"].as_str().and_then(|d| d.strip_prefix("sha256:")).ok_or("no checksum published")?.to_lowercase();

    std::fs::create_dir_all(target.parent().ok_or("bad path")?).map_err(|e| e.to_string())?;
    let part = target.with_extension("part");
    let got = match crate::http::download(url, &part, 600) {
        Ok(sum) => sum,
        Err(e) => {
            let _ = std::fs::remove_file(&part);
            return Err(format!("download failed: {}", e));
        }
    };
    if got != digest {
        let _ = std::fs::remove_file(&part);
        return Err("the download didn't match its checksum".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&part, std::fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
    }
    std::fs::rename(&part, &target).map_err(|e| e.to_string())?;
    Ok(target)
}

/// Finds cloudflared, downloading it the first time.
fn find_or_install_cloudflared() -> Result<PathBuf, String> {
    match find_cloudflared() {
        Some(p) => Ok(p),
        None => download_cloudflared(),
    }
}

fn try_cloudflared_tunnel(cf_bin: &Path, port: u16) -> Option<String> {
    set_status("Opening the Cloudflare tunnel...");
    let local_url = format!("http://127.0.0.1:{}", port);

    let mut cmd = Command::new(cf_bin);
    cmd.args([
            "tunnel",
            "--no-autoupdate",
            "--protocol",
            "http2",
            "--url",
            &local_url,
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    crate::utils::no_window(&mut cmd);
    let mut child = cmd.spawn().ok()?;

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    if let Ok(mut lock) = TUNNEL_PROCESS.lock() {
        if let Some(mut old) = lock.take() {
            let _ = old.kill();
            let _ = old.wait();
        }
        *lock = Some(child);
    }

    let (tx, rx) = std::sync::mpsc::channel::<String>();

    if let Some(out) = stdout {
        let tx_out = tx.clone();
        std::thread::spawn(move || {
            let reader = BufReader::new(out);
            for line in reader.lines().map_while(Result::ok) {
                let _ = tx_out.send(line);
            }
        });
    }

    if let Some(err) = stderr {
        let tx_err = tx;
        std::thread::spawn(move || {
            let reader = BufReader::new(err);
            for line in reader.lines().map_while(Result::ok) {
                let _ = tx_err.send(line);
            }
        });
    }

    let start_time = std::time::Instant::now();
    let mut pending_url: Option<String> = None;
    let mut url_found_at: Option<std::time::Instant> = None;

    while start_time.elapsed() < Duration::from_secs(10) && SERVER_RUNNING.load(Ordering::SeqCst) {
        if let Ok(line) = rx.recv_timeout(Duration::from_millis(100)) {
            if let Some(idx) = line.find("https://") {
                let candidate = &line[idx..];
                let end_idx = candidate
                    .find(|c: char| c.is_whitespace() || c == '|' || c == ',' || c == ')' || c == '"')
                    .unwrap_or(candidate.len());
                let clean_url = candidate[..end_idx].trim().trim_end_matches('/').to_string();
                if clean_url.ends_with(".trycloudflare.com") && !clean_url.contains("developers.cloudflare.com") {
                    pending_url = Some(format!("{}/clip.mp4", clean_url));
                    url_found_at = Some(std::time::Instant::now());
                }
            }
            if pending_url.is_some() && line.contains("Registered tunnel connection") {
                std::thread::sleep(Duration::from_millis(350));
                return pending_url;
            }
        }
        if let (Some(ref url), Some(t)) = (&pending_url, url_found_at) {
            if t.elapsed() >= Duration::from_millis(1600) {
                return Some(url.clone());
            }
        }
    }

    pending_url
}

fn run_tunnel_client(port: u16, _filename: String) {
    let cfg = crate::config::Config::load().unwrap_or_default();
    let cloudflared = if SERVER_RUNNING.load(Ordering::SeqCst) { Some(find_or_install_cloudflared()) } else { None };
    let mut discovered_url = match &cloudflared {
        Some(Ok(bin)) => try_cloudflared_tunnel(bin, port),
        _ => None,
    };

    if discovered_url.is_none() && !cfg.share_allow_ssh_fallback {
        if let Ok(mut st) = GLOBAL_SHARE_STATE.lock() {
            st.status_message = Some(match &cloudflared {
                Some(Err(e)) => format!("Couldn't get cloudflared ({}). Check your internet connection and try again.", e),
                _ => "Cloudflare tunnel timed out. Please retry.".into(),
            });
        }
        stop_live_share_keep_message();
        return;
    }

    if discovered_url.is_none() && SERVER_RUNNING.load(Ordering::SeqCst) {
        let port_str = format!("80:127.0.0.1:{}", port);
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| (d.as_millis() & 0xFFFFFF) as u32)
            .unwrap_or(42137);
        let custom_serveo_str = format!("lapse-{:06x}:80:127.0.0.1:{}", stamp, port);

        let known_hosts_arg = if cfg!(windows) {
            "UserKnownHostsFile=NUL"
        } else {
            "UserKnownHostsFile=/dev/null"
        };

        let candidates: Vec<(&str, Vec<&str>)> = vec![
            ("nokey@localhost.run", vec![
                "-n",
                "-o", "ConnectTimeout=5",
                "-o", "StrictHostKeyChecking=no",
                "-o", known_hosts_arg,
                "-o", "ExitOnForwardFailure=yes",
                "-o", "Compression=no",
                "-o", "IPQoS=throughput",
                "-o", "ServerAliveInterval=15",
                "-R", &port_str,
                "nokey@localhost.run",
            ]),
            ("serveo.net-custom", vec![
                "-n",
                "-o", "ConnectTimeout=5",
                "-o", "StrictHostKeyChecking=no",
                "-o", known_hosts_arg,
                "-o", "ExitOnForwardFailure=yes",
                "-o", "Compression=no",
                "-o", "IPQoS=throughput",
                "-o", "ServerAliveInterval=15",
                "-R", &custom_serveo_str,
                "serveo.net",
            ]),
        ];

        for (target_name, args) in candidates {
            if !SERVER_RUNNING.load(Ordering::SeqCst) || discovered_url.is_some() {
                break;
            }

            let mut cmd = Command::new("ssh");
            cmd.args(&args)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            crate::utils::no_window(&mut cmd);
            let mut child = match cmd.spawn() {
                Ok(c) => c,
                Err(_) => continue,
            };

            let stdout = child.stdout.take();
            let stderr = child.stderr.take();

            if let Ok(mut lock) = TUNNEL_PROCESS.lock() {
                if let Some(mut old) = lock.take() {
                    let _ = old.kill();
                    let _ = old.wait();
                }
                *lock = Some(child);
            }

            let (tx, rx) = std::sync::mpsc::channel::<String>();

            if let Some(out) = stdout {
                let tx_out = tx.clone();
                std::thread::spawn(move || {
                    let reader = BufReader::new(out);
                    for line in reader.lines().map_while(Result::ok) {
                        let _ = tx_out.send(line);
                    }
                });
            }

            if let Some(err) = stderr {
                let tx_err = tx;
                std::thread::spawn(move || {
                    let reader = BufReader::new(err);
                    for line in reader.lines().map_while(Result::ok) {
                        let _ = tx_err.send(line);
                    }
                });
            }

            let start_time = std::time::Instant::now();
            while start_time.elapsed() < Duration::from_secs(7) && SERVER_RUNNING.load(Ordering::SeqCst) {
                if let Ok(line) = rx.recv_timeout(Duration::from_millis(150)) {
                    if let Some(idx) = line.find("https://") {
                        let candidate = &line[idx..];
                        let end_idx = candidate
                            .find(|c: char| c.is_whitespace() || c == ',' || c == ')' || c == '"')
                            .unwrap_or(candidate.len());
                        let clean_url = candidate[..end_idx].trim().trim_end_matches('/').to_string();
                        if (clean_url.contains(".lhr.life")
                            || clean_url.contains(".serveousercontent.com")
                            || clean_url.contains(".serveo.net")
                            || clean_url.contains(".pinggy.link"))
                            && !clean_url.contains("admin")
                            && !clean_url.contains("docs")
                            && !clean_url.contains("localhost.run")
                            && !contains_ipv4_pattern(&clean_url)
                        {
                            discovered_url = Some(format!("{}/clip.mp4", clean_url));
                            break;
                        }
                    }
                }
            }

            if discovered_url.is_some() {
                break;
            } else {
                let _ = target_name;
            }
        }
    }

    if let Some(url) = discovered_url {
        copy_to_clipboard(&url);

        // With a webhook set, the link also goes to the Discord channel.
        let discord = (!cfg.discord_webhook.trim().is_empty()).then(|| crate::discord::post_message(&cfg.discord_webhook, &url));
        let status = match &discord {
            Some(Ok(())) => format!("Live link ready, copied and sent to Discord: {}", url),
            Some(Err(e)) => format!("Live link ready & copied: {} (Discord: {})", url, e),
            None => format!("Live link ready & copied: {}", url),
        };
        if let Ok(mut st) = GLOBAL_SHARE_STATE.lock() {
            st.public_url = Some(url.clone());
            st.last_shared_url = Some(url.clone());
            st.status_message = Some(status);
        }

        crate::utils::send_notification(
            "Lapse - Live Clip Shared",
            &format!("Live stream ready (copied to clipboard):\n{}\nPlaying while Lapse is open.", url),
            "emblem-shared",
        );
    } else {
        if let Ok(mut st) = GLOBAL_SHARE_STATE.lock() {
            st.status_message = Some("Public tunnel timed out (Please retry)".into());
        }
    }
}

#[cfg(test)]
mod cloudflared_tests {
    /// Downloads for real: `XDG_DATA_HOME=/tmp/x cargo test cloudflared_download -- --ignored`.
    #[test]
    #[ignore]
    fn cloudflared_download() {
        let path = super::download_cloudflared().unwrap();
        let mut cmd = std::process::Command::new(&path);
        cmd.arg("--version");
        crate::utils::no_window(&mut cmd);
        let out = cmd.output().unwrap();
        println!("{}", String::from_utf8_lossy(&out.stdout));
        assert!(out.status.success());
    }
}

#[cfg(test)]
mod clipboard_tests {
    /// Needs a display (X11 or Wayland): `cargo test copies_link -- --ignored`.
    #[test]
    #[ignore]
    fn copies_link() {
        super::copy_to_clipboard("https://example.trycloudflare.com/clip");
        std::thread::sleep(std::time::Duration::from_millis(500));
        let text = arboard::Clipboard::new().unwrap().get_text().unwrap();
        assert_eq!(text, "https://example.trycloudflare.com/clip");
    }
}
