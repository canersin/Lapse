use std::process::Command;

#[derive(Clone, PartialEq, Debug)]
pub struct GpuDevice {
    pub pci_id: String,
    pub name: String,
}

pub fn get_gpus() -> Vec<GpuDevice> {
    let mut gpus = Vec::new();
    if let Ok(output) = Command::new("lspci").arg("-nn").output() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            if line.contains("VGA compatible controller") || line.contains("3D controller") {
                let parts: Vec<&str> = line.splitn(2, ' ').collect();
                if parts.len() == 2 {
                    let pci_raw = parts[0];
                    let pci_clean = pci_raw.replace(":", "_").replace(".", "_");
                    let pci_id = format!("pci-0000_{}", pci_clean);
                    
                    let desc = parts[1];
                    let name = if let Some(idx) = desc.find(": ") {
                        let full_desc = desc[idx + 2..].to_string();

                        if let Some(rev_idx) = full_desc.find(" (rev") {
                            full_desc[..rev_idx].trim().to_string()
                        } else {
                            full_desc
                        }
                    } else {
                        desc.to_string()
                    };
                    
                    gpus.push(GpuDevice {
                        pci_id,
                        name: if name.is_empty() { "Unknown GPU".into() } else { name },
                    });
                }
            }
        }
    }
    gpus
}
