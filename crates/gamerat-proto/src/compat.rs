//! ratbagd API compatibility classifier.
//!
//! Pure logic — no D-Bus, no async — so callers (CLI, GUI, tests) can
//! decide how to surface the result. Probe `Manager.APIVersion` once
//! at startup, pass the integer into [`classify`], then render the
//! [`Compat`] variant however makes sense for that surface.
//!
//! ## Why warn instead of block
//!
//! ratbagd's `APIVersion` historically increments slowly: changes are
//! usually additive (new properties / methods), and even when a method
//! signature shifts, our hand-written proxy in `gamerat-ratbag` only
//! exercises a small subset. Refusing to talk to an unknown version
//! would lock users out of new ratbagd releases that probably still
//! work; refusing to talk to an older one would lock distros to our
//! exact pin. So we warn and keep going.

/// API version gamerat was developed and tested against. Bump when we
/// re-validate against a newer ratbagd release.
pub const RATBAGD_API_VERSION_EXPECTED: i32 = 2;

/// Minimum API version we still expect to talk to. The proxies we
/// hand-rolled in `gamerat-ratbag` haven't relied on anything that
/// landed after this version.
pub const RATBAGD_API_VERSION_MIN: i32 = 1;

/// Result of classifying a probed `APIVersion` against our pins.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Compat {
    /// Exact match to what we validated against. Best case.
    Exact,
    /// Within `[MIN, EXPECTED]` but not exact. Likely fine; we have
    /// real users hitting this combination.
    KnownCompat { actual: i32 },
    /// Below our supported floor. Operations will likely fail; warn
    /// loudly but keep trying — older ratbagd may still respond to
    /// the subset we use.
    BelowMin { actual: i32, min: i32 },
    /// Newer than anything we've tested. Probably still works, but
    /// the user has stepped outside the validated envelope.
    AboveKnown { actual: i32, expected: i32 },
}

impl Compat {
    /// True when this variant warrants surfacing to the user.
    #[must_use]
    pub const fn is_warning(&self) -> bool {
        !matches!(self, Self::Exact)
    }
}

/// Classify a live `APIVersion` reading.
#[must_use]
pub const fn classify(actual: i32) -> Compat {
    if actual == RATBAGD_API_VERSION_EXPECTED {
        Compat::Exact
    } else if actual < RATBAGD_API_VERSION_MIN {
        Compat::BelowMin {
            actual,
            min: RATBAGD_API_VERSION_MIN,
        }
    } else if actual > RATBAGD_API_VERSION_EXPECTED {
        Compat::AboveKnown {
            actual,
            expected: RATBAGD_API_VERSION_EXPECTED,
        }
    } else {
        Compat::KnownCompat { actual }
    }
}

/// Human-readable warning string for a non-exact [`Compat`]. Returns
/// `None` for [`Compat::Exact`] so callers can `if let Some(msg)`
/// without an extra branch.
#[must_use]
pub fn warning(compat: Compat) -> Option<String> {
    match compat {
        Compat::Exact => None,
        Compat::KnownCompat { actual } => Some(format!(
            "ratbagd APIVersion={actual} is within our supported range \
             (expected {RATBAGD_API_VERSION_EXPECTED}). Should work; \
             file a bug if you see anomalies."
        )),
        Compat::BelowMin { actual, min } => Some(format!(
            "ratbagd APIVersion={actual} is below the minimum we've \
             validated ({min}). gamerat may misbehave — consider \
             upgrading libratbag."
        )),
        Compat::AboveKnown { actual, expected } => Some(format!(
            "ratbagd APIVersion={actual} is newer than the version \
             gamerat was tested against ({expected}). This usually \
             works, but the combination hasn't been explicitly \
             validated — bugs may occur."
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_match_is_no_warning() {
        let c = classify(RATBAGD_API_VERSION_EXPECTED);
        assert_eq!(c, Compat::Exact);
        assert!(!c.is_warning());
        assert!(warning(c).is_none());
    }

    #[test]
    fn known_compat_in_range() {
        // Bracket: ratbagd 0.16 era was APIVersion=1.
        let c = classify(1);
        assert_eq!(c, Compat::KnownCompat { actual: 1 });
        assert!(c.is_warning());
        let msg = warning(c).expect("known-compat must surface a warning");
        assert!(msg.contains("within"));
    }

    #[test]
    fn below_min_warns() {
        let c = classify(0);
        assert!(matches!(c, Compat::BelowMin { actual: 0, min: 1 }));
        let msg = warning(c).expect("below-min must warn");
        assert!(msg.contains("below"));
    }

    #[test]
    fn above_known_warns() {
        let c = classify(RATBAGD_API_VERSION_EXPECTED + 1);
        assert!(matches!(c, Compat::AboveKnown { .. }));
        let msg = warning(c).expect("above-known must warn");
        assert!(msg.contains("newer"));
    }

    #[test]
    fn min_le_expected_invariant() {
        // Sanity: we'd reclassify everything if this got broken.
        const { assert!(RATBAGD_API_VERSION_MIN <= RATBAGD_API_VERSION_EXPECTED) };
    }
}
