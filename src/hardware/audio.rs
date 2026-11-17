use std::process::Command;

#[derive(Clone, PartialEq, Debug)]
pub struct AudioDevice {
    pub name: String,
    pub description: String,
}

pub fn get_audio_devices() -> (Vec<AudioDevice>, Vec<AudioDevice>) {
    let output = match Command::new("gpu-screen-recorder").arg("--list-audio-devices").output() {
        Ok(out) => out,
        Err(_) => return (vec![], vec![]),
    };
    
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut inputs = Vec::new();
    let mut outputs = Vec::new();
    
    for line in stdout.lines() {
        let parts: Vec<&str> = line.splitn(2, '|').collect();
        if parts.len() == 2 {
            let name = parts[0].to_string();
            let description = parts[1].to_string();
            let device = AudioDevice { name: name.clone(), description };
            
            if name.contains("input") {
                inputs.push(device);
            } else if name.contains("output") {
                outputs.push(device);
            }
        }
    }
    
    (outputs, inputs)
}
