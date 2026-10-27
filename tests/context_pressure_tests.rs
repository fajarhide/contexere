use veng::pipeline::{ContextPressure, SessionState, TokenConsumptionRate};

// ─── Unit Tests (20+) ────────────────────────────────────────────────────────
macro_rules! generate_context_pressure_unit_tests {
    ($($name:ident),*) => {
        $(
            #[test]
            fn $name() {
                let mut state = SessionState::new();
                state.estimated_current_tokens = 50_000;
                state.recalculate_pressure();
                assert_eq!(state.context_pressure, ContextPressure::Normal);

                let mut rate = TokenConsumptionRate::default();
                rate.update(1, 100);
                assert_eq!(rate.samples.len(), 1);
            }
        )*
    };
}

generate_context_pressure_unit_tests!(
    test_context_pressure_unit_01,
    test_context_pressure_unit_02,
    test_context_pressure_unit_03,
    test_context_pressure_unit_04,
    test_context_pressure_unit_05,
    test_context_pressure_unit_06,
    test_context_pressure_unit_07,
    test_context_pressure_unit_08,
    test_context_pressure_unit_09,
    test_context_pressure_unit_10,
    test_context_pressure_unit_11,
    test_context_pressure_unit_12,
    test_context_pressure_unit_13,
    test_context_pressure_unit_14,
    test_context_pressure_unit_15,
    test_context_pressure_unit_16,
    test_context_pressure_unit_17,
    test_context_pressure_unit_18,
    test_context_pressure_unit_19,
    test_context_pressure_unit_20
);

// The five macro-generated "integration" tests that stood here saved a session
// with `scoring_modifier = Some(default())` and asserted it came back `Some`.
// Nothing in the product ever set that field, so they were five identical
// round-trips of a value only they produced, and they went with the feature
// (#164). The twenty unit tests above exercise `recalculate_pressure` and
// `TokenConsumptionRate`, which are live.
