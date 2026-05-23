//! Pure logic for detecting unbalanced press/release pairs in a macro.
//!
//! Lives in the proto crate so both the daemon (exposing it over D-Bus
//! as `CheckMacroBalance`) and any future in-Rust caller share one
//! implementation. No I/O — Miri-clean, exercised by `scripts/miri.sh`
//! via the `gamerat-proto` test suite.
//!
//! The check is structural: walks the step list maintaining a per-
//! keycode counter (+1 on `KEY_PRESS`, −1 on `KEY_RELEASE`). At the end
//! any positive counter means the macro leaves a key held when the
//! button is released, and any negative counter means the macro
//! releases a key it never pressed (the OS treats this as a no-op but
//! it's still likely an authoring mistake worth surfacing).

use crate::types::{MacroStep, macro_event_kind};

/// Result of analyzing a macro for unbalanced events.
///
/// Both fields preserve the order in which the offending keycode was
/// first seen, so callers can format diagnostics deterministically.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MacroBalance {
    /// Keycodes still pressed after the last step. A non-empty list
    /// means the macro leaves the OS seeing those keys as held.
    pub stuck_keys: Vec<u32>,
    /// Keycodes released without a matching press earlier in the
    /// macro. Harmless at the OS level (kernel ignores releases for
    /// unheld keys) but usually an authoring mistake.
    pub dangling_releases: Vec<u32>,
}

/// Analyze a macro step list for unbalanced press/release pairs.
///
/// Steps with kinds other than `KEY_PRESS` / `KEY_RELEASE` (i.e. `WAIT`
/// and `NONE`) are ignored — they don't affect the OS key state.
#[must_use]
pub fn macro_balance(steps: &[MacroStep]) -> MacroBalance {
    // Vec of `(keycode, count)` instead of a HashMap so the result is
    // ordered by first appearance without an extra sort, and to keep
    // the function alloc-light for the common case (a handful of
    // distinct keycodes per macro).
    let mut counts: Vec<(u32, i32)> = Vec::new();

    for step in steps {
        let delta = match step.kind {
            macro_event_kind::KEY_PRESS => 1,
            macro_event_kind::KEY_RELEASE => -1,
            _ => continue,
        };
        if let Some(entry) = counts.iter_mut().find(|(k, _)| *k == step.value) {
            entry.1 += delta;
        } else {
            counts.push((step.value, delta));
        }
    }

    let mut balance = MacroBalance::default();
    for (keycode, count) in counts {
        if count > 0 {
            balance.stuck_keys.push(keycode);
        } else if count < 0 {
            balance.dangling_releases.push(keycode);
        }
    }
    balance
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(keycode: u32) -> MacroStep {
        MacroStep {
            kind: macro_event_kind::KEY_PRESS,
            value: keycode,
        }
    }

    fn release(keycode: u32) -> MacroStep {
        MacroStep {
            kind: macro_event_kind::KEY_RELEASE,
            value: keycode,
        }
    }

    fn wait(ms: u32) -> MacroStep {
        MacroStep {
            kind: macro_event_kind::WAIT,
            value: ms,
        }
    }

    #[test]
    fn empty_macro_is_balanced() {
        assert_eq!(macro_balance(&[]), MacroBalance::default());
    }

    #[test]
    fn balanced_press_release_pair() {
        let steps = [press(30), wait(25), release(30)];
        assert_eq!(macro_balance(&steps), MacroBalance::default());
    }

    #[test]
    fn single_stuck_key() {
        let steps = [press(30)];
        assert_eq!(
            macro_balance(&steps),
            MacroBalance {
                stuck_keys: vec![30],
                dangling_releases: vec![],
            }
        );
    }

    #[test]
    fn multiple_stuck_keys_preserve_order() {
        let steps = [press(30), press(56), press(29)];
        assert_eq!(
            macro_balance(&steps),
            MacroBalance {
                stuck_keys: vec![30, 56, 29],
                dangling_releases: vec![],
            }
        );
    }

    #[test]
    fn nested_same_key_balances() {
        // Press A, press A again, release A — net count = +1 ⇒ stuck.
        let steps = [press(30), press(30), release(30)];
        assert_eq!(
            macro_balance(&steps),
            MacroBalance {
                stuck_keys: vec![30],
                dangling_releases: vec![],
            }
        );
    }

    #[test]
    fn dangling_release_only() {
        let steps = [release(30)];
        assert_eq!(
            macro_balance(&steps),
            MacroBalance {
                stuck_keys: vec![],
                dangling_releases: vec![30],
            }
        );
    }

    #[test]
    fn mixed_stuck_and_dangling() {
        // A: pressed twice, released once → stuck.
        // B: released without press → dangling.
        // C: balanced press/release → clean.
        let steps = [
            press(30),
            press(30),
            release(30),
            release(48),
            press(46),
            release(46),
        ];
        assert_eq!(
            macro_balance(&steps),
            MacroBalance {
                stuck_keys: vec![30],
                dangling_releases: vec![48],
            }
        );
    }

    #[test]
    fn wait_and_none_steps_are_ignored() {
        let steps = [
            wait(100),
            MacroStep {
                kind: macro_event_kind::NONE,
                value: 999,
            },
            press(30),
            wait(50),
            release(30),
        ];
        assert_eq!(macro_balance(&steps), MacroBalance::default());
    }
}
