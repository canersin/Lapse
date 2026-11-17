use std::collections::HashMap;
use std::process::Command;

#[derive(Clone, PartialEq, Debug)]
pub struct GpuDevice {
    pub pci_id: String,
    pub name: String,
    pub card_path: String,
    pub render_node: String,
    pub is_primary: bool,
    pub connected_monitors: Vec<String>,
    /// PCI vendor id, e.g. "1002" for AMD.
    pub vendor_id: String,
    /// PCI device id, e.g. "7550" for Navi 48.
    pub device_id: String,
}

#[allow(dead_code)]
impl GpuDevice {
    /// Marketing name without the vendor prefix, e.g. "Navi 48 [Radeon RX 9070/9070 XT/9070 GRE]".
    pub fn short_name(&self) -> String {
        let n = if self
            .name
            .starts_with("Advanced Micro Devices, Inc. [AMD/ATI] ")
        {
            self.name
                .trim_start_matches("Advanced Micro Devices, Inc. [AMD/ATI] ")
                .to_string()
        } else if self.name.starts_with("NVIDIA Corporation ") {
            self.name
                .trim_start_matches("NVIDIA Corporation ")
                .to_string()
        } else if self.name.starts_with("Intel Corporation ") {
            self.name
                .trim_start_matches("Intel Corporation ")
                .to_string()
        } else {
            self.name.clone()
        };
        // Prefer the bracketed marketing name ("Radeon RX 9070/9070 XT/9070 GRE") when present,
        // skipping the "[1002:7550]" vendor:device suffix that `lspci -nn` appends.
        if let Some(a) = n.find('[') {
            if let Some(len) = n[a + 1..].find(']') {
                let inner = &n[a + 1..a + 1 + len];
                let is_pci_ids = inner.len() == 9 && inner.as_bytes()[4] == b':';
                if !inner.is_empty() && !is_pci_ids {
                    return inner.to_string();
                }
            }
        }
        match n.rfind(" [") {
            Some(i) if n.ends_with(']') => n[..i].to_string(),
            _ => n,
        }
    }

    pub fn display_name(&self) -> String {
        let short_name = self.short_name();

        if self.connected_monitors.is_empty() {
            format!("{} ({} - No Display Connected)", short_name, self.card_path)
        } else {
            let count = self.connected_monitors.len();
            let label = if count == 1 { "Display" } else { "Displays" };
            format!("{} ({} - {} {})", short_name, self.card_path, count, label)
        }
    }

    pub fn matches(&self, key: &str) -> bool {
        if key.eq_ignore_ascii_case("auto") {
            return false;
        }
        self.pci_id.eq_ignore_ascii_case(key)
            || self.card_path.eq_ignore_ascii_case(key)
            || self.render_node.eq_ignore_ascii_case(key)
            || self.name.eq_ignore_ascii_case(key)
            || self.display_name().eq_ignore_ascii_case(key)
            || (self.card_path.ends_with(key) && !key.is_empty())
    }

    /// Value for the DRI_PRIME environment variable that makes Mesa (OpenGL and
    /// Vulkan through the device-select layer) render on this GPU.
    pub fn dri_prime(&self) -> Option<String> {
        if !self.pci_id.contains(':') {
            return None;
        }
        let slot = if self.pci_id.matches(':').count() == 1 {
            format!("0000:{}", self.pci_id)
        } else {
            self.pci_id.clone()
        };
        Some(format!("pci-{}", slot.replace([':', '.'], "_")))
    }

    /// Value for MESA_VK_DEVICE_SELECT ("vendor:device").
    pub fn vk_device_select(&self) -> Option<String> {
        if self.vendor_id.is_empty() || self.device_id.is_empty() {
            return None;
        }
        Some(format!("{}:{}", self.vendor_id, self.device_id))
    }

    pub fn card_name(&self) -> &str {
        self.card_path.trim_start_matches("/dev/dri/")
    }

    pub fn supports_av1(&self) -> bool {
        let n = self.name.to_lowercase();
        if n.contains("navi 3")
            || n.contains("navi 4")
            || n.contains("rx 7")
            || n.contains("rx 8")
            || n.contains("rx 9")
        {
            return true;
        }
        if n.contains("rtx 40")
            || n.contains("rtx 50")
            || n.contains("ada lovelace")
            || n.contains("blackwell")
        {
            return true;
        }
        if n.contains("arc ") || n.contains("alchemist") || n.contains("battlemage") {
            return true;
        }
        false
    }

    pub fn supports_hevc(&self) -> bool {
        let n = self.name.to_lowercase();
        if n.contains("gtx 6") || n.contains("gtx 7") {
            return false;
        }
        true
    }
}

pub fn get_gpus() -> Vec<GpuDevice> {
    let mut lspci_map: HashMap<String, String> = HashMap::new();
    if let Ok(output) = Command::new("lspci").arg("-nn").output() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            if line.contains("VGA compatible controller") || line.contains("3D controller") {
                let parts: Vec<&str> = line.splitn(2, ' ').collect();
                if parts.len() == 2 {
                    let slot = parts[0].trim();
                    let desc = parts[1].trim();
                    let name = if let Some(idx) = desc.find(": ") {
                        let full = desc[idx + 2..].to_string();
                        if let Some(rev_idx) = full.find(" (rev") {
                            full[..rev_idx].trim().to_string()
                        } else {
                            full
                        }
                    } else {
                        desc.to_string()
                    };
                    lspci_map.insert(slot.to_string(), name);
                }
            }
        }
    }

    let mut gpus = Vec::new();
    let mut card_dirs = Vec::new();
    if let Ok(entries) = std::fs::read_dir("/sys/class/drm") {
        for entry in entries.flatten() {
            let fname = entry.file_name().to_string_lossy().to_string();
            if fname.starts_with("card") && !fname.contains('-') {
                card_dirs.push((fname, entry.path()));
            }
        }
    }
    card_dirs.sort_by(|a, b| a.0.cmp(&b.0));

    for (card_name, card_path_buf) in card_dirs {
        let dev_link = card_path_buf.join("device");
        let pci_slot = if let Ok(target) = std::fs::read_link(&dev_link) {
            target
                .file_name()
                .map(|f| f.to_string_lossy().to_string())
                .unwrap_or_default()
        } else {
            String::new()
        };

        let pci_short = if pci_slot.starts_with("0000:") {
            pci_slot[5..].to_string()
        } else {
            pci_slot.clone()
        };

        let name = lspci_map
            .get(&pci_short)
            .or_else(|| lspci_map.get(&pci_slot))
            .cloned()
            .unwrap_or_else(|| {
                if pci_slot.is_empty() {
                    format!("GPU ({})", card_name)
                } else {
                    format!("GPU ({})", pci_slot)
                }
            });

        let mut render_node = String::new();
        if let Ok(entries) = std::fs::read_dir("/sys/class/drm") {
            for entry in entries.flatten() {
                let rname = entry.file_name().to_string_lossy().to_string();
                if rname.starts_with("renderD") {
                    let rlink = entry.path().join("device");
                    if let Ok(rtarget) = std::fs::read_link(&rlink) {
                        let rslot = rtarget
                            .file_name()
                            .map(|f| f.to_string_lossy().to_string())
                            .unwrap_or_default();
                        if !rslot.is_empty() && rslot == pci_slot {
                            render_node = format!("/dev/dri/{}", rname);
                            break;
                        }
                    }
                }
            }
        }

        let mut connected_monitors = Vec::new();
        if let Ok(entries) = std::fs::read_dir("/sys/class/drm") {
            let prefix = format!("{}-", card_name);
            for entry in entries.flatten() {
                let cname = entry.file_name().to_string_lossy().to_string();
                if cname.starts_with(&prefix) {
                    let status_file = entry.path().join("status");
                    if let Ok(status) = std::fs::read_to_string(status_file) {
                        if status.trim() == "connected" {
                            let mon = cname[prefix.len()..].to_string();
                            connected_monitors.push(mon);
                        }
                    }
                }
            }
        }
        connected_monitors.sort();

        let read_id = |f: &str| {
            std::fs::read_to_string(dev_link.join(f))
                .map(|v| v.trim().trim_start_matches("0x").to_string())
                .unwrap_or_default()
        };
        let vendor_id = read_id("vendor");
        let device_id = read_id("device");

        let is_primary = !connected_monitors.is_empty();
        gpus.push(GpuDevice {
            pci_id: if pci_slot.is_empty() {
                card_name.clone()
            } else {
                pci_slot
            },
            name,
            card_path: format!("/dev/dri/{}", card_name),
            render_node,
            is_primary,
            connected_monitors,
            vendor_id,
            device_id,
        });
    }

    if gpus.is_empty() {
        for (slot, name) in lspci_map {
            let pci_clean = slot.replace(":", "_").replace(".", "_");
            gpus.push(GpuDevice {
                pci_id: format!("pci-0000_{}", pci_clean),
                name,
                card_path: "/dev/dri/card0".into(),
                render_node: "/dev/dri/renderD128".into(),
                is_primary: true,
                connected_monitors: Vec::new(),
                vendor_id: String::new(),
                device_id: String::new(),
            });
        }
    }

    gpus.sort_by(|a, b| {
        b.is_primary
            .cmp(&a.is_primary)
            .then_with(|| a.card_path.cmp(&b.card_path))
    });
    gpus
}

/// Finds the GPU selected by a config key ("Auto" returns None).
pub fn find_gpu<'a>(gpus: &'a [GpuDevice], key: &str) -> Option<&'a GpuDevice> {
    if key.is_empty() || key.eq_ignore_ascii_case("auto") {
        return None;
    }
    gpus.iter().find(|g| g.matches(key))
}

/// The GPU a monitor connector (e.g. "DP-1") is physically plugged into.
pub fn gpu_for_monitor<'a>(gpus: &'a [GpuDevice], monitor: &str) -> Option<&'a GpuDevice> {
    gpus.iter().find(|g| g.connected_monitors.iter().any(|m| m == monitor))
}

/// Environment variables that make a game render on `gpu`.
pub fn render_env_for(gpu: &GpuDevice) -> Vec<(String, String)> {
    let mut env = Vec::new();
    if let Some(prime) = gpu.dri_prime() {
        env.push(("DRI_PRIME".to_string(), prime));
    }
    if let Some(vk) = gpu.vk_device_select() {
        env.push(("MESA_VK_DEVICE_SELECT".to_string(), vk));
    }
    if gpu.vendor_id == "10de" {
        env.push(("__NV_PRIME_RENDER_OFFLOAD".to_string(), "1".to_string()));
        env.push(("__GLX_VENDOR_LIBRARY_NAME".to_string(), "nvidia".to_string()));
    }
    env
}

/// Describes how a dual GPU setup is wired and what is wrong with it, if anything.
#[derive(Clone, Debug, Default)]
pub struct DualGpuReport {
    pub game_gpu: Option<GpuDevice>,
    pub encode_gpu: Option<GpuDevice>,
    pub problems: Vec<String>,
}

impl DualGpuReport {
    pub fn is_split(&self) -> bool {
        match (&self.game_gpu, &self.encode_gpu) {
            (Some(a), Some(b)) => a.pci_id != b.pci_id,
            _ => false,
        }
    }
}

/// gpu-screen-recorder always captures *and encodes* on the GPU the recorded
/// monitor is plugged into. So the dual GPU recipe is:
///   monitor cable -> encoder GPU, game -> rendered on the other GPU via PRIME.
pub fn dual_gpu_report(
    gpus: &[GpuDevice],
    game_key: &str,
    encode_key: &str,
    capture_monitor: &str,
) -> DualGpuReport {
    let mut report = DualGpuReport::default();

    let encode = find_gpu(gpus, encode_key)
        .or_else(|| gpu_for_monitor(gpus, capture_monitor))
        .or_else(|| gpus.iter().find(|g| g.is_primary));
    let game = find_gpu(gpus, game_key).or_else(|| {
        // Auto: the strongest-looking GPU that is not the encoder (first listed other card).
        gpus.iter()
            .find(|g| Some(g.pci_id.as_str()) != encode.map(|e| e.pci_id.as_str()))
    });

    if gpus.len() < 2 {
        report
            .problems
            .push("Only one GPU detected. Dual GPU recording needs two graphics cards.".into());
    }

    if let Some(enc) = encode {
        if enc.connected_monitors.is_empty() {
            report.problems.push(format!(
                "No monitor is plugged into {}. gpu-screen-recorder encodes on the GPU the recorded monitor is connected to, so plug your monitor into this card.",
                enc.short_name()
            ));
        } else if let Some(mon_gpu) = gpu_for_monitor(gpus, capture_monitor) {
            if mon_gpu.pci_id != enc.pci_id {
                report.problems.push(format!(
                    "Monitor {} is connected to {}, not {}. Encoding will happen on {}.",
                    capture_monitor,
                    mon_gpu.short_name(),
                    enc.short_name(),
                    mon_gpu.short_name()
                ));
            }
        }
    }

    if let (Some(g), Some(e)) = (game, encode) {
        if g.pci_id == e.pci_id && gpus.len() >= 2 {
            report
                .problems
                .push("Game GPU and encoder GPU are the same card.".into());
        }
    }

    report.game_gpu = game.cloned();
    report.encode_gpu = encode.cloned();
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gpu(pci: &str, card: &str, name: &str, dev: &str, monitors: &[&str]) -> GpuDevice {
        GpuDevice {
            pci_id: pci.into(),
            name: name.into(),
            card_path: format!("/dev/dri/{}", card),
            render_node: String::new(),
            is_primary: !monitors.is_empty(),
            connected_monitors: monitors.iter().map(|m| m.to_string()).collect(),
            vendor_id: "1002".into(),
            device_id: dev.into(),
        }
    }

    fn rig(monitor_on_7800: bool) -> Vec<GpuDevice> {
        let (a, b): (&[&str], &[&str]) = if monitor_on_7800 { (&[], &["DP-1"]) } else { (&["DP-1"], &[]) };
        vec![
            gpu("0000:03:00.0", "card1", "Advanced Micro Devices, Inc. [AMD/ATI] Navi 48 [Radeon RX 9070/9070 XT/9070 GRE] [1002:7550]", "7550", a),
            gpu("0000:0b:00.0", "card2", "Advanced Micro Devices, Inc. [AMD/ATI] Navi 32 [Radeon RX 7700 XT / 7800 XT] [1002:747e]", "747e", b),
        ]
    }

    #[test]
    fn short_name_skips_pci_ids() {
        assert_eq!(rig(true)[0].short_name(), "Radeon RX 9070/9070 XT/9070 GRE");
        let plain = gpu("0000:01:00.0", "card0", "Some GPU [10de:2684]", "2684", &[]);
        assert_eq!(plain.short_name(), "Some GPU");
    }

    #[test]
    fn render_env_targets_game_gpu() {
        let g = &rig(true)[0];
        assert_eq!(g.dri_prime().unwrap(), "pci-0000_03_00_0");
        assert_eq!(g.vk_device_select().unwrap(), "1002:7550");
    }

    #[test]
    fn correct_dual_setup_has_no_problems() {
        let gpus = rig(true);
        let r = dual_gpu_report(&gpus, "0000:03:00.0", "0000:0b:00.0", "DP-1");
        assert!(r.is_split());
        assert!(r.problems.is_empty(), "{:?}", r.problems);
        assert_eq!(r.encode_gpu.unwrap().card_name(), "card2");
    }

    #[test]
    fn auto_picks_monitor_gpu_as_encoder_and_other_as_game() {
        let gpus = rig(true);
        let r = dual_gpu_report(&gpus, "Auto", "Auto", "DP-1");
        assert_eq!(r.encode_gpu.unwrap().device_id, "747e");
        assert_eq!(r.game_gpu.unwrap().device_id, "7550");
    }

    #[test]
    fn monitor_on_wrong_gpu_is_reported() {
        let gpus = rig(false);
        let r = dual_gpu_report(&gpus, "0000:03:00.0", "0000:0b:00.0", "DP-1");
        assert!(r.problems.iter().any(|p| p.contains("No monitor is plugged into")));
    }
}
