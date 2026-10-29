use contexere::pipeline::CollapseMode;
use contexere::pipeline::collapse::collapse;

#[test]
fn collapse_does_not_panic_on_box_drawing() {
    // Box drawing chars are 3 bytes each in UTF-8
    let input = "│━┌└ ".repeat(30) + "\n";
    let input = input.repeat(20);
    let result = collapse(&input, &CollapseMode::Generic);
    assert!(!result.collapsed_lines.is_empty());
}

#[test]
fn collapse_does_not_panic_on_emoji() {
    let input = "✗ build failed ⚠ warning ▶ running\n".repeat(25);
    let result = collapse(&input, &CollapseMode::Build);
    assert!(!result.collapsed_lines.is_empty());
}

#[test]
fn collapse_does_not_panic_on_spinner_frames() {
    // Braille spinner frames: ⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏ (3 bytes each)
    let input = (0..60)
        .map(|i| format!("⠋ Loading step {}...\n", i))
        .collect::<String>();
    let result = collapse(&input, &CollapseMode::Generic);
    assert!(!result.collapsed_lines.is_empty());
}

#[test]
fn collapse_does_not_panic_on_cjk() {
    let input = "テスト失敗: モジュール初期化エラー\n".repeat(25);
    let result = collapse(&input, &CollapseMode::Test);
    assert!(!result.collapsed_lines.is_empty());
}

#[test]
fn safe_truncate_never_panics_on_multibyte() {
    use contexere::util::text::safe_truncate;
    let s = "│━┌└⠋⠙✗⚠▶─".repeat(10);
    for n in 0..=s.len() {
        let mut string_buffer = s.clone();
        safe_truncate(&mut string_buffer, n);
    }
}

#[test]
fn safe_truncate_with_ellipsis_never_panics_on_multibyte() {
    use contexere::util::text::safe_truncate_with_ellipsis;
    let original = "✗ error: cannot find type `Foo`\n".repeat(5);
    for n in 0..=original.len() {
        let res = safe_truncate_with_ellipsis(&original, n); // must not panic
        assert!(res.len() <= n + 3); // +3 for potential char boundary snap
    }
}
