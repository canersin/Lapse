use std::sync::{Arc, Mutex, OnceLock};
use std::thread;

#[derive(Clone, Debug, PartialEq)]
pub enum UpdateStatus {
    Idle,
    Checking,
    UpToDate,
    Available {
        version: String,
        url: String,
    },
    Error(String),
}

#[derive(serde::Deserialize)]
struct GitHubRelease {
    tag_name: String,
    html_url: Option<String>,
}

static UPDATE_STATUS: OnceLock<Arc<Mutex<UpdateStatus>>> = OnceLock::new();

pub fn get_update_state() -> Arc<Mutex<UpdateStatus>> {
    UPDATE_STATUS
        .get_or_init(|| Arc::new(Mutex::new(UpdateStatus::Idle)))
        .clone()
}

pub fn check_for_updates() {
    let state = get_update_state();
    if let Ok(mut g) = state.lock() {
        if *g == UpdateStatus::Checking {
            return;
        }
        *g = UpdateStatus::Checking;
    }

    thread::spawn(move || {
        let output = crate::http::get_json("https://api.github.com/repos/canersin/Lapse/releases/latest", &[], 6);

        let new_status = match output {
            Ok(json) => {
                if let Ok(rel) = serde_json::from_value::<GitHubRelease>(json) {
                    let remote_ver = rel.tag_name.trim_start_matches('v');
                    let current_ver = env!("CARGO_PKG_VERSION");
                    if is_newer_version(remote_ver, current_ver) {
                        UpdateStatus::Available {
                            version: rel.tag_name,
                            url: rel.html_url.unwrap_or_else(|| "https://github.com/canersin/Lapse/releases".into()),
                        }
                    } else {
                        UpdateStatus::UpToDate
                    }
                } else {
                    UpdateStatus::Error("Could not parse release data".into())
                }
            }
            _ => UpdateStatus::Error("Network check failed".into()),
        };

        if let Ok(mut g) = state.lock() {
            *g = new_status;
        }
    });
}

fn is_newer_version(remote: &str, current: &str) -> bool {
    let parse = |v: &str| -> Vec<u32> {
        v.split('.')
            .filter_map(|part| part.chars().take_while(|c| c.is_ascii_digit()).collect::<String>().parse::<u32>().ok())
            .collect()
    };

    let r_parts = parse(remote);
    let c_parts = parse(current);

    for (r, c) in r_parts.iter().zip(c_parts.iter()) {
        if r > c {
            return true;
        }
        if r < c {
            return false;
        }
    }
    r_parts.len() > c_parts.len()
}
