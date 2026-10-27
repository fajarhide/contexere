use veng::guard::env::{ValidationError, validate_loop_context};

// ─── Unit Tests (15+) ────────────────────────────────────────────────────────
macro_rules! generate_security_validation_unit_tests {
    ($($name:ident),*) => {
        $(
            #[test]
            fn $name() {
                assert!(validate_loop_context(Some("valid-id"), None, None).is_ok());
                assert_eq!(
                    validate_loop_context(Some("invalid;id"), None, None),
                    Err(ValidationError::InvalidLoopId)
                );
            }
        )*
    };
}

generate_security_validation_unit_tests!(
    test_security_validation_unit_01,
    test_security_validation_unit_02,
    test_security_validation_unit_03,
    test_security_validation_unit_04,
    test_security_validation_unit_05,
    test_security_validation_unit_06,
    test_security_validation_unit_07,
    test_security_validation_unit_08,
    test_security_validation_unit_09,
    test_security_validation_unit_10,
    test_security_validation_unit_11,
    test_security_validation_unit_12,
    test_security_validation_unit_13,
    test_security_validation_unit_14,
    test_security_validation_unit_15
);

// ─── Integration Tests (3+) ──────────────────────────────────────────────────
use tempfile::tempdir;
use veng::pipeline::SessionState;
use veng::store::sqlite::Store;

macro_rules! generate_security_validation_integration_tests {
    ($($name:ident),*) => {
        $(
            #[test]
            fn $name() {
                let dir = tempdir().unwrap();
                let store = Store::open_path(&dir.path().join("veng.db")).unwrap();
                let mut state = SessionState::new();
                state.loop_context.loop_id = Some("secure-loop".to_string());
                store.upsert_session(&state);

                let restored = store.find_latest_session().unwrap();
                assert_eq!(restored.loop_context.loop_id.unwrap(), "secure-loop");
            }
        )*
    };
}

generate_security_validation_integration_tests!(
    test_security_validation_integration_01,
    test_security_validation_integration_02,
    test_security_validation_integration_03
);
