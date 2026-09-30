# Top-level entry points. Each target is thin; the real work is in scripts/
# and in the Cargo workspace under host/. Run `make help` for the list.

.DEFAULT_GOAL := help
HOST := host

.PHONY: help setup verify bind build test check fmt clippy audit vendor docs-check layout-check clean

help: ## Show this help
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*?## "}; {printf "  %-12s %s\n", $$1, $$2}'

setup: ## Install Arch packages, udev rule, memlock limit (needs sudo)
	sudo scripts/setup-arch.sh

verify: ## Confirm the card is enumerated and healthy (no root needed)
	scripts/verify-card.sh

bind: ## Bind the card to vfio-pci (needs sudo)
	sudo scripts/bind-vfio.sh

build: ## Build the host workspace and the assembly phictl and phitop (installed under host/target/debug/)
	cd $(HOST) && cargo build
	host/asm/phictl/build.sh
	host/asm/phitop/build.sh
	mkdir -p $(HOST)/target/debug
	cp host/asm/out/phictl $(HOST)/target/debug/phictl.new && mv -f $(HOST)/target/debug/phictl.new $(HOST)/target/debug/phictl
	cp host/asm/out/phitop $(HOST)/target/debug/phitop.new && mv -f $(HOST)/target/debug/phitop.new $(HOST)/target/debug/phitop

test: ## Run host tests that do not need the card
	cd $(HOST) && cargo test

fmt: ## Check formatting
	cd $(HOST) && cargo fmt --all -- --check

clippy: ## Lint
	cd $(HOST) && cargo clippy --all-targets -- -D warnings

docs-check: ## Enforce sibling .md files, the no-dash rule and relative links
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
