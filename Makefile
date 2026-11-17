# PolecatWorks Data-Engine Makefile

.PHONY: help build test test-be seed-validation-suite preflight clean

help:
	@echo "PolecatWorks Data-Engine Targets:"
	@echo "  make build                  - Build de-be-container Rust backend"
	@echo "  make test                   - Run all backend unit and integration tests"
	@echo "  make preflight              - Validate configuration and CLI preflight"
	@echo "  make seed-validation-suite  - Generate deterministic Parquet chaos & drift suite"

build:
	cargo build --manifest-path de-be-container/Cargo.toml

test: test-be

test-be:
	cargo test --manifest-path de-be-container/Cargo.toml

preflight:
	cargo run --manifest-path de-be-container/Cargo.toml -- -c de-be-container/config/default.yaml preflight

seed-validation-suite:
	cargo run --manifest-path de-be-container/Cargo.toml -- -c de-be-container/config/default.yaml seed-validation-suite --output-dir /tmp/de-validation-suite

clean:
	cargo clean --manifest-path de-be-container/Cargo.toml
