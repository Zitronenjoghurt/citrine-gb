.PHONY: version test test-mooneye ledger ledger-check ledger-bios ledger-bios-check ledger-intro ledger-intro-check arm7tdmi build-tests lab lab-deps check fmt lint dev native up down build logs publish results significance release bundle-mac icons
VERSION := $(shell sed -n '/^\[workspace.package\]/,/^\[/s/^version = "\(.*\)"/\1/p' Cargo.toml)

version:
	@echo $(VERSION)

results: build-tests lab-deps
	cargo run --release -p citrine-gb-lab --bin collect

significance: lab-deps
	cargo run --release -p citrine-gb-lab --bin analyze

lab: lab-deps
	cargo clippy -p citrine-gb-lab --all-targets
	cargo test --release -p citrine-gb-lab

lab-deps:
	git submodule update --init crates/sameboy-sys/SameBoy crates/mgba-sys/mGBA

build-tests:
	git submodule update --init tests/mooneye
	$(MAKE) -C tests/mooneye

test-mooneye: build-tests
	cargo test --release --test mooneye

ledger:
	cargo test --release -p citrine-gb --features gba --test ledger -- --write

ledger-check:
	cargo test --release -p citrine-gb --features gba --test ledger

ledger-bios:
	cargo test --release -p citrine-gb --features gba --test ledger -- --bios "$(GBA_BIOS)" --write

ledger-bios-check:
	cargo test --release -p citrine-gb --features gba --test ledger -- --bios "$(GBA_BIOS)"

ledger-intro:
	cargo test --release -p citrine-gb --features gba --test ledger -- --intro --write

ledger-intro-check:
	cargo test --release -p citrine-gb --features gba --test ledger -- --intro

arm7tdmi:
	cargo test --release -p citrine-gb --features gba,debug --test arm7tdmi -- --fetch

test:
	cargo test --release -- --nocapture

check: fmt lint test
	rustup target add wasm32-unknown-unknown
	cargo check -p citrine-gb --features debug
	cargo check -p citrine-gb --features serde
	cargo check -p citrine-gb-app --target wasm32-unknown-unknown

fmt:
	cargo fmt --all

lint:
	cargo fmt --all --check
	cargo clippy --all-targets --all-features -- -D warnings

dev:
	cargo install trunk
	cd crates/citrine-gb-app && trunk serve --release --open

native:
	cargo run --release --bin citrine-gb-app

up:
	docker compose -f crates/citrine-gb-server/docker/docker-compose.yml up -d

down:
	docker compose -f crates/citrine-gb-server/docker/docker-compose.yml down

build:
	docker image prune -f
	docker compose -f crates/citrine-gb-server/docker/docker-compose.yml build

logs:
	docker compose -f crates/citrine-gb-server/docker/docker-compose.yml logs -f

publish:
	cargo test -p citrine-gb --release
	cd crates/citrine-gb && cargo publish

icons:
	./scripts/gen-icns.sh

bundle-mac:
	rustup target add aarch64-apple-darwin x86_64-apple-darwin
	MACOSX_DEPLOYMENT_TARGET=11.0 cargo build --profile dist --locked -p citrine-gb-app --target aarch64-apple-darwin
	MACOSX_DEPLOYMENT_TARGET=11.0 cargo build --profile dist --locked -p citrine-gb-app --target x86_64-apple-darwin
	./scripts/bundle-macos.sh $(VERSION)

release:
	@if [ -z "$(v)" ]; then echo "Error: Version parameter is required. Use 'make release v=x.y.z'"; exit 1; fi
	@grep -q '^version = "$(v)"$$' Cargo.toml || { echo "Error: workspace version in Cargo.toml does not match $(v)"; exit 1; }
	$(MAKE) check
	git tag v$(v)
	git push origin v$(v)