export CARGO_TARGET_DIR ?= $(CURDIR)/target
.DEFAULT_GOAL := help

help:
	@echo "MagicVault: make check | test | test-compatibility | test-foundation | build-standalone | sync-lockfile"

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
	cargo test --locked -p magicvault-protocol -p magicvault-service -p magicvault -p magicvault-mcp

build-standalone:
	cargo build --locked --release -p magicvault -p magicvault-mcp

# Dependency resolution only; does not compile or run checks/tests.
sync-lockfile:
	cargo update --workspace

.PHONY: help check test test-compatibility test-foundation build-standalone sync-lockfile
