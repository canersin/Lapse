//! Web requests (update check, Discord, Steam art, share links) made by Lapse itself,
//! so it doesn't need curl on the system.

use std::fmt;
use std::io::{Cursor, Read, Write};
use std::path::Path;
use std::time::Duration;

const USER_AGENT: &str = concat!("Lapse/", env!("CARGO_PKG_VERSION"));

#[derive(Debug)]
pub enum Error {
    /// The server answered with this HTTP status.
    Status(u16),
    /// No answer: offline, DNS, TLS, timeout, or a local file problem.
    Network(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Status(code) => write!(f, "the server answered {}", code),
            Error::Network(e) => f.write_str(e),
        }
    }
}

impl From<ureq::Error> for Error {
    fn from(e: ureq::Error) -> Self {
        match e {
            ureq::Error::Status(code, _) => Error::Status(code),
            ureq::Error::Transport(t) => Error::Network(t.to_string()),
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Network(e.to_string())
    }
}

fn agent(timeout_secs: u64) -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(timeout_secs.min(10)))
        .timeout(Duration::from_secs(timeout_secs))
        .user_agent(USER_AGENT)
        .build()
}

/// GET returning parsed JSON; the `query` pairs are URL-encoded.
pub fn get_json(url: &str, query: &[(&str, &str)], timeout_secs: u64) -> Result<serde_json::Value, Error> {
    let mut req = agent(timeout_secs).get(url);
    for (k, v) in query {
        req = req.query(k, v);
    }
    let text = req.call()?.into_string()?;
    serde_json::from_str(&text).map_err(|e| Error::Network(e.to_string()))
}

/// POST with a JSON body.
pub fn post_json(url: &str, body: &serde_json::Value, timeout_secs: u64) -> Result<(), Error> {
    agent(timeout_secs)
        .post(url)
        .set("Content-Type", "application/json")
        .send_string(&body.to_string())?;
    Ok(())
}

/// POSTs an empty body with HTTP Basic auth, returning the HTTP status code (Ok(code) for any
/// HTTP response including 4xx/5xx, Err only for network errors).
pub fn post_empty_auth(url: &str, auth: (&str, &str), timeout_secs: u64) -> Result<u16, Error> {
    let creds = format!("{}:{}", auth.0, auth.1);
    let req = agent(timeout_secs)
        .post(url)
        .set("Authorization", &format!("Basic {}", base64_encode(creds.as_bytes())));
    match req.send_bytes(&[]) {
        Ok(resp) => Ok(resp.status()),
        Err(ureq::Error::Status(code, _)) => Ok(code),
        Err(ureq::Error::Transport(t)) => Err(Error::Network(t.to_string())),
    }
}

fn base64_encode(input: &[u8]) -> String {
    const CHARSET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((input.len() + 2) / 3 * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0] as usize;
        let b1 = chunk.get(1).copied().unwrap_or(0) as usize;
        let b2 = chunk.get(2).copied().unwrap_or(0) as usize;
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(CHARSET[(triple >> 18) & 0x3F] as char);
        out.push(CHARSET[(triple >> 12) & 0x3F] as char);
        if chunk.len() > 1 {
            out.push(CHARSET[(triple >> 6) & 0x3F] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(CHARSET[triple & 0x3F] as char);
        } else {
            out.push('=');
        }
    }
    out
}

/// Uploads a file as the multipart form field `field`, streaming it from disk.
pub fn post_file(url: &str, field: &str, path: &Path, timeout_secs: u64) -> Result<(), Error> {
    post_file_auth(url, None, field, path, timeout_secs).map(|_| ())
}

/// Uploads a file as multipart form-data with optional HTTP Basic auth, returning parsed JSON.
pub fn post_file_auth(
    url: &str,
    auth: Option<(&str, &str)>,
    field: &str,
    path: &Path,
    timeout_secs: u64,
) -> Result<serde_json::Value, Error> {
    let file = std::fs::File::open(path)?;
    let size = file.metadata()?.len();
    let name = path.file_name().map(|n| n.to_string_lossy().replace('"', "")).unwrap_or_else(|| "file".into());
    let boundary = format!(
        "lapse{:x}",
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos()
    );
    let head = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"{field}\"; filename=\"{name}\"\r\nContent-Type: application/octet-stream\r\n\r\n"
    );
    let tail = format!("\r\n--{boundary}--\r\n");
    let length = head.len() as u64 + size + tail.len() as u64;
    let body = Cursor::new(head.into_bytes()).chain(file).chain(Cursor::new(tail.into_bytes()));
    let mut req = agent(timeout_secs)
        .post(url)
        .set("Content-Type", &format!("multipart/form-data; boundary={boundary}"))
        .set("Content-Length", &length.to_string());
    if let Some((user, pass)) = auth {
        let creds = format!("{}:{}", user, pass);
        req = req.set("Authorization", &format!("Basic {}", base64_encode(creds.as_bytes())));
    }
    let resp = req.send(body)?;
    let text = resp.into_string()?;
    if text.trim().is_empty() {
        return Ok(serde_json::Value::Null);
    }
    serde_json::from_str(&text).map_err(|e| Error::Network(e.to_string()))
}

/// Downloads `url` into `dest` and returns the file's SHA-256 as lowercase hex.
pub fn download(url: &str, dest: &Path, timeout_secs: u64) -> Result<String, Error> {
    let mut reader = agent(timeout_secs).get(url).call()?.into_reader();
    let mut file = std::fs::File::create(dest)?;
    let mut hash = ring::digest::Context::new(&ring::digest::SHA256);
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hash.update(&buf[..n]);
        file.write_all(&buf[..n])?;
    }
    file.flush()?;
    Ok(hash.finish().as_ref().iter().map(|b| format!("{:02x}", b)).collect())
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;

    /// Answers one request with `status` and returns what the client sent.
    fn serve_once(status: u16) -> (String, std::thread::JoinHandle<Vec<u8>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/upload", listener.local_addr().unwrap());
        let handle = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut head = Vec::new();
            let mut length = 0usize;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    length = v.trim().parse().unwrap();
                }
                head.extend_from_slice(line.as_bytes());
                if line == "\r\n" {
                    break;
                }
            }
            let mut body = vec![0u8; length];
            reader.read_exact(&mut body).unwrap();
            let mut stream = stream;
            write!(stream, "HTTP/1.1 {} X\r\nContent-Length: 0\r\nConnection: close\r\n\r\n", status).unwrap();
            head.extend_from_slice(&body);
            head
        });
        (url, handle)
    }

    #[test]
    fn uploads_file_as_multipart() {
        let path = std::env::temp_dir().join(format!("lapse-upload-{}.mp4", std::process::id()));
        std::fs::write(&path, b"clip bytes").unwrap();
        let (url, server) = serve_once(200);
        super::post_file(&url, "file", &path, 10).unwrap();
        let sent = String::from_utf8(server.join().unwrap()).unwrap();
        let _ = std::fs::remove_file(&path);
        assert!(sent.contains("multipart/form-data; boundary=lapse"));
        assert!(sent.contains("name=\"file\"; filename=\"lapse-upload-"));
        assert!(sent.contains("\r\n\r\nclip bytes\r\n--lapse"));
    }

    #[test]
    fn reports_http_status() {
        let path = std::env::temp_dir().join(format!("lapse-upload-big-{}.mp4", std::process::id()));
        std::fs::write(&path, b"x").unwrap();
        let (url, server) = serve_once(413);
        let result = super::post_file(&url, "file", &path, 10);
        server.join().unwrap();
        let _ = std::fs::remove_file(&path);
        assert!(matches!(result, Err(super::Error::Status(413))));
    }

    #[test]
    fn post_empty_auth_returns_status() {
        let (url, server) = serve_once(401);
        let res = super::post_empty_auth(&url, ("user", "pass"), 10);
        let sent = String::from_utf8(server.join().unwrap()).unwrap();
        assert_eq!(res.unwrap(), 401);
        assert!(sent.contains("Authorization: Basic"));
    }

    /// Needs the internet: `cargo test http_live -- --ignored`.
    #[test]
    #[ignore]
    fn http_live() {
        let rel = super::get_json("https://api.github.com/repos/canersin/Lapse/releases/latest", &[], 10).unwrap();
        assert!(rel["tag_name"].as_str().unwrap().starts_with('v'));
        let dest = std::env::temp_dir().join(format!("lapse-license-{}", std::process::id()));
        let sum = super::download("https://raw.githubusercontent.com/canersin/Lapse/main/LICENSE", &dest, 20).unwrap();
        let bytes = std::fs::read(&dest).unwrap();
        let _ = std::fs::remove_file(&dest);
        let want: String = ring::digest::digest(&ring::digest::SHA256, &bytes).as_ref().iter().map(|b| format!("{:02x}", b)).collect();
        assert_eq!(sum, want);
        assert!(String::from_utf8_lossy(&bytes).contains("GNU GENERAL PUBLIC LICENSE"));
    }
}
