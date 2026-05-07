CARGO        := cargo
FEATURES     := --features duckdb-bundled
ARGS         ?=
TEST         ?=

.PHONY: help build release test test-one fmt fmt-check clippy check run install clean

help:
	@awk 'BEGIN {FS = ":.*##"; printf "Targets:\n"} /^[a-zA-Z_-]+:.*##/ { printf "  \033[36m%-12s\033[0m %s\n", $$1, $$2 }' $(MAKEFILE_LIST)

build: ## Debug build with bundled DuckDB
	$(CARGO) build $(FEATURES)

release: ## Portable release build with bundled DuckDB
	$(CARGO) build --release $(FEATURES)

test: ## Run full test suite
	$(CARGO) test $(FEATURES)

test-one: ## Run tests matching TEST=<substring>
	@if [ -z "$(TEST)" ]; then echo "Usage: make test-one TEST=<substring>"; exit 2; fi
	$(CARGO) test $(FEATURES) $(TEST)

fmt: ## Format sources
	$(CARGO) fmt

fmt-check: ## Verify formatting (CI gate)
	$(CARGO) fmt --check

clippy: ## Lint with -D warnings (CI gate)
	$(CARGO) clippy --all-targets --all-features -- -D warnings

check: fmt-check clippy test ## Full CI gate: fmt + clippy + test

run: ## Run the CLI: make run ARGS="volume list"
	$(CARGO) run $(FEATURES) -- $(ARGS)

install: ## Install the built binary into ~/.cargo/bin (honors Cargo.lock)
	$(CARGO) install --path . --locked $(FEATURES)

clean: ## cargo clean
	$(CARGO) clean
