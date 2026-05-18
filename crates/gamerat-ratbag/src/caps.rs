//! libratbag capability constants + helpers.
//!
//! Cap values come from `enum ratbag_resolution_capability` in
//! libratbag's `src/libratbag-enums.h`. ratbagd surfaces them through
//! the `Capabilities` property on the `Profile`, `Resolution`, and
//! `Button` interfaces; the values are wire-stable so we hard-code
//! the small set we care about here rather than re-deriving them
//! from the C headers at build time.

/// `RATBAG_RESOLUTION_CAP_SEPARATE_XY_RESOLUTION`. Resolution accepts
/// `(uu)` X/Y pairs in addition to single-axis `u`.
pub const RESOLUTION_CAP_SEPARATE_XY: u32 = 1;

/// `RATBAG_RESOLUTION_CAP_DISABLE`.
///
/// Resolution can be hardware-disabled via `IsDisabled = true`; firmware
/// will then skip the slot when the DPI-cycle / DPI-up / DPI-down button
/// is pressed. Not every driver declares this — Logitech HID++ 2.0
/// onboard profiles do, but older drivers may not.
pub const RESOLUTION_CAP_DISABLE: u32 = 2;

/// Plan per-slot `IsDisabled` writes for a profile application.
///
/// Given the `Capabilities` array of every resolution on a profile and
/// how many DPI stages the gamerat profile wants to materialise,
/// produce a per-slot write plan. Returned vector matches
/// `caps_per_slot` in length and order:
///
/// - `Some(true)`  → write `IsDisabled = true`  (slot is beyond the
///   profile's stage count, firmware should skip it in the cycle).
/// - `Some(false)` → write `IsDisabled = false` (slot is in-range; clear
///   any stale disable from a previously-smaller profile).
/// - `None`        → driver doesn't claim the disable cap on this slot;
///   leave it alone, the write would fail with `ENOTSUP`.
///
/// Pure function so the gating logic can be unit-tested without a
/// running ratbagd. The actual proxy writes live in
/// [`crate::Device::apply_profile_complete`].
///
/// # Future quality-of-life
///
/// A "manual per-stage" toggle (let the user disable e.g. stage 1 while
/// keeping 0 and 2) is a natural extension once the wire format grows
/// a per-stage `enabled` bit. Out of scope for now — shortening the
/// profile to N stages and letting the firmware skip the rest covers
/// the common pain point.
#[must_use]
pub fn plan_resolution_disable(
    caps_per_slot: &[Vec<u32>],
    stage_count: usize,
) -> Vec<Option<bool>> {
    caps_per_slot
        .iter()
        .enumerate()
        .map(|(idx, caps)| {
            if caps.contains(&RESOLUTION_CAP_DISABLE) {
                Some(idx >= stage_count)
            } else {
                None
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disables_extras_when_every_slot_supports_disable() {
        // 5-slot device, profile with 1 stage → slot 0 enabled, 1..4 disabled.
        let caps = vec![vec![1, 2], vec![2], vec![2], vec![2], vec![2]];
        let plan = plan_resolution_disable(&caps, 1);
        assert_eq!(
            plan,
            vec![Some(false), Some(true), Some(true), Some(true), Some(true),]
        );
    }

    #[test]
    fn enables_all_when_stage_count_matches_slot_count() {
        let caps = vec![vec![2], vec![2], vec![2]];
        let plan = plan_resolution_disable(&caps, 3);
        assert_eq!(plan, vec![Some(false), Some(false), Some(false)]);
    }

    #[test]
    fn three_stages_on_five_slot_device() {
        let caps = vec![vec![2], vec![2], vec![2], vec![2], vec![2]];
        let plan = plan_resolution_disable(&caps, 3);
        assert_eq!(
            plan,
            vec![
                Some(false),
                Some(false),
                Some(false),
                Some(true),
                Some(true),
            ]
        );
    }

    #[test]
    fn skips_slots_without_disable_cap() {
        // SEPARATE_XY only, no DISABLE → leave the slot untouched.
        let caps = vec![vec![1], vec![1], vec![1]];
        let plan = plan_resolution_disable(&caps, 1);
        assert_eq!(plan, vec![None, None, None]);
    }

    #[test]
    fn mixed_caps_per_slot() {
        // Hypothetical driver where only some slots claim the cap.
        // Planner reports None for the slots without it; the rest are
        // gated normally.
        let caps = vec![vec![2], vec![], vec![2], vec![1], vec![2]];
        let plan = plan_resolution_disable(&caps, 2);
        assert_eq!(plan, vec![Some(false), None, Some(true), None, Some(true)]);
    }

    #[test]
    fn empty_device_yields_empty_plan() {
        let plan = plan_resolution_disable(&[], 0);
        assert!(plan.is_empty());
    }

    #[test]
    fn stage_count_exceeds_slot_count_is_clamped_implicitly() {
        // Asking for 7 stages on a 3-slot device: every slot < 7, so
        // every supported slot ends up enabled. apply_profile_complete
        // already drops extra stages via `take(stages_to_write)`.
        let caps = vec![vec![2], vec![2], vec![2]];
        let plan = plan_resolution_disable(&caps, 7);
        assert_eq!(plan, vec![Some(false), Some(false), Some(false)]);
    }
}
