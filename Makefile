export CARGO_TARGET_DIR ?= $(CURDIR)/target
.DEFAULT_GOAL := help

help:
	@echo "MagicVault libraries: make check | test | test-compatibility"

check:
	python3 scripts/check_store_durability_adoption.py
	cargo check --workspace --all-targets

test:
	cargo test --workspace

test-compatibility:
	cargo test -p magicvault-core --test extraction_compatibility

.PHONY: help check test test-compatibility
