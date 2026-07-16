//! Small shared helpers for the runner/engine layer.
//!
//! Step execution moved to the graph engine ([`crate::core::graph`]) in Phase 2;
//! what remains here is the duration formatter shared across the UI.

use std::time::Duration;

/// Format a duration compactly, e.g. `4.2s` or `320ms`.
pub fn fmt_duration(d: Duration) -> String {
    let secs = d.as_secs_f64();
    if secs >= 1.0 {
        format!("{secs:.1}s")
    } else {
        format!("{}ms", d.as_millis())
    }
}
