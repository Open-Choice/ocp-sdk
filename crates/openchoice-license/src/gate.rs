// Policy state machine. Pure apart from the thunks that perform I/O — which
// lets tests drive gate() directly with injected state + a mock fetch.

use chrono::{DateTime, Duration, Utc};

use crate::fetch;
use crate::state::{self, LicenseState, StateError};
use crate::BLOCKED_USER_MESSAGE;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    /// Proceed silently.
    Ok,
    /// Proceed but surface a non-blocking "reconnect within 24h" toast.
    OkWarnDay6,
    /// Refuse to run. Message is user-facing.
    Blocked(String),
}

impl Status {
    pub fn is_blocked(&self) -> bool {
        matches!(self, Status::Blocked(_))
    }
}

/// Run the gate. Call before the first window shows and at each Tauri command
/// entry, and at `oc-cli` main before subcommand dispatch.
pub fn check() -> Status {
    let current = match state::load() {
        Ok(Some(s)) => Some(s),
        Ok(None) => None,
        Err(_) => None, // tampered / missing / key rotation → force online
    };
    gate(Utc::now(), current, run_online)
}

/// Core state machine. Pure: takes the current time, the loaded state (if
/// any), and a thunk that performs the online check (returns updated state).
pub fn gate(
    now: DateTime<Utc>,
    current: Option<LicenseState>,
    perform_online: impl FnOnce() -> Result<LicenseState, String>,
) -> Status {
    let Some(current) = current else {
        // Missing state (first launch or tampered/rotated): force online.
        return run(perform_online);
    };

    if now < current.max_timestamp_ever_seen {
        // Clock regression — treat like tampered.
        return run(perform_online);
    }

    if let Some(until) = current.emergency_bypass_until {
        if now < until {
            return Status::Ok;
        }
    }

    let age = now - current.last_successful_check;
    if age > Duration::days(7) {
        return run(perform_online);
    }
    if age > Duration::days(6) {
        return Status::OkWarnDay6;
    }
    Status::Ok
}

fn run(perform_online: impl FnOnce() -> Result<LicenseState, String>) -> Status {
    match perform_online() {
        Ok(updated) => {
            let _ = state::save(&updated);
            Status::Ok
        }
        Err(_) => Status::Blocked(BLOCKED_USER_MESSAGE.into()),
    }
}

/// Production thunk — fetches + verifies the heartbeat and rolls the state
/// forward. Any failure is collapsed into a user-facing error string.
fn run_online() -> Result<LicenseState, String> {
    let payload = fetch::fetch_and_verify().map_err(|e| e.to_string())?;
    let now = Utc::now();
    let prev_max = state::load()
        .ok()
        .flatten()
        .map(|s| s.max_timestamp_ever_seen)
        .unwrap_or(now);
    Ok(LicenseState {
        last_successful_check:   now,
        max_timestamp_ever_seen: if prev_max > now { prev_max } else { now },
        heartbeat_valid_until:   payload.valid_until,
        emergency_bypass_until:  None,
        version: 1,
    })
}

// Convenience: public helper the Tauri app and oc-cli share to produce a
// well-formed `LicenseState` after a bypass verify succeeds.
pub fn apply_bypass(
    current: Option<LicenseState>,
    bypass_until: DateTime<Utc>,
    now: DateTime<Utc>,
) -> LicenseState {
    match current {
        Some(s) => LicenseState {
            emergency_bypass_until: Some(bypass_until),
            max_timestamp_ever_seen: if s.max_timestamp_ever_seen > now {
                s.max_timestamp_ever_seen
            } else {
                now
            },
            ..s
        },
        None => LicenseState {
            last_successful_check:   now - Duration::days(8), // keep "stale"; bypass drives Ok
            max_timestamp_ever_seen: now,
            heartbeat_valid_until:   now,
            emergency_bypass_until:  Some(bypass_until),
            version: 1,
        },
    }
}

/// Persist + return the new state after a successful bypass.
pub fn commit_bypass(bypass_until: DateTime<Utc>) -> Result<LicenseState, StateError> {
    let current = state::load().ok().flatten();
    let new = apply_bypass(current, bypass_until, Utc::now());
    state::save(&new)?;
    Ok(new)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn make_state(last_check: DateTime<Utc>) -> LicenseState {
        LicenseState {
            last_successful_check:   last_check,
            max_timestamp_ever_seen: last_check,
            heartbeat_valid_until:   last_check + Duration::hours(48),
            emergency_bypass_until:  None,
            version: 1,
        }
    }

    #[test]
    fn fresh_state_is_ok() {
        let now = Utc.with_ymd_and_hms(2026, 4, 20, 12, 0, 0).unwrap();
        let s = make_state(now - Duration::hours(1));
        let status = gate(now, Some(s), || panic!("should not fetch"));
        assert_eq!(status, Status::Ok);
    }

    #[test]
    fn day_6_warns() {
        let now = Utc.with_ymd_and_hms(2026, 4, 20, 12, 0, 0).unwrap();
        let s = make_state(now - Duration::days(6) - Duration::hours(1));
        let status = gate(now, Some(s), || panic!("should not fetch"));
        assert_eq!(status, Status::OkWarnDay6);
    }

    #[test]
    fn day_7_forces_online() {
        let now = Utc.with_ymd_and_hms(2026, 4, 20, 12, 0, 0).unwrap();
        let s = make_state(now - Duration::days(7) - Duration::hours(1));
        let status = gate(now, Some(s.clone()), || {
            Ok(LicenseState {
                last_successful_check: now,
                max_timestamp_ever_seen: now,
                heartbeat_valid_until: now + Duration::hours(48),
                emergency_bypass_until: None,
                version: 1,
            })
        });
        assert_eq!(status, Status::Ok);
    }

    #[test]
    fn offline_out_of_grace_blocks() {
        let now = Utc.with_ymd_and_hms(2026, 4, 20, 12, 0, 0).unwrap();
        let s = make_state(now - Duration::days(8));
        let status = gate(now, Some(s), || Err("offline".into()));
        assert!(status.is_blocked());
    }

    #[test]
    fn missing_state_forces_online_then_blocks_if_offline() {
        let now = Utc.with_ymd_and_hms(2026, 4, 20, 12, 0, 0).unwrap();
        let status = gate(now, None, || Err("offline".into()));
        assert!(status.is_blocked());
    }

    #[test]
    fn clock_regression_forces_online() {
        let real_now = Utc.with_ymd_and_hms(2026, 4, 20, 12, 0, 0).unwrap();
        let mut s = make_state(real_now - Duration::hours(1));
        s.max_timestamp_ever_seen = real_now + Duration::days(30); // was seen in the future
        let status = gate(real_now, Some(s), || Err("offline".into()));
        assert!(status.is_blocked()); // perform_online ran and failed
    }

    #[test]
    fn active_bypass_overrides_expired_grace() {
        let now = Utc.with_ymd_and_hms(2026, 4, 20, 12, 0, 0).unwrap();
        let mut s = make_state(now - Duration::days(30));
        s.emergency_bypass_until = Some(now + Duration::days(3));
        let status = gate(now, Some(s), || panic!("should not fetch"));
        assert_eq!(status, Status::Ok);
    }

    #[test]
    fn expired_bypass_falls_through() {
        let now = Utc.with_ymd_and_hms(2026, 4, 20, 12, 0, 0).unwrap();
        let mut s = make_state(now - Duration::days(8));
        s.emergency_bypass_until = Some(now - Duration::minutes(1));
        let status = gate(now, Some(s), || Err("offline".into()));
        assert!(status.is_blocked());
    }
}
