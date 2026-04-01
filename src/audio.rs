use std::process::Command;

#[derive(Clone, PartialEq, Debug)]
pub struct AudioDevice {
    pub name: String,
    pub description: String,
}

pub fn get_audio_devices(is_sink: bool) -> Vec<AudioDevice> {
    let arg = if is_sink { "sinks" } else { "sources" };
    
    let output = match Command::new("pactl").arg("list").arg(arg).output() {
        Ok(out) => out,
        Err(_) => return vec![], // pactl not found or error
    };
    
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut devices = Vec::new();
    
    let mut current_name = String::new();
    let mut current_desc = String::new();
    
    for line in stdout.lines() {
        let trimmed = line.trim();
        if line.starts_with("Sink #") || line.starts_with("Source #") {
            if !current_name.is_empty() {
                devices.push(AudioDevice {
                    name: current_name.clone(),
                    description: if current_desc.is_empty() { current_name.clone() } else { current_desc.clone() },
                });
                current_name.clear();
                current_desc.clear();
            }
        }
        if trimmed.starts_with("Name: ") {
            current_name = trimmed.replace("Name: ", "").trim().to_string();
        } else if trimmed.starts_with("Description: ") {
            current_desc = trimmed.replace("Description: ", "").trim().to_string();
        }
    }
    
    if !current_name.is_empty() {
        devices.push(AudioDevice {
            name: current_name.clone(),
            description: if current_desc.is_empty() { current_name } else { current_desc },
        });
    }
    
    devices
}
