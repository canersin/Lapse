//! League of Legends Live Client Data polling and multikill detection.

use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::recorder::Recorder;
use super::{Action, Game};

/// Custom certificate verifier that only trusts 127.0.0.1 / localhost on port 2999.
#[derive(Debug)]
pub struct LocalhostCertVerifier;

impl ureq::rustls::client::danger::ServerCertVerifier for LocalhostCertVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &ureq::rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[ureq::rustls::pki_types::CertificateDer<'_>],
        server_name: &ureq::rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: ureq::rustls::pki_types::UnixTime,
    ) -> Result<ureq::rustls::client::danger::ServerCertVerified, ureq::rustls::Error> {
        match server_name {
            ureq::rustls::pki_types::ServerName::IpAddress(ip)
                if std::net::IpAddr::from(*ip) == std::net::IpAddr::V4(std::net::Ipv4Addr::new(127, 0, 0, 1)) =>
            {
                Ok(ureq::rustls::client::danger::ServerCertVerified::assertion())
            }
            ureq::rustls::pki_types::ServerName::DnsName(dns)
                if dns.as_ref() == "localhost" || dns.as_ref() == "127.0.0.1" =>
            {
                Ok(ureq::rustls::client::danger::ServerCertVerified::assertion())
            }
            _ => Err(ureq::rustls::Error::InvalidCertificate(
                ureq::rustls::CertificateError::ApplicationVerificationFailure,
            )),
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &ureq::rustls::pki_types::CertificateDer<'_>,
        dss: &ureq::rustls::DigitallySignedStruct,
    ) -> Result<ureq::rustls::client::danger::HandshakeSignatureValid, ureq::rustls::Error> {
        ureq::rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &ureq::rustls::crypto::ring::default_provider().signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &ureq::rustls::pki_types::CertificateDer<'_>,
        dss: &ureq::rustls::DigitallySignedStruct,
    ) -> Result<ureq::rustls::client::danger::HandshakeSignatureValid, ureq::rustls::Error> {
        ureq::rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &ureq::rustls::crypto::ring::default_provider().signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<ureq::rustls::SignatureScheme> {
        ureq::rustls::crypto::ring::default_provider()
            .signature_verification_algorithms
            .supported_schemes()
    }
}

/// Builds a ureq Agent configured for League of Legends' self-signed HTTPS server on 127.0.0.1:2999.
pub fn create_lol_agent() -> ureq::Agent {
    let client_config = ureq::rustls::ClientConfig::builder_with_provider(
        ureq::rustls::crypto::ring::default_provider().into(),
    )
    .with_protocol_versions(&[&ureq::rustls::version::TLS12, &ureq::rustls::version::TLS13])
    .expect("TLS protocol versions valid")
    .dangerous()
    .with_custom_certificate_verifier(Arc::new(LocalhostCertVerifier))
    .with_no_client_auth();

    ureq::builder()
        .tls_config(Arc::new(client_config))
        .timeout(Duration::from_millis(800))
        .build()
}

/// Checks whether League of Legends is running right now using the game detector.
pub fn is_lol_running() -> bool {
    let entry = crate::config::GameEntry {
        name: "League of Legends".into(),
        matcher: "League of Legends".into(),
        ..Default::default()
    };
    crate::hardware::game_detector::is_running(&entry)
}

const STREAK_WINDOW: Duration = Duration::from_secs(18);
const QUIET_AFTER_KILL: Duration = Duration::from_secs(10);

#[derive(Default, Debug)]
pub struct LolTracker {
    active_player: Option<String>,
    last_event_id: Option<i64>,
    streak: u32,
    saved: u32,
    aced: bool,
    first_kill: Option<Instant>,
    last_kill: Option<Instant>,
    new_kills: u32,
}

impl LolTracker {
    pub fn set_active_player(&mut self, name: String) {
        let clean = name.trim().trim_matches('"').to_string();
        if !clean.is_empty() {
            self.active_player = Some(clean);
        }
    }

    pub fn active_player(&self) -> Option<&str> {
        self.active_player.as_deref()
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn take_new_kills(&mut self) -> u32 {
        std::mem::take(&mut self.new_kills)
    }

    fn finish(&mut self, min_kills: u32, now: Instant) -> Action {
        let is_multikill = self.streak >= min_kills && self.streak > self.saved;
        let is_ace = self.aced && self.saved == 0;
        if !is_multikill && !is_ace {
            return Action::Nothing;
        }
        let Some(first) = self.first_kill else { return Action::Nothing; };
        self.saved = self.streak.max(if self.aced { 5 } else { 0 });
        let secs = now.saturating_duration_since(first).as_secs() as u32;

        let what = if self.aced && self.streak < 2 {
            "Ace"
        } else {
            match self.streak {
                2 => "Double_Kill",
                3 => "Triple_Kill",
                4 => "Quadra_Kill",
                5 => "Penta_Kill",
                _ => if self.aced { "Ace" } else { "Multikill" },
            }
        };
        let label = what.to_string();
        let kills = if self.streak > 0 { self.streak } else { 1 };
        Action::Save { secs, kills, label }
    }

    fn end_streak(&mut self) {
        self.streak = 0;
        self.saved = 0;
        self.aced = false;
        self.first_kill = None;
        self.last_kill = None;
    }

    pub fn update(&mut self, state: &serde_json::Value, min_kills: u32, now: Instant) -> Action {
        let Some(active) = self.active_player.clone() else {
            return Action::Nothing;
        };
        let Some(events_array) = state.get("Events").and_then(|e| e.as_array()) else {
            return Action::Nothing;
        };

        if self.last_event_id.is_none() {
            // Baseline poll: record highest EventID, ignore past events
            let max_id = events_array
                .iter()
                .filter_map(|ev| ev.get("EventID").and_then(|id| id.as_i64()))
                .max()
                .unwrap_or(-1);
            self.last_event_id = Some(max_id);
            return Action::Nothing;
        }

        let mut highest_seen = self.last_event_id.unwrap_or(-1);
        let mut player_died = false;

        for ev in events_array {
            let event_id = ev.get("EventID").and_then(|id| id.as_i64()).unwrap_or(-1);
            if event_id <= self.last_event_id.unwrap_or(-1) {
                continue;
            }
            if event_id > highest_seen {
                highest_seen = event_id;
            }

            let event_name = ev.get("EventName").and_then(|n| n.as_str()).unwrap_or("");
            match event_name {
                "ChampionKill" => {
                    let killer = ev.get("KillerName").and_then(|k| k.as_str()).unwrap_or("");
                    let victim = ev.get("VictimName").and_then(|v| v.as_str()).unwrap_or("");
                    if killer == active {
                        self.new_kills += 1;
                        let in_window = self.last_kill.map(|l| now.saturating_duration_since(l) <= STREAK_WINDOW).unwrap_or(false);
                        if !in_window {
                            self.end_streak();
                            self.first_kill = Some(now);
                        }
                        self.last_kill = Some(now);
                    } else if victim == active {
                        player_died = true;
                    }
                }
                "Multikill" => {
                    let killer = ev.get("KillerName").and_then(|k| k.as_str()).unwrap_or("");
                    if killer == active {
                        let streak = ev.get("KillStreak").and_then(|k| k.as_u64()).map(|k| k as u32).unwrap_or(2);
                        self.streak = self.streak.max(streak);
                        if self.first_kill.is_none() {
                            self.first_kill = Some(now);
                        }
                        self.last_kill = Some(now);
                    }
                }
                "Ace" => {
                    let acer = ev.get("Acer").and_then(|k| k.as_str()).unwrap_or("");
                    if acer == active {
                        self.aced = true;
                        if self.first_kill.is_none() {
                            self.first_kill = Some(now);
                        }
                        self.last_kill = Some(now);
                    }
                }
                _ => {}
            }
        }

        self.last_event_id = Some(highest_seen);

        if player_died {
            let action = self.finish(min_kills, now);
            self.end_streak();
            action
        } else {
            Action::Nothing
        }
    }

    pub fn poll(&mut self, min_kills: u32, now: Instant) -> Action {
        let Some(last) = self.last_kill else { return Action::Nothing; };
        let quiet = now.saturating_duration_since(last);
        if quiet >= STREAK_WINDOW {
            let action = self.finish(min_kills, now);
            self.end_streak();
            action
        } else if quiet >= QUIET_AFTER_KILL {
            self.finish(min_kills, now)
        } else {
            Action::Nothing
        }
    }
}

/// Background poller for League of Legends game events. Runs only while the game is open.
pub fn start_poller(recorder: Arc<Mutex<Recorder>>) {
    std::thread::spawn(move || {
        let agent = create_lol_agent();
        let mut tracker = LolTracker::default();

        loop {
            let (enabled, min_kills) = match recorder.lock() {
                Ok(rec) => (rec.config().lol_auto_clip, rec.config().lol_min_kills),
                Err(_) => {
                    std::thread::sleep(Duration::from_secs(1));
                    continue;
                }
            };

            if !enabled {
                std::thread::sleep(Duration::from_secs(2));
                continue;
            }

            // Cheap check: at most every few seconds while not running
            if !is_lol_running() {
                if tracker.active_player().is_some() {
                    tracker.reset();
                }
                std::thread::sleep(Duration::from_secs(3));
                continue;
            }

            if tracker.active_player().is_none() {
                if let Ok(resp) = agent.get("https://127.0.0.1:2999/liveclientdata/activeplayername").call() {
                    if let Ok(text) = resp.into_string() {
                        tracker.set_active_player(text);
                    }
                }
            }

            let now = Instant::now();

            if tracker.active_player().is_some() {
                if let Ok(resp) = agent.get("https://127.0.0.1:2999/liveclientdata/eventdata").call() {
                    if let Ok(text) = resp.into_string() {
                        if let Ok(val) = serde_json::from_str::<serde_json::Value>(&text) {
                            super::LAST_UPDATE[Game::Lol.index()].store(super::unix_now(), Ordering::Relaxed);
                            let action = tracker.update(&val, min_kills, now);
                            let kills = tracker.take_new_kills();
                            if kills > 0 {
                                if let Ok(mut rec) = recorder.lock() {
                                    rec.add_kill_marks(kills);
                                }
                            }
                            super::run(Game::Lol, action, &recorder);
                        }
                    }
                }

                let action = tracker.poll(min_kills, now);
                super::run(Game::Lol, action, &recorder);
            }

            std::thread::sleep(Duration::from_secs(1));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lol_multikill_parsing() {
        let mut tracker = LolTracker::default();
        tracker.set_active_player("\"Faker\"".into());
        assert_eq!(tracker.active_player(), Some("Faker"));

        let t0 = Instant::now();

        // 1. Initial poll is baseline: ignore old events
        let baseline = serde_json::json!({
            "Events": [
                { "EventID": 0, "EventName": "GameStart" },
                { "EventID": 1, "EventName": "ChampionKill", "KillerName": "Faker", "VictimName": "Rookie" },
                { "EventID": 2, "EventName": "Multikill", "KillerName": "Faker", "KillStreak": 2 }
            ]
        });
        assert_eq!(tracker.update(&baseline, 2, t0), Action::Nothing);

        // 2. Faker scores kill 1 of a new streak at t0 + 1s
        let kill1 = serde_json::json!({
            "Events": [
                { "EventID": 3, "EventName": "ChampionKill", "KillerName": "Faker", "VictimName": "TheShy" }
            ]
        });
        assert_eq!(tracker.update(&kill1, 2, t0 + Duration::from_secs(1)), Action::Nothing);

        // 3. Faker scores kill 2 + Double Kill event at t0 + 5s
        let kill2 = serde_json::json!({
            "Events": [
                { "EventID": 4, "EventName": "ChampionKill", "KillerName": "Faker", "VictimName": "JackeyLove" },
                { "EventID": 5, "EventName": "Multikill", "KillerName": "Faker", "KillStreak": 2 }
            ]
        });
        assert_eq!(tracker.update(&kill2, 2, t0 + Duration::from_secs(5)), Action::Nothing);

        // 4. Quiet for 10 s after kill 2: double kill is saved!
        let save_action = tracker.poll(2, t0 + Duration::from_secs(15));
        assert_eq!(
            save_action,
            Action::Save {
                secs: 14, // from t0+1s to t0+15s
                kills: 2,
                label: "Double_Kill".into(),
            }
        );

        // 5. Subsequent poll inside STREAK_WINDOW doesn't re-save the same streak
        assert_eq!(tracker.poll(2, t0 + Duration::from_secs(16)), Action::Nothing);

        // 6. Faker gets a 3rd kill inside the 18s window: Triple Kill!
        let kill3 = serde_json::json!({
            "Events": [
                { "EventID": 6, "EventName": "ChampionKill", "KillerName": "Faker", "VictimName": "Baolan" },
                { "EventID": 7, "EventName": "Multikill", "KillerName": "Faker", "KillStreak": 3 }
            ]
        });
        assert_eq!(tracker.update(&kill3, 2, t0 + Duration::from_secs(17)), Action::Nothing);

        // Quiet for 10 s after kill 3 (t0 + 27s): triple kill is saved!
        let save_triple = tracker.poll(2, t0 + Duration::from_secs(27));
        assert_eq!(
            save_triple,
            Action::Save {
                secs: 26,
                kills: 3,
                label: "Triple_Kill".into(),
            }
        );

        // After STREAK_WINDOW (18s after t0+17s = t0+36s), streak ends
        assert_eq!(tracker.poll(2, t0 + Duration::from_secs(36)), Action::Nothing);
        assert_eq!(tracker.streak, 0);
    }

    #[test]
    fn lol_ace_parsing() {
        let mut tracker = LolTracker::default();
        tracker.set_active_player("Faker".into());
        let t0 = Instant::now();

        let baseline = serde_json::json!({ "Events": [] });
        tracker.update(&baseline, 3, t0);

        // Faker scores an Ace
        let ace = serde_json::json!({
            "Events": [
                { "EventID": 1, "EventName": "ChampionKill", "KillerName": "Faker", "VictimName": "Ruler" },
                { "EventID": 2, "EventName": "Ace", "Acer": "Faker", "AcingTeam": "ORDER" }
            ]
        });
        tracker.update(&ace, 3, t0 + Duration::from_secs(2));

        // 10s quiet: Ace saved even if min_kills was 3
        let action = tracker.poll(3, t0 + Duration::from_secs(12));
        assert_eq!(
            action,
            Action::Save {
                secs: 10,
                kills: 1,
                label: "Ace".into(),
            }
        );
    }

    #[test]
    fn lol_tls_verifier_localhost_only() {
        use ureq::rustls::client::danger::ServerCertVerifier;
        use ureq::rustls::pki_types::{CertificateDer, ServerName, UnixTime};

        let verifier = LocalhostCertVerifier;
        let empty_cert = CertificateDer::from(vec![]);
        let now = UnixTime::now();

        // 127.0.0.1 should pass verification
        let localhost_ip = ServerName::try_from("127.0.0.1").unwrap();
        assert!(verifier.verify_server_cert(&empty_cert, &[], &localhost_ip, &[], now).is_ok());

        // localhost should pass verification
        let localhost_dns = ServerName::try_from("localhost").unwrap();
        assert!(verifier.verify_server_cert(&empty_cert, &[], &localhost_dns, &[], now).is_ok());

        // google.com should fail verification
        let remote = ServerName::try_from("google.com").unwrap();
        assert!(verifier.verify_server_cert(&empty_cert, &[], &remote, &[], now).is_err());
    }
}
