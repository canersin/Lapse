//! Uploads clips to Streamable via API with HTTP Basic authentication.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use serde::{Deserialize, Serialize};

/// Free Streamable accounts upload limits: 250 MB and 10 minutes.
pub const MAX_BYTES: u64 = 250 * 1024 * 1024;
pub const MAX_DURATION_SECS: f64 = 600.0;
pub const UPLOAD_URL: &str = "https://api.streamable.com/upload";

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct StreamableCredentials {
    #[serde(default)]
    pub email: String,
    #[serde(default)]
    pub password: String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum UploadState {
    Uploading,
    Done(String),
    Failed(String),
}

static UPLOADS: Mutex<Vec<(PathBuf, UploadState)>> = Mutex::new(Vec::new());

pub fn set(path: &Path, state: UploadState) {
    let mut uploads = UPLOADS.lock().unwrap_or_else(|e| e.into_inner());
    uploads.retain(|(p, _)| p != path);
    uploads.push((path.to_path_buf(), state));
}

pub fn state_for(path: &Path) -> Option<UploadState> {
    UPLOADS.lock().unwrap_or_else(|e| e.into_inner()).iter().find(|(p, _)| p == path).map(|(_, s)| s.clone())
}

/// Path to streamable.json next to config.toml.
pub fn credentials_path() -> Option<PathBuf> {
    directories::ProjectDirs::from("com", "lapse", "lapse")
        .map(|dirs| dirs.config_dir().join("streamable.json"))
}

pub fn load_credentials_from(path: &Path) -> Option<StreamableCredentials> {
    let content = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

pub fn load_credentials() -> Option<StreamableCredentials> {
    let path = credentials_path()?;
    load_credentials_from(&path)
}

pub fn save_credentials_to(path: &Path, email: &str, password: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let creds = StreamableCredentials {
        email: email.trim().to_string(),
        password: password.to_string(),
    };
    let json = serde_json::to_string_pretty(&creds).unwrap_or_default();
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        use std::os::unix::fs::PermissionsExt;
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true).mode(0o600);
        let mut file = options.open(path)?;
        file.write_all(json.as_bytes())?;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
        Ok(())
    }
    #[cfg(not(unix))]
    {
        std::fs::write(path, json.as_bytes())
    }
}

pub fn save_credentials(email: &str, password: &str) -> std::io::Result<()> {
    let Some(path) = credentials_path() else {
        return Err(std::io::Error::new(std::io::ErrorKind::NotFound, "config directory not found"));
    };
    save_credentials_to(&path, email, password)
}

pub fn remove_credentials() -> std::io::Result<()> {
    if let Some(path) = credentials_path() {
        if path.exists() {
            std::fs::remove_file(path)?;
        }
    }
    Ok(())
}

pub fn is_configured() -> bool {
    load_credentials()
        .map(|c| !c.email.trim().is_empty() && !c.password.is_empty())
        .unwrap_or(false)
}

pub fn check_size_limit(bytes: u64) -> Result<(), &'static str> {
    if bytes > MAX_BYTES {
        Err("File is over Streamable's 250 MB limit")
    } else {
        Ok(())
    }
}

pub fn check_limits(bytes: u64, duration_secs: Option<f64>) -> Result<(), &'static str> {
    check_size_limit(bytes)?;
    if let Some(d) = duration_secs {
        if d > MAX_DURATION_SECS {
            return Err("Clip is longer than Streamable's 10-minute limit");
        }
    }
    Ok(())
}

pub fn map_http_error(err: &crate::http::Error) -> String {
    match err {
        crate::http::Error::Status(401) => "Wrong Streamable email or password".to_string(),
        crate::http::Error::Status(429) => "Streamable is rate limiting, try again later".to_string(),
        crate::http::Error::Status(code) => format!("Streamable answered {}", code),
        crate::http::Error::Network(msg) => {
            let first = msg.lines().next().unwrap_or("Network error");
            first.to_string()
        }
    }
}

pub fn parse_upload_json(val: &serde_json::Value) -> Result<String, String> {
    let shortcode = val.get("shortcode")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing shortcode in Streamable response".to_string())?;
    Ok(format!("https://streamable.com/{}", shortcode))
}

pub fn parse_upload_response(body: &str) -> Result<String, String> {
    let val: serde_json::Value = serde_json::from_str(body).map_err(|e| e.to_string())?;
    parse_upload_json(&val)
}

pub fn copy_to_clipboard(text: &str) {
    // Tests exercise the upload path; they must not overwrite the real clipboard.
    if cfg!(test) {
        return;
    }
    let t = text.to_string();
    std::thread::spawn(move || {
        let Ok(mut clipboard) = arboard::Clipboard::new() else { return };
        #[cfg(target_os = "linux")]
        {
            use arboard::SetExtLinux;
            let _ = clipboard.set().wait().text(t);
        }
        #[cfg(not(target_os = "linux"))]
        let _ = clipboard.set_text(t);
    });
}

pub fn upload_with_creds(path: PathBuf, upload_url: &str, creds: StreamableCredentials) {
    let bytes = match std::fs::metadata(&path) {
        Ok(m) => m.len(),
        Err(e) => {
            set(&path, UploadState::Failed(e.to_string()));
            return;
        }
    };
    if let Err(msg) = check_size_limit(bytes) {
        set(&path, UploadState::Failed(msg.to_string()));
        return;
    }
    if creds.email.trim().is_empty() || creds.password.is_empty() {
        set(&path, UploadState::Failed("Streamable credentials not configured".into()));
        return;
    }
    set(&path, UploadState::Uploading);
    let key = path.clone();
    let url = upload_url.to_string();
    std::thread::spawn(move || {
        let res = crate::http::post_file_auth(
            &url,
            Some((&creds.email, &creds.password)),
            "file",
            &path,
            600,
        );
        match res {
            Ok(json) => match parse_upload_json(&json) {
                Ok(share_url) => {
                    copy_to_clipboard(&share_url);
                    crate::utils::send_notification("Lapse", "Streamable link copied", "emblem-shared");
                    set(&key, UploadState::Done(share_url));
                }
                Err(e) => {
                    set(&key, UploadState::Failed(e));
                }
            },
            Err(e) => {
                set(&key, UploadState::Failed(map_http_error(&e)));
            }
        }
    });
}

pub fn upload_to(path: PathBuf, upload_url: &str) {
    let Some(creds) = load_credentials() else {
        set(&path, UploadState::Failed("Streamable credentials not configured".into()));
        return;
    };
    upload_with_creds(path, upload_url, creds);
}

pub fn upload(path: PathBuf) {
    upload_to(path, UPLOAD_URL);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_response_parsing_shortcode_to_url() {
        let valid_json = r#"{"shortcode":"ab12cd","status":1}"#;
        assert_eq!(
            parse_upload_response(valid_json),
            Ok("https://streamable.com/ab12cd".to_string())
        );

        let missing_code = r#"{"status":1}"#;
        assert!(parse_upload_response(missing_code).is_err());

        let invalid_json = "not json";
        assert!(parse_upload_response(invalid_json).is_err());
    }

    #[test]
    fn test_size_limit_check() {
        assert!(check_size_limit(1024).is_ok());
        assert!(check_size_limit(MAX_BYTES).is_ok());
        assert!(check_size_limit(MAX_BYTES + 1).is_err());
        assert_eq!(
            check_size_limit(MAX_BYTES + 1),
            Err("File is over Streamable's 250 MB limit")
        );

        assert!(check_limits(1024, Some(500.0)).is_ok());
        assert!(check_limits(1024, Some(601.0)).is_err());
        assert!(check_limits(MAX_BYTES + 1, Some(100.0)).is_err());
    }

    #[test]
    fn test_error_mapping() {
        assert_eq!(
            map_http_error(&crate::http::Error::Status(401)),
            "Wrong Streamable email or password"
        );
        assert_eq!(
            map_http_error(&crate::http::Error::Status(429)),
            "Streamable is rate limiting, try again later"
        );
        assert_eq!(
            map_http_error(&crate::http::Error::Status(500)),
            "Streamable answered 500"
        );
        assert_eq!(
            map_http_error(&crate::http::Error::Network("connection timed out".to_string())),
            "connection timed out"
        );
    }

    #[test]
    fn test_credentials_roundtrip_and_permissions() {
        let temp = std::env::temp_dir().join(format!("lapse-streamable-test-{}", std::process::id()));
        let _ = std::fs::remove_file(&temp);

        assert_eq!(load_credentials_from(&temp), None);

        save_credentials_to(&temp, "test@example.com", "secret123").unwrap();
        let loaded = load_credentials_from(&temp).unwrap();
        assert_eq!(loaded.email, "test@example.com");
        assert_eq!(loaded.password, "secret123");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let meta = std::fs::metadata(&temp).unwrap();
            let mode = meta.permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }

        let _ = std::fs::remove_file(&temp);
    }

    #[test]
    fn test_local_mock_upload() {
        use std::io::{BufRead, BufReader, Read, Write};
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let url = format!("http://{}/upload", addr);

        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut headers = Vec::new();
            let mut length = 0usize;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    length = v.trim().parse().unwrap();
                }
                headers.push(line.clone());
                if line == "\r\n" {
                    break;
                }
            }
            let mut body = vec![0u8; length];
            reader.read_exact(&mut body).unwrap();
            let mut stream = stream;
            let response = "{\"shortcode\": \"mock99\"}";
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                response.len(),
                response
            ).unwrap();
            (headers, body)
        });

        let test_file = std::env::temp_dir().join(format!("lapse-mock-{}.mp4", std::process::id()));
        std::fs::write(&test_file, b"sample clip content").unwrap();

        let creds = StreamableCredentials {
            email: "user@lapse.com".into(),
            password: "mypassword".into(),
        };

        upload_with_creds(test_file.clone(), &url, creds);

        // Wait for background upload to complete
        for _ in 0..50 {
            std::thread::sleep(std::time::Duration::from_millis(50));
            if let Some(UploadState::Done(url)) = state_for(&test_file) {
                assert_eq!(url, "https://streamable.com/mock99");
                break;
            }
        }

        let (headers, _body) = server.join().unwrap();
        let auth_header = headers.iter().find(|h| h.to_ascii_lowercase().starts_with("authorization:"));
        assert!(auth_header.is_some());
        let _ = std::fs::remove_file(&test_file);
    }
}
