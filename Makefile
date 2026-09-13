# Top-level entry points. Each target is thin; the real work is in scripts/
# and in the Cargo workspace under host/. Run `make help` for the list.

.DEFAULT_GOAL := help
HOST := host

.PHONY: help setup verify bind build test check fmt clippy audit vendor docs-check clean

help: ## Show this help
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*?## "}; {printf "  %-12s %s\n", $$1, $$2}'

setup: ## Install Arch packages, udev rule, memlock limit (needs sudo)
	sudo scripts/setup-arch.sh

verify: ## Confirm the card is enumerated and healthy (no root needed)
	scripts/verify-card.sh

bind: ## Bind the card to vfio-pci (needs sudo)
	sudo scripts/bind-vfio.sh

build: ## Build the host workspace
	cd $(HOST) && cargo build

test: ## Run host tests that do not need the card
	cd $(HOST) && cargo test

fmt: ## Check formatting
	cd $(HOST) && cargo fmt --all -- --check

clippy: ## Lint
	cd $(HOST) && cargo clippy --all-targets -- -D warnings

docs-check: ## Enforce sibling .md files and the no-em-dash rule
	scripts/check-docs.sh

check: docs-check fmt clippy build test layout-check ## Everything CI would run (also rebuilds the binaries)

audit: build ## Audit a binary for KNC-illegal instructions: make audit BIN=path
	$(HOST)/target/debug/phi-isa-audit $(BIN)

vendor: ## Fetch reference material into vendor/ (MPSS 3.8.6, Intel k1om tree, PDFs)
	scripts/fetch-vendor.sh

clean: ## Remove build outputs
	cd $(HOST) && cargo clean

layout-check: ## Compare the C ring layout with the Rust constants (tcc)
	tcc -run tools/ring-layout-check.c
