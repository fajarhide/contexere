.PHONY: all fmt clippy test security binary-check ci clean

# The default target runs the full CI pipeline
all: ci

fmt:
	@echo "=== Running Rustfmt ==="
	cargo fmt --check

clippy:
	@echo "=== Running Clippy ==="
	cargo clippy --all-targets -- -D warnings

test:
	@echo "=== Running Tests ==="
	# One run, not two. `cargo insta test` is `cargo test` with insta's assertion
	# mode changed, so running it after `cargo test --all` re-executed the whole
	# suite and reported what the first run had already decided: 11s of the CI job
	# and 44s to 55s of a warm local one. `--check` is the stricter of the two
	# modes, since it refuses to write a `.snap.new` and fails instead, so this
	# tightens the gate while halving its runs. Update snapshots the documented
	# way, with `cargo insta review`.
	#
	# `--all` is insta's own flag, not cargo's. Behind a `--` it reaches libtest
	# instead, which answers `Unrecognized option: 'all'` once per target and
	# fails the phase.
	cargo insta test --check --all

security:
	@echo "=== Running Security Audit ==="
	cargo audit
	@echo "=== Checking for dangerous patterns ==="
	@FAIL=0; \
	if grep -rn 'Command::new("sh")' crates/contexere-reduce/src/hooks/; then \
		echo "WARNING: Command::new(\"sh\") found in hooks"; \
		FAIL=1; \
	fi; \
	if grep -rn 'LD_PRELOAD' src/ crates/contexere-reduce/src/ --include='*.rs' | grep -v 'guard/env.rs' | grep -v 'DENYLIST'; then \
		echo "WARNING: LD_PRELOAD referenced outside guard/env.rs"; \
		FAIL=1; \
	fi; \
	if grep -rn ' as char' src/ crates/contexere-reduce/src/ --include='*.rs'; then \
		echo "WARNING: 'as char' found. This is unsafe for UTF-8 bytes. Use char::from() or iterators instead."; \
		FAIL=1; \
	fi; \
	UNWRAP_COUNT=$$(grep -rn '\.unwrap()' crates/contexere-reduce/src/hooks/ --include='*.rs' | grep -v '#\[test\]' | grep -v 'mod tests' | grep -v '// safe:' | wc -l | tr -d ' '); \
	if [ "$$UNWRAP_COUNT" -gt 5 ]; then \
		echo "WARNING: $$UNWRAP_COUNT unwrap() calls in src/hooks/ (max 5 allowed)"; \
		grep -rn '\.unwrap()' crates/contexere-reduce/src/hooks/ --include='*.rs' | grep -v '#\[test\]' | grep -v 'mod tests'; \
		FAIL=1; \
	fi; \
	if [ $$FAIL -eq 1 ]; then \
		echo "Security checks found issues. Review above."; \
		exit 1; \
	fi; \
	echo "All security pattern checks passed ✓"

# Where `cargo build --profile ci` writes, which follows CARGO_TARGET_DIR (#969).
CI_BIN = $(or $(CARGO_TARGET_DIR),target)/ci/contexere

binary-check:
	@echo "=== Building Release Binary ==="
	cargo build --profile ci
	@echo "=== Checking Binary Size ==="
	@if [ ! -f "$(CI_BIN)" ]; then \
		echo "ERROR: no binary at $(CI_BIN)"; \
		exit 1; \
	fi
	@SIZE=$$(stat -c%s "$(CI_BIN)" 2>/dev/null || stat -f%z "$(CI_BIN)"); \
	SIZE_MB=$$((SIZE / 1048576)); \
	echo "Binary size: $${SIZE_MB}MB ($${SIZE} bytes)"; \
	if [ $$SIZE -gt 15728640 ]; then \
		echo "ERROR: Binary exceeds 15MB limit"; \
		exit 1; \
	fi; \
	echo "Binary size check passed ✓"
	@echo "=== Running Smoke Tests ==="
	chmod +x tests/smoke_test.sh
	# The smoke test drives the binary directly, so cargo's `[env]` block does
	# not reach it and it would learn filters into the developer's live
	# `~/.omni` (#307). It gets its own home rather than the suite's, and a
	# clean one each run: it asserts things like "pipe output <= input", and
	# session history feeds the scorer, so inheriting whatever `cargo test`
	# left behind makes those assertions depend on test ordering.
	rm -rf $(CURDIR)/target/omni-smoke
	OMNI_HOME=$(CURDIR)/target/omni-smoke tests/smoke_test.sh "$(CI_BIN)"

ci: fmt clippy test security binary-check
	@echo "========================================"
	@echo "🚀 All CI checks passed successfully! 🚀"
	@echo "========================================"

bench:
	./scripts/bench.sh

clean:
	cargo clean

changelog-cut:
	@if [ -z "$(VERSION)" ]; then echo "Usage: make changelog-cut VERSION=0.5.1"; exit 1; fi
	./scripts/changelog_cut.sh $(VERSION)

bump:
	@if [ -z "$(VERSION)" ]; then echo "Usage: make bump VERSION=0.5.1"; exit 1; fi
	./scripts/bump_version.sh $(VERSION)

release-sha:
	@if [ -z "$(VERSION)" ]; then echo "Usage: make release-sha VERSION=0.5.1"; exit 1; fi
	./scripts/update_homebrew_sha.sh $(VERSION)

release:
	@if [ -z "$(VERSION)" ]; then echo "Usage: make release VERSION=0.5.1"; exit 1; fi
	@$(MAKE) bump VERSION=$(VERSION)
	./scripts/omni-release.sh $(VERSION)
