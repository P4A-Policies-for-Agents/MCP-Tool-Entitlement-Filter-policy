// Copyright 2026 Salesforce, Inc. All rights reserved.
//! Pure CDGC helpers (no PDK imports) — the JWT nonce and the cached
//! field-sensitivity types. The HTTP fetch lives in lib.rs (needs the HttpClient).

use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use crate::entitlement::GovernedField;

/// Cached, parsed per-field sensitivity map for one asset, plus its governed
/// identity (so the policy can self-describe the response — one CDGC fetch does both).
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct CachedFieldMap {
    pub fields: Vec<GovernedField>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub external_id: Option<String>,
    /// Unix seconds when fetched — drives the refresh TTL.
    pub timestamp: i64,
}

/// Single-initiator refresh lock entry (stampede control).
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct RefreshLock {
    pub acquired_at: i64,
}

/// Per-request JWT nonce: nanoseconds since the Unix epoch as a decimal string.
pub fn nonce_from_time(now: SystemTime) -> String {
    now.duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_nanos().to_string())
        .unwrap_or_else(|_| "0".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nonce_is_decimal() {
        let n = nonce_from_time(SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1));
        assert_eq!(n, "1000000000");
    }
}
