//! Encode / decode `Button.Mapping` between ratbagd's tagged-variant
//! wire shape and our flat [`gamerat_proto::ButtonAction`].
//!
//! ratbagd exposes the property as `(uv)`:
//!
//!   1. `u` — the action kind (`RATBAG_BUTTON_ACTION_TYPE_*`).
//!   2. `v` — a variant whose inner type depends on `kind`:
//!      * `NONE`(0)    → `u` (zero, ignored)
//!      * `MOUSE`(1)   → `u` (target hardware button)
//!      * `SPECIAL`(2) → `u` (one of `(1<<30)+N` special-action values)
//!      * `KEY`(3)     → `u` (Linux keycode)
//!      * `MACRO`(4)   → `a(uu)` (sequence of `(macro_event_kind, value)`)
//!
//! We can't lean on zbus's automatic `Type` derive for this — the
//! variant payload changes shape per kind. The two pair functions
//! here own the marshalling so callers see a flat `ButtonAction`.
//!
//! There's one quirk worth knowing: when *writing* the property, zbus
//! marshals a bare `Value::U32(n)` as `u<n>` rather than `v<u<n>>`.
//! ratbagd expects the variant-of-variant shape `v<u<n>>`, so we
//! wrap inner values in `Value::Value(Box::new(...))` — same trick
//! as the resolution-DPI writer in `client.rs`. See
//! `memory/zbus_variant_property_write.md` for the longer story.

use gamerat_proto::{ButtonAction, MacroStep, button_action_kind};
use zbus::zvariant::{Array, OwnedValue, Structure, Value};

use crate::error::{Error, Result};

/// Flatten ratbagd's `(uv)` Mapping into a [`ButtonAction`].
///
/// Unknown action kinds round-trip as `NONE` rather than producing an
/// error — we'd rather render "unsupported binding" in the UI than
/// crash the daemon on a single weird mouse.
pub fn decode_mapping(value: &OwnedValue) -> Result<ButtonAction> {
    let structure = value
        .downcast_ref::<Structure<'_>>()
        .map_err(|_| Error::ratbagd_op("Button.Mapping is not a struct"))?;
    let fields = structure.fields();
    if fields.len() != 2 {
        return Err(Error::ratbagd_op(
            "Button.Mapping struct must have 2 fields",
        ));
    }
    let kind = fields[0]
        .downcast_ref::<u32>()
        .map_err(|_| Error::ratbagd_op("Button.Mapping[0] is not u32"))?;
    let payload = &fields[1];

    match kind {
        button_action_kind::MOUSE => Ok(ButtonAction::mouse(decode_inner_u32(payload)?)),
        button_action_kind::SPECIAL => Ok(ButtonAction::special(decode_inner_u32(payload)?)),
        button_action_kind::KEY => Ok(ButtonAction::key(decode_inner_u32(payload)?)),
        button_action_kind::MACRO => {
            let steps = decode_macro_steps(payload)?;
            Ok(ButtonAction::macro_action(steps))
        }
        // NONE and any future/unrecognised kind: round-trip as
        // ButtonAction::none(). libratbag treats unknown kinds as
        // disabled, so this matches its behaviour.
        _ => Ok(ButtonAction::none()),
    }
}

/// Build the `(uv)` value ratbagd's `Button.Mapping` setter expects.
///
/// Two subtleties worth knowing for next time:
///
/// 1. The first field is passed as a bare `u32` — wrapping it in
///    `Value::U32(...)` before handing it to `Structure::from` makes
///    zvariant infer the field's type as `Value` (signature `v`)
///    instead of `u`, producing the wrong `(vv)` wire shape and
///    triggering ratbagd's "Incorrect parameters for property
///    'Mapping', expected '(uv)', got '(vv)'" `InvalidArgs`.
/// 2. The variant payload is passed as a plain `Value` (e.g.
///    `Value::U32(n)` for scalar bindings, or a `Value::Array` for
///    the macro form). zvariant automatically serializes a
///    Value-typed struct field as `v<inner>` — wrapping it again in
///    `Value::Value(Box::new(...))` adds a SECOND variant layer,
///    yielding `(u v<v<u>>)` on the wire. ratbagd's Mapping reader
///    expects `(u v<u>)`, and the extra layer makes its
///    `sd_bus_message_read(m, "v", "u", &map)` return -ENXIO. The
///    standalone property-write rule (one-`Value` wrap, see the
///    `zbus_variant_property_write` memory) does NOT apply inside a
///    Structure.
pub fn encode_mapping(action: &ButtonAction) -> Value<'static> {
    let inner: Value<'static> = match action.kind {
        button_action_kind::MACRO => {
            let pairs: Vec<(u32, u32)> = action
                .macro_steps
                .iter()
                .map(|s| (s.kind, s.value))
                .collect();
            Value::new(pairs)
        }
        // NONE / MOUSE / SPECIAL / KEY: u32 payload. NONE conventionally
        // ships 0 — libratbag treats anything but the recognised kinds
        // identically and zeroes the binding either way.
        _ => Value::U32(action.value),
    };

    Value::from(Structure::from((action.kind, inner)))
}

fn decode_inner_u32(payload: &Value<'_>) -> Result<u32> {
    // `payload` is the variant-typed second field of (uv). zbus
    // represents it as `Value::Value(Box<Value>)` after parsing —
    // peel one layer, then downcast to u32.
    let inner: &Value<'_> = match payload {
        Value::Value(boxed) => boxed.as_ref(),
        // ratbagd should always wrap; tolerate a non-wrapped u32 in
        // case some legacy version skips the variant box.
        other => other,
    };
    inner
        .downcast_ref::<u32>()
        .map_err(|_| Error::ratbagd_op("Button.Mapping inner is not u32"))
}

fn decode_macro_steps(payload: &Value<'_>) -> Result<Vec<MacroStep>> {
    let inner: &Value<'_> = match payload {
        Value::Value(boxed) => boxed.as_ref(),
        other => other,
    };
    let array = inner
        .downcast_ref::<Array<'_>>()
        .map_err(|_| Error::ratbagd_op("Button.Mapping macro is not array"))?;

    // zvariant 5's Array doesn't impl Vec::try_from<&Array> for our
    // tuple type, so walk it element-by-element and pull out each
    // (kind, value) struct manually.
    let mut steps = Vec::new();
    for item in array.iter() {
        let structure = item
            .downcast_ref::<Structure<'_>>()
            .map_err(|_| Error::ratbagd_op("macro event is not (uu)"))?;
        let fields = structure.fields();
        if fields.len() != 2 {
            return Err(Error::ratbagd_op("macro event must have 2 fields"));
        }
        let kind = fields[0]
            .downcast_ref::<u32>()
            .map_err(|_| Error::ratbagd_op("macro event kind is not u32"))?;
        let value = fields[1]
            .downcast_ref::<u32>()
            .map_err(|_| Error::ratbagd_op("macro event value is not u32"))?;
        steps.push(MacroStep { kind, value });
    }
    Ok(steps)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gamerat_proto::{button_special, macro_event_kind};

    fn pack(action: &ButtonAction) -> OwnedValue {
        let v = encode_mapping(action);
        OwnedValue::try_from(v).expect("encoded value must own")
    }

    #[test]
    fn encoded_signature_is_uv() {
        // ratbagd's Button.Mapping property is `(uv)`. zbus serializes
        // Structure fields by their declared Rust type — passing a
        // `Value::U32` as the first field silently produces `(vv)`
        // instead. This test pins the wire shape so that regression
        // can't sneak back in.
        for action in [
            ButtonAction::none(),
            ButtonAction::mouse(1),
            ButtonAction::key(30),
            ButtonAction::special(button_special::WHEEL_DOWN),
            ButtonAction::macro_action(vec![MacroStep {
                kind: macro_event_kind::KEY_PRESS,
                value: 30,
            }]),
        ] {
            let v = encode_mapping(&action);
            assert_eq!(
                v.value_signature().to_string(),
                "(uv)",
                "wrong wire sig for {action:?}",
            );
        }
    }

    /// Pins the *inner* variant layer count to exactly one. The
    /// previous implementation wrapped the payload in
    /// `Value::Value(Box::new(...))` while ALSO placing it as a
    /// Value-typed Structure field, which zvariant double-wraps to
    /// `(u v<v<u>>)`. ratbagd's `sd_bus_message_read(m, "v", "u",
    /// &map)` rejects that with `-ENXIO`, surfaced as the
    /// `System.Error.ENXIO: No such device or address` error in
    /// `Device.Set`. This regression test asserts that the second
    /// struct field's inner value is the bare scalar (or array),
    /// not another `Value::Value`.
    #[test]
    fn encoded_variant_has_no_extra_wrap() {
        let owned = pack(&ButtonAction::mouse(5));
        let structure = owned
            .downcast_ref::<Structure<'_>>()
            .expect("encoded form is a struct");
        let fields = structure.fields();
        assert_eq!(fields.len(), 2);
        let inner = match &fields[1] {
            Value::Value(boxed) => boxed.as_ref(),
            other => other,
        };
        assert!(
            !matches!(inner, Value::Value(_)),
            "second struct field should be `v<u>`, not `v<v<u>>` — \
             got an extra Value::Value wrap: {inner:?}"
        );
        let n = inner.downcast_ref::<u32>().expect("inner is u32");
        assert_eq!(n, 5);
    }

    #[test]
    fn round_trip_none() {
        let owned = pack(&ButtonAction::none());
        let back = decode_mapping(&owned).expect("decode");
        // NONE round-trips as NONE; value is opaque (libratbag treats
        // payload as ignored).
        assert_eq!(back.kind, button_action_kind::NONE);
        assert!(back.macro_steps.is_empty());
    }

    #[test]
    fn round_trip_mouse() {
        let owned = pack(&ButtonAction::mouse(5));
        let back = decode_mapping(&owned).expect("decode");
        assert_eq!(back.kind, button_action_kind::MOUSE);
        assert_eq!(back.value, 5);
    }

    #[test]
    fn round_trip_special() {
        let owned = pack(&ButtonAction::special(button_special::WHEEL_DOWN));
        let back = decode_mapping(&owned).expect("decode");
        assert_eq!(back.kind, button_action_kind::SPECIAL);
        assert_eq!(back.value, button_special::WHEEL_DOWN);
    }

    #[test]
    fn round_trip_key() {
        let owned = pack(&ButtonAction::key(30));
        let back = decode_mapping(&owned).expect("decode");
        assert_eq!(back.kind, button_action_kind::KEY);
        assert_eq!(back.value, 30);
    }

    #[test]
    fn round_trip_macro() {
        let steps = vec![
            MacroStep {
                kind: macro_event_kind::KEY_PRESS,
                value: 30,
            },
            MacroStep {
                kind: macro_event_kind::WAIT,
                value: 25,
            },
            MacroStep {
                kind: macro_event_kind::KEY_RELEASE,
                value: 30,
            },
        ];
        let owned = pack(&ButtonAction::macro_action(steps.clone()));
        let back = decode_mapping(&owned).expect("decode");
        assert_eq!(back.kind, button_action_kind::MACRO);
        assert_eq!(back.macro_steps, steps);
    }

    #[test]
    fn unknown_kind_decodes_to_none() {
        // Synthesize a mapping with a future / unrecognised action kind.
        let v = Value::from(Structure::from((
            Value::U32(99),
            Value::Value(Box::new(Value::U32(0))),
        )));
        let owned = OwnedValue::try_from(v).expect("owned");
        let back = decode_mapping(&owned).expect("decode");
        assert_eq!(back.kind, button_action_kind::NONE);
    }
}
