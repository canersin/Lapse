#[cfg(not(windows))]
use std::process::Command;

#[derive(Clone, PartialEq, Debug)]
pub struct AudioDevice {
    pub name: String,
    pub description: String,
}

#[cfg(not(windows))]
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

#[cfg(windows)]
pub fn get_audio_devices() -> (Vec<AudioDevice>, Vec<AudioDevice>) {
    let _ = wasapi::initialize_mta();
    let enumerator = match wasapi::DeviceEnumerator::new() {
        Ok(e) => e,
        Err(_) => return (vec![], vec![]),
    };

    let get_devices_for_dir = |direction: wasapi::Direction| -> Vec<AudioDevice> {
        let mut list = Vec::new();
        if let Ok(coll) = enumerator.get_device_collection(direction) {
            if let Ok(count) = coll.get_nbr_devices() {
                for i in 0..count {
                    if let Ok(dev) = coll.get_device_at_index(i) {
                        if let Ok(name) = dev.get_friendlyname() {
                            list.push(AudioDevice {
                                name: name.clone(),
                                description: name,
                            });
                        }
                    }
                }
            }
        }
        list
    };

    let outputs = get_devices_for_dir(wasapi::Direction::Render);
    let inputs = get_devices_for_dir(wasapi::Direction::Capture);
    (outputs, inputs)
}
