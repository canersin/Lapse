use std::process::Command;

#[derive(Clone, PartialEq, Debug)]
pub struct MonitorDevice {
    pub name: String,
    pub resolution: String,
}

pub fn get_monitors() -> Vec<MonitorDevice> {
    let output = match Command::new("gpu-screen-recorder").arg("--list-monitors").output() {
        Ok(out) => out,
        Err(_) => return vec![],
    };
    
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut monitors = Vec::new();
    
    for line in stdout.lines() {
        let parts: Vec<&str> = line.splitn(2, '|').collect();
        if parts.len() == 2 {
            monitors.push(MonitorDevice {
                name: parts[0].to_string(),
                resolution: parts[1].to_string(),
            });
        }
    }
    
    monitors
}
