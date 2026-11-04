//! The artist's standing with each check: the state the profile saves.
//!
//! Cuts are keyed by project path and CutId because a CutId is unique only
//! inside a project. The path is the empty string until a project is saved.

use std::collections::{BTreeMap, BTreeSet};

use crate::ids::{CutId, DrawingId};

use super::Level;
use super::event::{CheckId, Delivery, Notice};

/// Where a check stands. A map value in the saved profile: never rename.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum CheckState {
    #[default]
    On,
    Off,
    Retired,
    MyStyle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct CheckRecord {
    pub state: CheckState,
    pub streak: u32,
}

/// One cut's fires and times.
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct CutRecord {
    pub fires: BTreeMap<CheckId, u32>,
    /// Already judged for retirement: a cut counts once.
    pub counted: bool,
    /// Keys the key check has fired on, so a reopened project does not repeat.
    pub keys_told: BTreeSet<DrawingId>,
    /// Active seconds per drawing. Durations only, never a clock time.
    pub active: BTreeMap<DrawingId, f64>,
}

fn on() -> bool {
    true
}

/// Everything the profile keeps.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Book {
    #[serde(default)]
    pub level: Level,
    #[serde(default = "on")]
    pub enabled: bool,
    #[serde(default = "on")]
    pub dots: bool,
    #[serde(default)]
    checks: BTreeMap<CheckId, CheckRecord>,
    #[serde(default)]
    cuts: BTreeMap<String, BTreeMap<CutId, CutRecord>>,
    #[serde(default)]
    retired_told: BTreeSet<(Level, CheckId)>,
    #[serde(default)]
    level_told: BTreeSet<Level>,
}

impl Default for Book {
    fn default() -> Self {
        Self {
            level: Level::default(),
            enabled: true,
            dots: true,
            checks: BTreeMap::new(),
            cuts: BTreeMap::new(),
            retired_told: BTreeSet::new(),
            level_told: BTreeSet::new(),
        }
    }
}

impl Book {
    pub fn record(&self, check: CheckId) -> CheckRecord {
        self.checks.get(&check).copied().unwrap_or_default()
    }

    pub fn state(&self, check: CheckId) -> CheckState {
        self.record(check).state
    }

    pub fn streak(&self, check: CheckId) -> u32 {
        self.record(check).streak
    }

    /// How a fire of this check would reach the artist, if at all.
    pub fn delivery(&self, check: CheckId) -> Option<Delivery> {
        if !self.enabled {
            return None;
        }
        match self.state(check) {
            CheckState::On => Some(Delivery::Card),
            CheckState::Retired if self.dots => Some(Delivery::Dot),
            _ => None,
        }
    }

    fn set_state(&mut self, check: CheckId, state: CheckState) {
        self.checks.insert(check, CheckRecord { state, streak: 0 });
    }

    /// That's my style: this check stays off for this artist.
    pub fn my_style(&mut self, check: CheckId) {
        self.set_state(check, CheckState::MyStyle);
    }

    pub fn turn_off(&mut self, check: CheckId) {
        self.set_state(check, CheckState::Off);
    }

    /// Turn a check back on, whatever put it off. Its streak starts again.
    pub fn turn_on(&mut self, check: CheckId) {
        self.set_state(check, CheckState::On);
    }

    /// Change level. Only the artist does this. A retired check comes back,
    /// because the new level asks for more; Off and My style stay as they are.
    pub fn set_level(&mut self, level: Level) {
        if level == self.level {
            return;
        }
        self.level = level;
        for check in CheckId::ALL {
            let state = self.state(check);
            let state = if state == CheckState::Retired { CheckState::On } else { state };
            self.set_state(check, state);
        }
    }

    pub fn cut(&self, project: &str, cut: CutId) -> Option<&CutRecord> {
        self.cuts.get(project)?.get(&cut)
    }

    pub fn fires(&self, project: &str, cut: CutId, check: CheckId) -> u32 {
        self.cut(project, cut)
            .and_then(|c| c.fires.get(&check).copied())
            .unwrap_or(0)
    }

    fn cut_mut(&mut self, project: &str, cut: CutId) -> &mut CutRecord {
        if !self.cuts.contains_key(project) {
            self.cuts.insert(project.to_string(), BTreeMap::new());
        }
        self.cuts
            .get_mut(project)
            .expect("just inserted")
            .entry(cut)
            .or_default()
    }

    pub(super) fn add_fire(&mut self, project: &str, cut: CutId, check: CheckId) {
        *self.cut_mut(project, cut).fires.entry(check).or_insert(0) += 1;
    }

    pub(super) fn add_active(&mut self, project: &str, cut: CutId, id: DrawingId, secs: f64) {
        *self.cut_mut(project, cut).active.entry(id).or_insert(0.0) += secs;
    }

    pub fn active(&self, project: &str, cut: CutId, id: DrawingId) -> f64 {
        self.cut(project, cut)
            .and_then(|c| c.active.get(&id).copied())
            .unwrap_or(0.0)
    }

    /// False if this key was already told on this cut; marks it told.
    pub(super) fn tell_key(&mut self, project: &str, cut: CutId, key: DrawingId) -> bool {
        self.cut_mut(project, cut).keys_told.insert(key)
    }

    /// Forget a document that was never saved.
    pub(super) fn drop_unsaved(&mut self) {
        self.cuts.remove("");
    }

    /// The first save names the project: its counters move to the path.
    pub(super) fn adopt_unsaved(&mut self, path: &str) {
        if let Some(cuts) = self.cuts.remove("") {
            self.cuts.insert(path.to_string(), cuts);
        }
    }

    /// Judge a finished cut. Each cut counts once, however often it is
    /// exported or marked done.
    pub(super) fn finish_cut(&mut self, project: &str, cut: CutId) -> Vec<Notice> {
        let record = self.cut_mut(project, cut);
        if record.counted {
            return Vec::new();
        }
        record.counted = true;
        let fires = record.fires.clone();
        let t = self.level.thresholds();
        let mut notices = Vec::new();
        for check in CheckId::ALL {
            let mut r = self.record(check);
            if r.state != CheckState::On {
                continue;
            }
            if fires.get(&check).copied().unwrap_or(0) > t.allowed_per_cut {
                r.streak = 0;
            } else {
                r.streak += 1;
            }
            if r.streak >= t.retire_after {
                r.state = CheckState::Retired;
                if self.retired_told.insert((self.level, check)) {
                    notices.push(Notice::Retired { check, shots: r.streak });
                }
            }
            self.checks.insert(check, r);
        }
        if let Some(next) = self.level.next() {
            if self.all_retired() && self.level_told.insert(self.level) {
                notices.push(Notice::NextLevel { next });
            }
        }
        notices
    }

    /// Every check the artist left in play is retired. A check they switched
    /// off or called their style does not hold the next level back.
    fn all_retired(&self) -> bool {
        let states = CheckId::ALL.map(|c| self.state(c));
        states.contains(&CheckState::Retired) && !states.contains(&CheckState::On)
    }
}
