//! Round-based games (Counter-Strike 2): a clip when the player gets the minimum number
//! of kills in one round.
//!
//! The clip runs from the first kill until the next round begins, so a 3k or an ace is one
//! clip and the end of the round (exit frags, the round-win banner) is in it. It never ends
//! sooner than a few seconds after the last kill, so that kill is surely in the buffer.

use std::time::{Duration, Instant};

use super::{label, short_name, Action};

/// The clip goes on at least this long after the last kill.
const AFTER_LAST_KILL: Duration = Duration::from_secs(8);
/// After a round ends the next one normally begins within 7 seconds; save by then even
/// if the game never says so (the match's last round, leaving the server).
const NEXT_ROUND_WAIT: Duration = Duration::from_secs(10);
/// Left the server mid-round: save anyway once this much time has passed since the last kill.
const GIVE_UP_AFTER_KILL: Duration = Duration::from_secs(180);

/// Kills of the local player in the current round.
#[derive(Default, Debug)]
pub(super) struct RoundTracker {
    round: Option<i64>,
    kills: u32,
    first_kill: Option<Instant>,
    last_kill: Option<Instant>,
    /// The round's clip was handed to `due` (or there was nothing to save).
    ended: bool,
    /// Map of the round, for the clip name.
    map: String,
    /// A finished round waiting for the next one to begin before it is saved.
    due: Option<Due>,
    /// The player's kills in the match so far. Counting their increase (instead of the
    /// game's per-round counter) is right however the game times the round change.
    match_kills: Option<u32>,
    /// Kills since the last `take_new_kills`, for timeline marks.
    new_kills: u32,
    /// The game's per-round kill counter as last seen; only used while the game runs with
    /// an older cfg that doesn't send match stats.
    round_kills_seen: u32,
}

#[derive(Debug)]
struct Due {
    /// Save when this passes...
    at: Instant,
    /// ...but never before this.
    not_before: Instant,
    first_kill: Instant,
    kills: u32,
    label: String,
}

impl RoundTracker {
    fn new_round(&mut self, round: Option<i64>) {
        *self = RoundTracker {
            round,
            map: std::mem::take(&mut self.map),
            due: self.due.take(),
            match_kills: self.match_kills,
            new_kills: self.new_kills,
            ..Default::default()
        };
    }

    /// The round is over: its clip waits for the next round to begin.
    fn end_round(&mut self, min_kills: u32, now: Instant) {
        if std::mem::replace(&mut self.ended, true) || self.kills < min_kills {
            return;
        }
        let (Some(first_kill), Some(last_kill)) = (self.first_kill, self.last_kill) else { return };
        let what = if self.kills >= 5 { "Ace".to_string() } else { format!("{}K", self.kills) };
        let not_before = last_kill + AFTER_LAST_KILL;
        self.due = Some(Due { at: (now + NEXT_ROUND_WAIT).max(not_before), not_before, first_kill, kills: self.kills, label: label(&self.map, &what) });
    }

    /// The next round began: save as soon as the last kill is far enough back.
    fn next_round_began(&mut self, now: Instant) {
        if let Some(due) = self.due.as_mut() {
            due.at = due.at.min(due.not_before.max(now));
        }
    }

    fn save_if_due(&mut self, now: Instant) -> Action {
        match self.due.take() {
            Some(due) if now >= due.at => {
                let secs = now.saturating_duration_since(due.first_kill).as_secs() as u32;
                Action::Save { secs, kills: due.kills, label: due.label }
            }
            other => {
                self.due = other;
                Action::Nothing
            }
        }
    }

    pub(super) fn update(&mut self, state: &serde_json::Value, min_kills: u32, now: Instant) -> Action {
        let round = state.pointer("/map/round").and_then(|v| v.as_i64());
        let phase = state.pointer("/round/phase").and_then(|v| v.as_str());
        if let Some(map) = state.pointer("/map/name").and_then(|v| v.as_str()) {
            if round == self.round || self.map.is_empty() {
                self.map = short_name(map, &["de_", "cs_", "ar_"]);
            }
        }
        let round_kills = state.pointer("/player/state/round_kills").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        if round != self.round {
            self.end_round(min_kills, now);
            self.new_round(round);
            // The counter may still hold the last round's kills in this update.
            self.round_kills_seen = round_kills;
        }
        match phase {
            Some("over") => self.end_round(min_kills, now),
            Some("freezetime") => self.next_round_began(now),
            _ => {}
        }

        // Only count kills of the player running the game, not someone being spectated
        // (after dying the game reports the teammate being watched).
        let me = state.pointer("/provider/steamid").and_then(|v| v.as_str());
        let player = state.pointer("/player/steamid").and_then(|v| v.as_str());
        if me.is_some() && me == player {
            let gained = match state.pointer("/player/match_stats/kills").and_then(|v| v.as_u64()).map(|k| k as u32) {
                Some(total) => {
                    // A new match (or a reconnect) starts the count over.
                    let previous = self.match_kills.replace(total).filter(|p| *p <= total);
                    previous.map(|p| total - p).unwrap_or(0)
                }
                None => {
                    let gained = round_kills.saturating_sub(self.round_kills_seen);
                    self.round_kills_seen = self.round_kills_seen.max(round_kills);
                    gained
                }
            };
            if gained > 0 {
                self.new_kills += gained;
                // Kills after the round's end (exit frags) belong to its clip's tail, not
                // to a new streak.
                if !self.ended {
                    self.kills += gained;
                    self.first_kill.get_or_insert(now);
                    self.last_kill = Some(now);
                }
            }
        }
        self.save_if_due(now)
    }

    pub(super) fn take_new_kills(&mut self) -> u32 {
        std::mem::take(&mut self.new_kills)
    }

    /// Called every second: saves a waiting clip, and covers leaving mid-round.
    pub(super) fn poll(&mut self, min_kills: u32, now: Instant) -> Action {
        if self.last_kill.is_some_and(|t| now.saturating_duration_since(t) >= GIVE_UP_AFTER_KILL) {
            self.end_round(min_kills, now);
        }
        self.save_if_due(now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Game state with the player's total kills in the match.
    fn state(round: i64, kills: u64, phase: &str) -> serde_json::Value {
        json!({
            "provider": {"steamid": "765"},
            "map": {"round": round, "name": "de_dust2"},
            "round": {"phase": phase},
            "player": {"steamid": "765", "match_stats": {"kills": kills}},
        })
    }

    fn at(t0: Instant, s: u64) -> Instant {
        t0 + Duration::from_secs(s)
    }

    #[test]
    fn clip_runs_until_the_next_round_begins() {
        let mut t = RoundTracker::default();
        let t0 = Instant::now();
        t.update(&state(3, 4, "live"), 3, t0);
        t.update(&state(3, 5, "live"), 3, at(t0, 1));
        t.update(&state(3, 7, "live"), 3, at(t0, 9));
        // A quiet moment or dying doesn't end the clip.
        assert_eq!(t.poll(3, at(t0, 30)), Action::Nothing);
        // The round ends (the game may bump the round number here already)...
        assert_eq!(t.update(&state(4, 7, "over"), 3, at(t0, 40)), Action::Nothing);
        assert_eq!(t.poll(3, at(t0, 45)), Action::Nothing);
        // ...and the clip is saved when the next round begins.
        assert_eq!(t.update(&state(4, 7, "freezetime"), 3, at(t0, 47)), Action::Save { secs: 46, kills: 3, label: "dust2_3K".into() });
        assert_eq!(t.poll(3, at(t0, 60)), Action::Nothing);
        assert_eq!(t.take_new_kills(), 3);
    }

    #[test]
    fn round_ending_on_the_last_kill_keeps_that_kill_and_exit_frags() {
        let mut t = RoundTracker::default();
        let t0 = Instant::now();
        t.update(&state(7, 0, "live"), 2, t0);
        t.update(&state(7, 1, "live"), 2, at(t0, 1));
        t.update(&state(7, 2, "live"), 2, at(t0, 3));
        t.update(&state(7, 2, "over"), 2, at(t0, 3));
        // An exit frag after the round ended: marked, but no new streak.
        t.update(&state(7, 3, "over"), 2, at(t0, 6));
        assert_eq!(t.take_new_kills(), 3);
        // The next round begins quickly, but the last kill is only 5 s back.
        assert_eq!(t.update(&state(8, 3, "freezetime"), 2, at(t0, 8)), Action::Nothing);
        assert_eq!(t.poll(2, at(t0, 11)), Action::Save { secs: 10, kills: 2, label: "dust2_2K".into() });
    }

    #[test]
    fn older_cfg_without_match_stats_counts_round_kills() {
        let mut t = RoundTracker::default();
        let t0 = Instant::now();
        let rk = |round: i64, kills: u64, phase: &str| {
            json!({
                "provider": {"steamid": "765"},
                "map": {"round": round, "name": "de_dust2"},
                "round": {"phase": phase},
                "player": {"steamid": "765", "state": {"round_kills": kills}},
            })
        };
        t.update(&rk(2, 0, "live"), 2, t0);
        t.update(&rk(2, 2, "live"), 2, at(t0, 2));
        // The round number moves on while the counter still shows the old round.
        t.update(&rk(3, 2, "over"), 2, at(t0, 3));
        assert_eq!(t.take_new_kills(), 2);
        assert_eq!(t.update(&rk(3, 0, "freezetime"), 2, at(t0, 11)), Action::Save { secs: 9, kills: 2, label: "dust2_2K".into() });
        // The reset counter of the new round adds nothing.
        t.update(&rk(3, 0, "live"), 2, at(t0, 20));
        assert_eq!(t.take_new_kills(), 0);
    }

    #[test]
    fn last_round_saves_without_a_next_round_and_spectators_are_ignored() {
        let mut t = RoundTracker::default();
        let t0 = Instant::now();
        t.update(&state(24, 10, "live"), 2, t0);
        t.update(&state(24, 12, "live"), 2, at(t0, 1));
        t.update(&state(24, 12, "over"), 2, at(t0, 5));
        assert_eq!(t.poll(2, at(t0, 14)), Action::Nothing);
        assert_eq!(t.poll(2, at(t0, 15)), Action::Save { secs: 14, kills: 2, label: "dust2_2K".into() });

        let mut spectating = state(25, 30, "live");
        spectating["player"]["steamid"] = json!("999");
        t.update(&spectating, 2, at(t0, 20));
        assert_eq!(t.kills, 0);
    }
}
