// openchoice-license — subscription gate for the Open Choice desktop app and oc-cli.
//
// One public entry point: `check()` runs the policy state machine and returns
// a `Status`. `try_bypass(code)` verifies an emergency-override code against
// the pre-shipped weekly table and extends grace by 7 days on success.
//
// See PLAN.md in open-choice-heartbeat for the full design.

pub mod bypass;
pub mod bypass_table;
pub mod derive;
pub mod fetch;
pub mod gate;
pub mod salt;
pub mod state;

pub use bypass::{try_bypass, BypassError};
pub use fetch::FetchError;
pub use gate::{check, Status};
pub use state::{LicenseState, StateError};

/// Message surfaced in the blocking modal / CLI error.
pub const BLOCKED_USER_MESSAGE: &str = "Open Choice couldn't verify your subscription. \
Check your internet connection and try again. If you're still blocked after reconnecting, \
contact Trevor for an emergency code.";
