# Prefer an available external build volume without requiring it on other hosts.
# An explicit environment/command-line target always wins.
BUILD_VOLUME ?= /Volumes/SSD1
ifeq ($(origin CARGO_TARGET_DIR),undefined)
CARGO_TARGET_DIR := $(shell sh scripts/cargo-target-dir.sh magicvault "$(CURDIR)" "$(BUILD_VOLUME)")
endif
ifeq ($(strip $(CARGO_TARGET_DIR)),)
$(error CARGO_TARGET_DIR must not be empty)
endif
export CARGO_TARGET_DIR
.DEFAULT_GOAL := help

help:
	@echo "MagicVault: test-compatibility | test-foundation | test-browser | test-delivery | test-browser-native | test-cli-native | test-extension-native | test-public-web | test-qualification-fixtures | fixture-site | build-standalone | package-extension | sync-lockfile"
	@echo "check/test are full lanes; run only when explicitly authorized. Browser native tests require a disposable Chrome/Chromium installation."
	@echo "Cargo artifacts: $(CARGO_TARGET_DIR) (print-target-dir; override CARGO_TARGET_DIR or BUILD_VOLUME)"
	@echo "Architecture: check-architecture | test-architecture | architecture-snapshot (candidate only)"

print-target-dir:
	@printf '%s\n' "$(CARGO_TARGET_DIR)"

# Routing regression tests only; no Rust compilation or application execution.
test-build-paths:
	python3 scripts/tests/test_build_paths.py

check-architecture:
	python3 scripts/check_architecture.py

architecture-snapshot:
	@python3 scripts/check_architecture.py --snapshot

test-architecture:
	python3 scripts/tests/test_architecture.py

check: check-architecture
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
	$(MAKE) test-extension

# Fast extension-only JavaScript and actual assembled-asset startup fixtures.
test-extension:
	node --test extension/tests/*.test.cjs scripts/tests/distribution.test.mjs

# Real local HTTP/TLS, child processes and shipped clients; synthetic custody/UI.
test-delivery:
	cargo test --locked -p magicvault-protocol --test delivery_contract
	cargo test --locked -p magicvault-effect --lib http::tests
	cargo test --locked -p magicvault-effect --test http_delivery --test process_delivery
	cargo test --locked -p magicvault-service --lib broker::delivery::tests
	cargo test --locked -p magicvault --test delivery_cli
	cargo test --locked -p magicvault-mcp --test end_to_end

# Explicit opt-in only. Set MAGICVAULT_CHROME to a trusted browser executable.
test-browser-native:
	cargo test --locked -p magicvault-effect --test chromium -- --ignored --test-threads=1

# Actual shipped CLI/daemon/CDP, but synthetic keys and human interaction.
test-cli-native:
	cargo test --locked -p magicvault --test browser_native -- --ignored --test-threads=1

# Real Chrome -> native host -> daemon -> MCP. Separate user-data roots contain
# test-only host manifests and a pregranted loopback extension fixture. Synthetic
# consent/key providers; no native setup, OS-wide host registration or keychain.
# Chrome must support Extensions.loadUnpacked; explicit browser path required.
test-extension-native: build-standalone
	@case "$(CARGO_TARGET_DIR)" in \
	  /*) magicvault_qa_target="$(CARGO_TARGET_DIR)" ;; \
	  *) magicvault_qa_target="$(CURDIR)/$(CARGO_TARGET_DIR)" ;; \
	esac; \
	MAGICVAULT_TEST_NATIVE_HOST="$$magicvault_qa_target/release/magicvault-native-host" \
	MAGICVAULT_TEST_MCP="$$magicvault_qa_target/release/magicvault-mcp" \
	cargo test --locked --release -p magicvault-mcp --test extension_native -- --ignored --nocapture --test-threads=1

# Twenty actual CLI launches/deliveries per destination; private synthetic roots.
test-delivery-latency:
	cargo test --locked --release -p magicvault --test delivery_cli repeated_cli_delivery_latency -- --ignored --nocapture --test-threads=1

# Explicit opt-in; only synthetic private roots and loopback recipients. Includes
# 32 deliveries/destination, 2,000 paced status calls, limits and in-flight drain.
test-service-reliability:
	cargo test --locked --release -p magicvault --test delivery_cli bounded_cli_delivery_capacity_and_service_resources -- --ignored --nocapture --test-threads=1
	cargo test --locked --release -p magicvault --test delivery_cli shutdown_drains_inflight_process_tree_and_http_without_replay -- --ignored --nocapture --test-threads=1

# Public demonstration sites only; requires MAGICVAULT_PUBLIC_WEB=1 as well.
test-public-web:
	cargo test --locked -p magicvault-effect --test chromium_public -- --ignored --test-threads=1

# Real local HTTP/child processes. These qualify fixtures, not product effects.
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

.PHONY: help print-target-dir test-build-paths check-architecture architecture-snapshot test-architecture check test test-compatibility test-foundation test-browser test-browser-native test-cli-native test-public-web test-qualification-fixtures fixture-site build-standalone package-extension sync-lockfile
.PHONY: test-delivery

# Distribution assembly never publishes, signs, initializes custody or installs a service.
test-distribution:
	node --test scripts/tests/distribution.test.mjs

package-npm:
	@test -n "$(NPM_SCOPE)" -a -n "$(PACKAGE_OUTPUT)" || (echo 'Set NPM_SCOPE and a fresh PACKAGE_OUTPUT directory'; exit 1)
	node scripts/package-npm.mjs --binary-dir "$(CARGO_TARGET_DIR)/release" --output "$(PACKAGE_OUTPUT)" --scope "$(NPM_SCOPE)"

test-package-install:
	@test -n "$(PACKAGE_OUTPUT)" -a -n "$(PACKAGE_TEST_OUTPUT)" || (echo 'Set PACKAGE_OUTPUT and a fresh PACKAGE_TEST_OUTPUT directory'; exit 1)
	node scripts/qualify-package.mjs --packages "$(PACKAGE_OUTPUT)" --work "$(PACKAGE_TEST_OUTPUT)" --with-rust-tests

test-package-browser:
	@test -n "$(PACKAGE_OUTPUT)" -a -n "$(PACKAGE_TEST_OUTPUT)" || (echo 'Set PACKAGE_OUTPUT and a fresh PACKAGE_TEST_OUTPUT directory'; exit 1)
	node scripts/qualify-package.mjs --packages "$(PACKAGE_OUTPUT)" --work "$(PACKAGE_TEST_OUTPUT)" --with-rust-tests --with-browser-tests

.PHONY: test-distribution package-npm test-package-install
.PHONY: test-extension
.PHONY: test-extension-native
.PHONY: test-delivery-latency
.PHONY: test-package-browser
.PHONY: test-service-reliability
