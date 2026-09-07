export CARGO_TARGET_DIR ?= $(CURDIR)/target
.DEFAULT_GOAL := help

help:
	@echo "MagicVault: test-compatibility | test-foundation | test-browser | test-browser-native | test-cli-native | test-public-web | test-qualification-fixtures | fixture-site | build-standalone | package-extension | sync-lockfile"
	@echo "check/test are full lanes; run only when explicitly authorized. Browser native tests require a disposable Chrome/Chromium installation."

check:
	python3 scripts/check_store_durability_adoption.py
	cargo check --locked --workspace --all-targets

test:
	cargo test --locked --workspace

test-compatibility:
	cargo test --locked -p magicvault-core --test extraction_compatibility
	cargo test --locked -p magicvault-core --test metadata_projection
	cargo test --locked -p magicvault-core --lib store::tests::audit_
	cargo test --locked -p magicvault-primitives

test-foundation:
	cargo test --locked -p magicvault-protocol --test contract
	cargo test --locked -p magicvault-service --lib broker::tests
	cargo test --locked -p magicvault-service --lib storage::initialization_tests
	cargo test --locked -p magicvault-service --test foundation
	cargo test --locked -p magicvault --test cli_flow
	cargo test --locked -p magicvault-mcp --lib transport::tests
	cargo test --locked -p magicvault-mcp --bin magicvault-mcp
	cargo test --locked -p magicvault-mcp --test end_to_end

# Synthetic, targeted browser paths. Never starts a real browser or installer.
test-browser:
	cargo test --locked -p magicvault-protocol --test browser_contract
	cargo test --locked -p magicvault-effect --test cdp_transport --test native_bridge
	cargo test --locked -p magicvault-service --lib broker::browser::tests
	cargo test --locked -p magicvault-service --lib native::tests
	cargo test --locked -p magicvault --test cli_flow --test native_host
	cargo test --locked -p magicvault-mcp --test end_to_end
	node --test extension/tests/fill.test.cjs extension/tests/worker.test.cjs

# Explicit opt-in only. Set MAGICVAULT_CHROME to a trusted browser executable.
test-browser-native:
	cargo test --locked -p magicvault-effect --test chromium -- --ignored --test-threads=1

# Actual shipped CLI/daemon/CDP, but synthetic keys and human interaction.
test-cli-native:
	cargo test --locked -p magicvault --test browser_native -- --ignored --test-threads=1

# Public demonstration sites only; requires MAGICVAULT_PUBLIC_WEB=1 as well.
test-public-web:
	cargo test --locked -p magicvault-effect --test chromium_public -- --ignored --test-threads=1

# Real local HTTP/child processes. These qualify fixture contracts, not Phase 4.
test-qualification-fixtures:
	node --test scripts/tests/qualification-fixtures.test.mjs

fixture-site:
	node scripts/serve-browser-fixtures.mjs

build-standalone:
	cargo build --locked --release -p magicvault -p magicvault-mcp

# Source packaging only, no browser launch or extension/host installation.
package-extension:
	mkdir -p dist/extension
	cp extension/manifest.json extension/worker.js extension/options.html extension/options.js extension/options.css dist/extension/
	cp magicvault-effect/src/fill.js dist/extension/fill.js

# Dependency resolution only; does not compile or run checks/tests.
sync-lockfile:
	cargo update --workspace

.PHONY: help check test test-compatibility test-foundation test-browser test-browser-native test-cli-native test-public-web test-qualification-fixtures fixture-site build-standalone package-extension sync-lockfile
