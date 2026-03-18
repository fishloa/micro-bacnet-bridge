//! Processor pipeline for BACnet point value transformation.
//!
//! # Overview
//!
//! Each discovered BACnet point may carry a [`PointRule`] that controls how its
//! value is transformed before being forwarded (dashboard, BACnet/IP, MQTT, REST API).
//! The processors come from the [`crate::config::Convertor`] referenced by the rule's
//! `convertor_id` field.
//!
//! The pipeline is ordered: processors are applied left-to-right for the forward
//! direction (BACnet → display) and right-to-left for the reverse direction
//! (display/write → BACnet).
//!
//! # Processor semantics
//!
//! | Processor      | Forward                                 | Reverse                        |
//! |----------------|-----------------------------------------|--------------------------------|
//! | `SetUnit(u)`   | No-op (unit is metadata only)           | No-op                          |
//! | `Scale{f,o}`   | `display = raw × f + o`                 | `raw = (display − o) ÷ f`      |
//! | `MapStates(v)` | `Enumerated(n)/UnsignedInt(n)` → label  | label → `Enumerated(index+1)`  |
//!
//! # Modes
//!
//! - [`PointMode::Ignore`] — suppress the point entirely; `process_value` returns `Null`.
//! - [`PointMode::Passthrough`] — forward the raw value unchanged.
//! - [`PointMode::Processed`] — apply the full processor chain supplied by the convertor.

use crate::bacnet::BacnetValue;
use crate::config::{PointMode, Processor};
use heapless::String;

// Re-export for use in tests
#[cfg(test)]
use heapless::Vec as HVec;

// ---------------------------------------------------------------------------
// is_active
// ---------------------------------------------------------------------------

/// Return `true` if the point is active (i.e. not ignored).
///
/// A point is active when its mode is [`PointMode::Passthrough`] or
/// [`PointMode::Processed`]. Ignored points should be suppressed from all
/// output channels.
pub fn is_active(mode: &PointMode) -> bool {
    !matches!(mode, PointMode::Ignore)
}

// ---------------------------------------------------------------------------
// process_value
// ---------------------------------------------------------------------------

/// Apply the processor pipeline to a raw BACnet value (forward direction).
///
/// - If `mode` is [`PointMode::Ignore`], returns [`BacnetValue::Null`] regardless of
///   the processors.
/// - If `mode` is [`PointMode::Passthrough`], returns `value` unchanged.
/// - If `mode` is [`PointMode::Processed`], applies each [`Processor`] in order.
///   The `processors` slice comes from the [`crate::config::Convertor`] referenced
///   by the point's rule.
///
/// Only the last `MapStates` processor in the chain affects state-text resolution.
/// A `Scale` processor after a `MapStates` would receive the `CharString` output of
/// `MapStates` and pass it through unchanged (scale only applies to numeric types).
pub fn process_value(
    value: &BacnetValue,
    mode: &PointMode,
    processors: &[Processor],
) -> BacnetValue {
    match mode {
        PointMode::Ignore => BacnetValue::Null,
        PointMode::Passthrough => value.clone(),
        PointMode::Processed => {
            let mut current = value.clone();
            for proc in processors {
                current = apply_processor(current, proc);
            }
            current
        }
    }
}

/// Apply a single processor step to a value.
fn apply_processor(value: BacnetValue, proc: &Processor) -> BacnetValue {
    match proc {
        Processor::SetUnit(_) => {
            // Unit is metadata only — no value change.
            value
        }
        Processor::Scale { factor, offset } => apply_scale(value, *factor, *offset),
        Processor::MapStates(labels) => apply_map_states(value, labels),
    }
}

/// Apply `value × factor + offset` to numeric types.
///
/// - `Real(f)` → `Real(f * factor + offset)`
/// - `SignedInt(n)` → `Real(n as f32 * factor + offset)`
/// - `UnsignedInt(n)` → `Real(n as f32 * factor + offset)`
/// - `Enumerated(n)` → `Real(n as f32 * factor + offset)`
/// - All other types → pass-through unchanged.
fn apply_scale(value: BacnetValue, factor: f32, offset: f32) -> BacnetValue {
    match value {
        BacnetValue::Real(f) => BacnetValue::Real(f * factor + offset),
        BacnetValue::SignedInt(n) => BacnetValue::Real(n as f32 * factor + offset),
        BacnetValue::UnsignedInt(n) => BacnetValue::Real(n as f32 * factor + offset),
        BacnetValue::Enumerated(n) => BacnetValue::Real(n as f32 * factor + offset),
        // Non-numeric types pass through.
        other => other,
    }
}

/// Look up a multi-state value in a label table (1-based index).
///
/// - `Enumerated(n)` or `UnsignedInt(n)` where `n >= 1` and `labels[n-1]` exists
///   → `CharString(label)`.
/// - `Enumerated(n)` or `UnsignedInt(n)` where the index is out of range
///   → unchanged (return `Enumerated`/`UnsignedInt` as-is).
/// - All other types → pass-through unchanged.
fn apply_map_states(value: BacnetValue, labels: &heapless::Vec<String<12>, 8>) -> BacnetValue {
    let index = match &value {
        BacnetValue::Enumerated(n) if *n >= 1 => Some((*n as usize) - 1),
        BacnetValue::UnsignedInt(n) if *n >= 1 => Some((*n as usize) - 1),
        _ => None,
    };
    if let Some(idx) = index {
        if let Some(label) = labels.get(idx) {
            let mut s = String::<64>::new();
            for ch in label.chars() {
                let _ = s.push(ch);
            }
            return BacnetValue::CharString(s);
        }
    }
    value
}

// ---------------------------------------------------------------------------
// reverse_value
// ---------------------------------------------------------------------------

/// Reverse the processor pipeline (display/write string → raw BACnet value).
///
/// Processors are applied in **reverse order** (right-to-left):
/// - `SetUnit` → no-op.
/// - `Scale { factor, offset }` → `raw = (parsed - offset) / factor`.
/// - `MapStates(labels)` → look up `display_value` in labels; if found return
///   `Enumerated(index + 1)`.
///
/// Resolution order (only one reversal step is needed per processor invocation):
/// 1. Try `MapStates` reversal first (state name takes priority over numeric parse).
/// 2. Try boolean string keywords (`"true"`, `"false"`, `"Active"`, etc.).
/// 3. Parse as `f32` and apply inverse scale.
///
/// Returns `None` if the string cannot be reversed by any processor in the chain
/// and does not parse as a boolean or number.
pub fn reverse_value(display_value: &str, processors: &[Processor]) -> Option<BacnetValue> {
    // Walk processors in reverse order, looking for a step that can consume the string.
    for proc in processors.iter().rev() {
        match proc {
            Processor::SetUnit(_) => continue,
            Processor::MapStates(labels) => {
                // Try label lookup first.
                for (idx, label) in labels.iter().enumerate() {
                    if label.as_str() == display_value {
                        return Some(BacnetValue::Enumerated(idx as u32 + 1));
                    }
                }
                // Label not found — keep walking (outer processors may handle it).
            }
            Processor::Scale { factor, offset } => {
                // Try boolean keywords first.
                if let Some(b) = parse_boolean(display_value) {
                    return Some(BacnetValue::Boolean(b));
                }
                // Try numeric reverse.
                if let Ok(parsed) = display_value.parse::<f32>() {
                    // Special case: "1" → Boolean(true), "0" → Boolean(false)
                    if display_value == "1" {
                        return Some(BacnetValue::Boolean(true));
                    }
                    if display_value == "0" {
                        return Some(BacnetValue::Boolean(false));
                    }
                    let raw = if *factor == 0.0 {
                        parsed
                    } else {
                        (parsed - offset) / factor
                    };
                    return Some(BacnetValue::Real(raw));
                }
                // Cannot parse — keep walking.
            }
        }
    }

    // No processor handled it. Fall back to generic parse.
    if let Some(b) = parse_boolean(display_value) {
        return Some(BacnetValue::Boolean(b));
    }
    if display_value == "1" {
        return Some(BacnetValue::Boolean(true));
    }
    if display_value == "0" {
        return Some(BacnetValue::Boolean(false));
    }
    if let Ok(parsed) = display_value.parse::<f32>() {
        return Some(BacnetValue::Real(parsed));
    }

    None
}

/// Parse a boolean keyword. Returns `Some(true/false)` or `None`.
fn parse_boolean(s: &str) -> Option<bool> {
    match s {
        "true" | "Active" | "ON" => Some(true),
        "false" | "Inactive" | "OFF" => Some(false),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{PointMode, Processor};
    use heapless::{String, Vec};

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    fn labels(strs: &[&str]) -> HVec<String<12>, 8> {
        let mut v: HVec<String<12>, 8> = HVec::new();
        for &s in strs {
            let mut hs = String::<12>::new();
            let _ = hs.push_str(s);
            let _ = v.push(hs);
        }
        v
    }

    fn scale_proc(factor: f32, offset: f32) -> Processor {
        Processor::Scale { factor, offset }
    }

    fn set_unit_proc(unit: u16) -> Processor {
        Processor::SetUnit(unit)
    }

    fn map_states_proc(strs: &[&str]) -> Processor {
        Processor::MapStates(labels(strs))
    }

    // -----------------------------------------------------------------------
    // is_active
    // -----------------------------------------------------------------------

    #[test]
    fn is_active_ignore_false() {
        assert!(!is_active(&PointMode::Ignore));
    }

    #[test]
    fn is_active_passthrough_true() {
        assert!(is_active(&PointMode::Passthrough));
    }

    #[test]
    fn is_active_processed_true() {
        assert!(is_active(&PointMode::Processed));
    }

    // -----------------------------------------------------------------------
    // process_value — Ignore mode
    // -----------------------------------------------------------------------

    #[test]
    fn process_value_ignore_returns_null() {
        let result = process_value(&BacnetValue::Real(42.0), &PointMode::Ignore, &[]);
        assert_eq!(result, BacnetValue::Null);
    }

    #[test]
    fn process_value_ignore_with_processors_still_null() {
        let procs = [scale_proc(2.0, 0.0)];
        let result = process_value(&BacnetValue::Real(10.0), &PointMode::Ignore, &procs);
        assert_eq!(result, BacnetValue::Null);
    }

    // -----------------------------------------------------------------------
    // process_value — Passthrough mode
    // -----------------------------------------------------------------------

    #[test]
    fn process_value_passthrough_real() {
        let input = BacnetValue::Real(99.5);
        let result = process_value(&input, &PointMode::Passthrough, &[]);
        assert_eq!(result, BacnetValue::Real(99.5));
    }

    #[test]
    fn process_value_passthrough_ignores_processors() {
        // Even if processors are present, Passthrough skips them.
        let procs = [scale_proc(100.0, 0.0)];
        let result = process_value(&BacnetValue::Real(1.0), &PointMode::Passthrough, &procs);
        assert_eq!(result, BacnetValue::Real(1.0));
    }

    #[test]
    fn process_value_passthrough_boolean() {
        assert_eq!(
            process_value(&BacnetValue::Boolean(true), &PointMode::Passthrough, &[]),
            BacnetValue::Boolean(true)
        );
    }

    // -----------------------------------------------------------------------
    // process_value — SetUnit processor
    // -----------------------------------------------------------------------

    #[test]
    fn process_value_set_unit_no_value_change() {
        let procs = [set_unit_proc(0)]; // DegreesCelsius
        let result = process_value(&BacnetValue::Real(25.0), &PointMode::Processed, &procs);
        assert_eq!(result, BacnetValue::Real(25.0));
    }

    // -----------------------------------------------------------------------
    // process_value — Scale processor
    // -----------------------------------------------------------------------

    #[test]
    fn process_value_scale_real() {
        let procs = [scale_proc(2.0, 10.0)];
        // 5 * 2 + 10 = 20
        let result = process_value(&BacnetValue::Real(5.0), &PointMode::Processed, &procs);
        assert_eq!(result, BacnetValue::Real(20.0));
    }

    #[test]
    fn process_value_scale_signed_int() {
        let procs = [scale_proc(0.1, -40.0)];
        // SignedInt(500) * 0.1 + (-40) = 50 - 40 = 10.0
        let result = process_value(&BacnetValue::SignedInt(500), &PointMode::Processed, &procs);
        assert_eq!(result, BacnetValue::Real(10.0));
    }

    #[test]
    fn process_value_scale_unsigned_int() {
        let procs = [scale_proc(1.0, 0.0)];
        let result = process_value(&BacnetValue::UnsignedInt(42), &PointMode::Processed, &procs);
        assert_eq!(result, BacnetValue::Real(42.0));
    }

    #[test]
    fn process_value_scale_enumerated() {
        let procs = [scale_proc(1.0, 0.0)];
        let result = process_value(&BacnetValue::Enumerated(3), &PointMode::Processed, &procs);
        assert_eq!(result, BacnetValue::Real(3.0));
    }

    #[test]
    fn process_value_scale_boolean_passthrough() {
        // Scale does not affect Boolean.
        let procs = [scale_proc(100.0, 50.0)];
        let result = process_value(&BacnetValue::Boolean(true), &PointMode::Processed, &procs);
        assert_eq!(result, BacnetValue::Boolean(true));
    }

    #[test]
    fn process_value_scale_charstring_passthrough() {
        let procs = [scale_proc(2.0, 1.0)];
        let mut s = String::<64>::new();
        let _ = s.push_str("hello");
        let result = process_value(
            &BacnetValue::CharString(s.clone()),
            &PointMode::Processed,
            &procs,
        );
        assert_eq!(result, BacnetValue::CharString(s));
    }

    #[test]
    fn process_value_scale_identity() {
        let procs = [scale_proc(1.0, 0.0)];
        let result = process_value(&BacnetValue::Real(7.7), &PointMode::Processed, &procs);
        assert!((result.as_real().unwrap() - 7.7).abs() < 1e-5);
    }

    // -----------------------------------------------------------------------
    // process_value — MapStates processor
    // -----------------------------------------------------------------------

    #[test]
    fn process_value_map_states_enumerated() {
        let procs = [map_states_proc(&["Off", "Heat", "Cool", "Auto"])];
        // Enumerated(2) → "Heat" (1-based)
        let result = process_value(&BacnetValue::Enumerated(2), &PointMode::Processed, &procs);
        let mut expected = String::<64>::new();
        let _ = expected.push_str("Heat");
        assert_eq!(result, BacnetValue::CharString(expected));
    }

    #[test]
    fn process_value_map_states_unsigned_int() {
        let procs = [map_states_proc(&["Manual", "Auto", "Override"])];
        // UnsignedInt(1) → "Manual"
        let result = process_value(&BacnetValue::UnsignedInt(1), &PointMode::Processed, &procs);
        let mut expected = String::<64>::new();
        let _ = expected.push_str("Manual");
        assert_eq!(result, BacnetValue::CharString(expected));
    }

    #[test]
    fn process_value_map_states_zero_passthrough() {
        // State 0 is below the 1-based range → value unchanged.
        let procs = [map_states_proc(&["Off", "On"])];
        let result = process_value(&BacnetValue::UnsignedInt(0), &PointMode::Processed, &procs);
        assert_eq!(result, BacnetValue::UnsignedInt(0));
    }

    #[test]
    fn process_value_map_states_out_of_range() {
        let procs = [map_states_proc(&["Off", "On"])];
        // State 5, only 2 labels → unchanged
        let result = process_value(&BacnetValue::Enumerated(5), &PointMode::Processed, &procs);
        assert_eq!(result, BacnetValue::Enumerated(5));
    }

    #[test]
    fn process_value_map_states_boolean_passthrough() {
        let procs = [map_states_proc(&["A", "B"])];
        let result = process_value(&BacnetValue::Boolean(false), &PointMode::Processed, &procs);
        assert_eq!(result, BacnetValue::Boolean(false));
    }

    // -----------------------------------------------------------------------
    // process_value — chained processors
    // -----------------------------------------------------------------------

    #[test]
    fn process_value_set_unit_then_scale() {
        // SetUnit should not affect value; Scale should.
        let procs = [set_unit_proc(0), scale_proc(10.0, 0.0)];
        let result = process_value(&BacnetValue::Real(3.0), &PointMode::Processed, &procs);
        assert_eq!(result, BacnetValue::Real(30.0));
    }

    #[test]
    fn process_value_scale_then_scale() {
        // Two consecutive Scale steps: first ×2 then +100.
        let procs = [scale_proc(2.0, 0.0), scale_proc(1.0, 100.0)];
        let result = process_value(&BacnetValue::Real(5.0), &PointMode::Processed, &procs);
        assert_eq!(result, BacnetValue::Real(110.0));
    }

    #[test]
    fn process_value_set_unit_only() {
        // SetUnit is a no-op for the value.
        let procs = [set_unit_proc(95)];
        let result = process_value(&BacnetValue::Real(-1.5), &PointMode::Processed, &procs);
        assert_eq!(result, BacnetValue::Real(-1.5));
    }

    // -----------------------------------------------------------------------
    // reverse_value
    // -----------------------------------------------------------------------

    #[test]
    fn reverse_value_empty_processors_numeric() {
        let result = reverse_value("42.0", &[]);
        assert_eq!(result, Some(BacnetValue::Real(42.0)));
    }

    #[test]
    fn reverse_value_empty_processors_boolean_true() {
        assert_eq!(reverse_value("true", &[]), Some(BacnetValue::Boolean(true)));
        assert_eq!(
            reverse_value("Active", &[]),
            Some(BacnetValue::Boolean(true))
        );
        assert_eq!(reverse_value("ON", &[]), Some(BacnetValue::Boolean(true)));
    }

    #[test]
    fn reverse_value_empty_processors_boolean_false() {
        assert_eq!(
            reverse_value("false", &[]),
            Some(BacnetValue::Boolean(false))
        );
        assert_eq!(
            reverse_value("Inactive", &[]),
            Some(BacnetValue::Boolean(false))
        );
        assert_eq!(reverse_value("OFF", &[]), Some(BacnetValue::Boolean(false)));
    }

    #[test]
    fn reverse_value_empty_processors_none() {
        assert_eq!(reverse_value("not-a-value", &[]), None);
        assert_eq!(reverse_value("banana", &[]), None);
    }

    #[test]
    fn reverse_value_scale_reversal() {
        // Forward: display = raw * 2 + 10.  Reverse: raw = (display - 10) / 2.
        let procs = [scale_proc(2.0, 10.0)];
        // display "20" → raw (20 - 10) / 2 = 5
        match reverse_value("20", &procs) {
            Some(BacnetValue::Real(v)) => assert!((v - 5.0).abs() < 1e-4),
            other => panic!("expected Real(5.0), got {:?}", other),
        }
    }

    #[test]
    fn reverse_value_scale_negative_offset() {
        // Forward: display = raw * 1 + (-40).  Reverse: raw = display + 40.
        let procs = [scale_proc(1.0, -40.0)];
        match reverse_value("10", &procs) {
            Some(BacnetValue::Real(v)) => assert!((v - 50.0).abs() < 1e-4),
            other => panic!("expected Real(50.0), got {:?}", other),
        }
    }

    #[test]
    fn reverse_value_scale_zero_factor_no_divide_by_zero() {
        let procs = [scale_proc(0.0, 10.0)];
        // When factor = 0, reverse returns the parsed value unchanged.
        match reverse_value("42", &procs) {
            Some(BacnetValue::Real(v)) => assert!((v - 42.0).abs() < 1e-4),
            other => panic!("expected Real(42.0), got {:?}", other),
        }
    }

    #[test]
    fn reverse_value_map_states() {
        let procs = [map_states_proc(&["Off", "Heat", "Cool", "Auto"])];
        // "Heat" is index 1 → Enumerated(2)
        assert_eq!(
            reverse_value("Heat", &procs),
            Some(BacnetValue::Enumerated(2))
        );
        assert_eq!(
            reverse_value("Off", &procs),
            Some(BacnetValue::Enumerated(1))
        );
        assert_eq!(
            reverse_value("Auto", &procs),
            Some(BacnetValue::Enumerated(4))
        );
    }

    #[test]
    fn reverse_value_map_states_not_found_falls_through_to_numeric() {
        let procs = [map_states_proc(&["Off", "On"])];
        // "99" is not in labels, falls through to numeric parse.
        match reverse_value("99", &procs) {
            Some(BacnetValue::Real(v)) => assert!((v - 99.0).abs() < 1e-4),
            other => panic!("expected Real(99.0), got {:?}", other),
        }
    }

    #[test]
    fn reverse_value_map_states_case_sensitive() {
        let procs = [map_states_proc(&["Off", "Heat"])];
        // "heat" (lowercase) must NOT match "Heat".
        // Falls through to boolean/numeric; "heat" doesn't parse → None.
        assert_eq!(reverse_value("heat", &procs), None);
    }

    #[test]
    fn reverse_value_set_unit_is_noop() {
        let procs = [set_unit_proc(0), scale_proc(1.0, 0.0)];
        match reverse_value("77", &procs) {
            Some(BacnetValue::Real(v)) => assert!((v - 77.0).abs() < 1e-4),
            other => panic!("expected Real(77.0), got {:?}", other),
        }
    }

    #[test]
    fn reverse_value_one_and_zero_are_boolean() {
        let procs: [Processor; 0] = [];
        assert_eq!(reverse_value("1", &procs), Some(BacnetValue::Boolean(true)));
        assert_eq!(
            reverse_value("0", &procs),
            Some(BacnetValue::Boolean(false))
        );
    }

    // -----------------------------------------------------------------------
    // Roundtrip: process_value → reverse_value
    // -----------------------------------------------------------------------

    #[test]
    fn roundtrip_scale() {
        let procs_vec: Vec<Processor, 4> = {
            let mut v = Vec::new();
            let _ = v.push(scale_proc(0.5, -5.0));
            v
        };
        let procs: &[Processor] = &procs_vec;

        // Forward: raw 7.5, scale 0.5, offset -5 → display -1.25
        let display = process_value(&BacnetValue::Real(7.5), &PointMode::Processed, procs);
        assert_eq!(display, BacnetValue::Real(-1.25));

        // Reverse: "-1.25" → raw (−1.25 + 5) / 0.5 = 3.75 / 0.5 = 7.5
        match reverse_value("-1.25", procs) {
            Some(BacnetValue::Real(v)) => assert!((v - 7.5).abs() < 1e-4, "got {}", v),
            other => panic!("expected Real(7.5), got {:?}", other),
        }
    }

    #[test]
    fn roundtrip_map_states_all_labels() {
        let state_labels = ["Off", "Heat", "Cool", "Auto"];
        let procs_vec: Vec<Processor, 4> = {
            let mut v = Vec::new();
            let _ = v.push(map_states_proc(&state_labels));
            v
        };
        let procs: &[Processor] = &procs_vec;

        for (i, &label) in state_labels.iter().enumerate() {
            let state = (i + 1) as u32;
            // Forward
            let display = process_value(
                &BacnetValue::Enumerated(state),
                &PointMode::Processed,
                procs,
            );
            let mut expected_str = String::<64>::new();
            let _ = expected_str.push_str(label);
            assert_eq!(display, BacnetValue::CharString(expected_str));
            // Reverse
            assert_eq!(
                reverse_value(label, procs),
                Some(BacnetValue::Enumerated(state))
            );
        }
    }

    #[test]
    fn roundtrip_set_unit_then_scale() {
        let procs_vec: Vec<Processor, 4> = {
            let mut v = Vec::new();
            let _ = v.push(set_unit_proc(0)); // DegreesCelsius metadata
            let _ = v.push(scale_proc(0.1, -40.0));
            v
        };
        let procs: &[Processor] = &procs_vec;

        // Forward: UnsignedInt(900) × 0.1 + (−40) = 90 − 40 = 50.0
        let display = process_value(&BacnetValue::UnsignedInt(900), &PointMode::Processed, procs);
        assert_eq!(display, BacnetValue::Real(50.0));

        // Reverse: (50 − (−40)) / 0.1 = 90 / 0.1 = 900
        match reverse_value("50", procs) {
            Some(BacnetValue::Real(v)) => assert!((v - 900.0).abs() < 0.1, "got {}", v),
            other => panic!("expected Real(900.0), got {:?}", other),
        }
    }
}

// ---------------------------------------------------------------------------
// BacnetValue helper (used in tests)
// ---------------------------------------------------------------------------

impl BacnetValue {
    /// Return the `f32` if this is a `Real` variant, or `None`.
    pub fn as_real(&self) -> Option<f32> {
        match self {
            BacnetValue::Real(f) => Some(*f),
            _ => None,
        }
    }
}
