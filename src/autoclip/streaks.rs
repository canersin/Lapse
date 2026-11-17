//! Kill-streak games (Dota 2): a clip for multi-kills (double kill, triple kill, ultra kill, rampage).
//!
//! Dota counts kills as one multi-kill while each comes within 18 seconds of the
//! previous one. The clip is saved once the streak is over: 18 s without a kill or the
//! hero dies. If the streak was already saved early (quiet moment) and grows later,
//! the bigger streak is saved again, so a rampage is never cut short.

use std::time::{Duration, Instant};

use super::{label, short_name, Action};

/// Dota's multi-kill window between two kills.
const STREAK_WINDOW: Duration = Duration::from_secs(18);
/// Save after this long without a kill; a later kill inside the window extends the streak.
const QUIET_AFTER_KILL: Duration = Duration::from_secs(10);

#[derive(Default, Debug)]
pub(super) struct StreakTracker {
    match_id: Option<String>,
    /// Total kills of the last update; None until the first update of a match.
    kills: Option<u32>,
    streak: u32,
    /// Streak size that was already saved.
    saved: u32,
    first_kill: Option<Instant>,
    last_kill: Option<Instant>,
    hero: String,
    /// Kills since the last `take_new_kills`, for timeline marks.
    new_kills: u32,
}

impl StreakTracker {
    fn finish(&mut self, min_kills: u32, now: Instant) -> Action {
        if self.streak < min_kills || self.streak <= self.saved {
            return Action::Nothing;
        }
        let Some(first) = self.first_kill else { return Action::Nothing };
        self.saved = self.streak;
        let secs = now.saturating_duration_since(first).as_secs();
        let what = match self.streak {
            2 => "Double_Kill",
            3 => "Triple_Kill",
            4 => "Ultra_Kill",
            _ => "Rampage",
        };
        Action::Save { secs: secs as u32, kills: self.streak, label: label(&self.hero, what) }
    }

    fn end_streak(&mut self) {
        self.streak = 0;
        self.saved = 0;
        self.first_kill = None;
        self.last_kill = None;
    }

    pub(super) fn take_new_kills(&mut self) -> u32 {
        std::mem::take(&mut self.new_kills)
    }

    pub(super) fn update(&mut self, state: &serde_json::Value, min_kills: u32, now: Instant) -> Action {
        let match_id = state.pointer("/map/matchid").and_then(|v| v.as_str()).map(String::from);
        if match_id != self.match_id {
            *self = StreakTracker { match_id, ..Default::default() };
        }
        if let Some(hero) = state.pointer("/hero/name").and_then(|v| v.as_str()) {
            self.hero = short_name(hero, &["npc_dota_hero_"]);
        }
        // Spectators and the main menu have no own kill count.
        let Some(kills) = state.pointer("/player/kills").and_then(|v| v.as_u64()).map(|k| k as u32) else {
            return Action::Nothing;
        };
        let previous = self.kills.replace(kills);
        let mut action = Action::Nothing;

        match previous {
            Some(prev) if kills > prev => {
                let new_kills = kills - prev;
                self.new_kills += new_kills;
                let in_window = self.last_kill.map(|l| now.saturating_duration_since(l) <= STREAK_WINDOW).unwrap_or(false);
                if !in_window {
                    action = self.finish(min_kills, now);
                    self.end_streak();
                    self.first_kill = Some(now);
                }
                self.streak += new_kills;
                self.last_kill = Some(now);
            }
            // First update of the match (kills already counted) or a reconnect: no streak.
            _ => {}
        }

        let dead = state.pointer("/hero/alive").and_then(|v| v.as_bool()) == Some(false);
        if action == Action::Nothing && dead {
            action = self.finish(min_kills, now);
            self.end_streak();
        }
        action
    }

    /// Called every second: saves after a quiet moment and closes streaks that ran out.
    pub(super) fn poll(&mut self, min_kills: u32, now: Instant) -> Action {
        let Some(last) = self.last_kill else { return Action::Nothing };
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn state(kills: u64, alive: bool) -> serde_json::Value {
        json!({
            "map": {"matchid": "8123"},
            "player": {"kills": kills},
            "hero": {"alive": alive, "name": "npc_dota_hero_axe"},
        })
    }

    fn at(t0: Instant, s: u64) -> Instant {
        t0 + Duration::from_secs(s)
    }

    #[test]
    fn triple_kill_then_rampage_saves_the_bigger_streak() {
        let mut t = StreakTracker::default();
        let t0 = Instant::now();
        assert_eq!(t.update(&state(4, true), 3, t0), Action::Nothing); // joined with 4 kills
        t.update(&state(5, true), 3, at(t0, 10));
        t.update(&state(6, true), 3, at(t0, 15));
        t.update(&state(7, true), 3, at(t0, 20));
        // Quiet for 10 s: the triple kill is saved from its first kill.
        assert_eq!(t.poll(3, at(t0, 30)), Action::Save { secs: 20, kills: 3, label: "axe_Triple_Kill".into() });
        assert_eq!(t.poll(3, at(t0, 31)), Action::Nothing);
        // Two more kills inside the 18 s window turn it into a rampage, saved again.
        t.update(&state(8, true), 3, at(t0, 34));
        t.update(&state(9, true), 3, at(t0, 36));
        assert_eq!(t.update(&state(9, false), 3, at(t0, 40)), Action::Save { secs: 30, kills: 5, label: "axe_Rampage".into() });
    }

    #[test]
    fn slow_kills_are_not_a_streak() {
        let mut t = StreakTracker::default();
        let t0 = Instant::now();
        t.update(&state(0, true), 2, t0);
        t.update(&state(1, true), 2, at(t0, 5));
        t.update(&state(2, true), 2, at(t0, 30));
        assert_eq!(t.poll(2, at(t0, 60)), Action::Nothing);
        // Spectating (no own kills) changes nothing.
        assert_eq!(t.update(&json!({"map": {"matchid": "8123"}}), 2, at(t0, 61)), Action::Nothing);
    }
}
