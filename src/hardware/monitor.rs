use crate::utils::run_system_cmd;

#[derive(Clone, PartialEq, Debug)]
pub struct MonitorDevice {
    pub name: String,
    pub resolution: String,
    pub card_name: String,
}

pub fn get_monitors() -> Vec<MonitorDevice> {
    let mut monitors = Vec::new();

    if let Ok(entries) = std::fs::read_dir("/sys/class/drm") {
        for entry in entries.flatten() {
            let path = entry.path();
            let fname = entry.file_name().to_string_lossy().to_string();
            if !fname.contains('-') || fname.starts_with("render") || fname.starts_with("version") {
                continue;
            }

            let status_file = path.join("status");
            if let Ok(status) = std::fs::read_to_string(status_file) {
                if status.trim() == "connected" {
                    let (card_str, connector_name) = if let Some(idx) = fname.find('-') {
                        (fname[..idx].to_string(), fname[idx + 1..].to_string())
                    } else {
                        ("card0".to_string(), fname.clone())
                    };

                    let mut res = "Native".to_string();
                    let modes_file = path.join("modes");
                    if let Ok(modes) = std::fs::read_to_string(modes_file) {
                        if let Some(first_mode) = modes.lines().next() {
                            if !first_mode.trim().is_empty() {
                                res = first_mode.trim().to_string();
                            }
                        }
                    }

                    if !monitors
                        .iter()
                        .any(|m: &MonitorDevice| m.name == connector_name)
                    {
                        monitors.push(MonitorDevice {
                            name: connector_name,
                            resolution: res,
                            card_name: card_str,
                        });
                    }
                }
            }
        }
    }

    if let Some(output) = run_system_cmd("gpu-screen-recorder", &["--list-monitors"]) {
        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            let parts: Vec<&str> = line.splitn(2, '|').collect();
            if parts.len() == 2 {
                let name = parts[0].trim().to_string();
                let resolution = parts[1].trim().to_string();
                if let Some(existing) = monitors.iter_mut().find(|m| m.name == name) {
                    if existing.resolution == "Native" || existing.resolution.is_empty() {
                        existing.resolution = resolution;
                    }
                } else if !name.is_empty() {
                    monitors.push(MonitorDevice {
                        name,
                        resolution,
                        card_name: "card0".into(),
                    });
                }
            }
        }
    }

    monitors.sort_by(|a, b| {
        a.card_name
            .cmp(&b.card_name)
            .then_with(|| a.name.cmp(&b.name))
    });
    monitors
}

#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct MonitorRect {
    pub name: String,
    pub is_primary: bool,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

pub fn get_detailed_monitors() -> Vec<MonitorRect> {
    let output = match run_system_cmd("xrandr", &[]) {
        Some(o) => o,
        None => return vec![],
    };

    let text = String::from_utf8_lossy(&output.stdout);
    let mut list = Vec::new();

    for line in text.lines() {
        if !line.contains(" connected") {
            continue;
        }

        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.is_empty() {
            continue;
        }

        let name = parts[0].to_string();
        let is_primary = line.contains(" primary ");

        for token in &parts {
            if let Some((w_h, x_y)) = token.split_once('+') {
                if let Some((w_str, h_str)) = w_h.split_once('x') {
                    if let Some((x_str, y_str)) = x_y.split_once('+') {
                        if let (Ok(w), Ok(h), Ok(x), Ok(y)) = (
                            w_str.parse::<i32>(),
                            h_str.parse::<i32>(),
                            x_str.parse::<i32>(),
                            y_str.parse::<i32>(),
                        ) {
                            list.push(MonitorRect {
                                name: name.clone(),
                                is_primary,
                                x,
                                y,
                                width: w,
                                height: h,
                            });
                            break;
                        }
                    }
                }
            }
        }
    }

    list
}

#[allow(dead_code)]
pub fn get_primary_monitor_name() -> Option<String> {
    let detailed = get_detailed_monitors();
    if let Some(m) = detailed.iter().find(|m| m.is_primary) {
        return Some(m.name.clone());
    }

    if let Some(first) = detailed.first() {
        return Some(first.name.clone());
    }

    let simple = get_monitors();
    simple.first().map(|m| m.name.clone())
}

#[allow(dead_code)]
pub fn find_monitor_at(x: i32, y: i32) -> Option<String> {
    let detailed = get_detailed_monitors();
    for m in &detailed {
        if x >= m.x && x < m.x + m.width && y >= m.y && y < m.y + m.height {
            return Some(m.name.clone());
        }
    }
    None
}

pub fn get_game_or_primary_monitor(configured_monitor: &str) -> String {
    if !configured_monitor.is_empty()
        && configured_monitor != "auto"
        && configured_monitor != "focused"
        && configured_monitor != "screen"
    {
        return configured_monitor.to_string();
    }
    "focused".to_string()
}
