.PHONY: test test-mooneye build-tests lab lab-deps check dev native up down build logs release mac win publish results significance

# Read from [workspace.package] in Cargo.toml; override with `make release v=x.y.z`.
VERSION := $(or $(v),$(shell sed -n '/^\[workspace.package\]/,/^\[/s/^version = "\(.*\)"/\1/p' Cargo.toml))

results: build-tests lab-deps
	cargo run --release -p citrine-gb-lab --bin collect

significance: lab-deps
	cargo run --release -p citrine-gb-lab --bin analyze

# The lab is not a default workspace member, so `check`/`test` skip it; check it explicitly.
lab: lab-deps
	cargo clippy -p citrine-gb-lab --all-targets
	cargo test --release -p citrine-gb-lab

lab-deps:
	git submodule update --init lab/SameBoy

build-tests:
	git submodule update --init tests/mooneye
	$(MAKE) -C tests/mooneye

test-mooneye: build-tests
	cargo test --release --test mooneye

test:
	cargo test --release -- --nocapture

check:
	rustup target add wasm32-unknown-unknown aarch64-apple-darwin x86_64-pc-windows-gnu
	cargo check -p citrine-gb
	cargo check -p citrine-gb --features debug
	cargo check -p citrine-gb --features serde
	cargo check -p citrine-gb-app
	cargo check -p citrine-gb-app --target wasm32-unknown-unknown
	cargo check -p citrine-gb-app --target aarch64-apple-darwin
	cargo check -p citrine-gb-app --target x86_64-pc-windows-gnu
	$(MAKE) test

dev:
	cargo install trunk
	cd app && trunk serve --release --open

native:
	cargo run --release --bin citrine-gb-app

up:
	docker compose -f server/docker/docker-compose.yml up -d

down:
	docker compose -f server/docker/docker-compose.yml down

build:
	docker image prune -f
	docker compose -f server/docker/docker-compose.yml build

logs:
	docker compose -f server/docker/docker-compose.yml logs -f

release: check mac win

mac:
	@if [ -z "$(VERSION)" ]; then echo "Error: could not read version from Cargo.toml"; exit 1; fi
	cd app && CARGO_TARGET_DIR=../target cargo bundle --target aarch64-apple-darwin --release
	mkdir -p build/macos/v$(VERSION)
	cp -r "target/aarch64-apple-darwin/release/bundle/osx/Citrine.app" "build/macos/v$(VERSION)/Citrine v$(VERSION).app"
	codesign --force --deep --sign "https://github.com/Zitronenjoghurt" "build/macos/v$(VERSION)/Citrine v$(VERSION).app"
	cd build/macos/v$(VERSION) && zip -r citrine-v$(VERSION)-mac-arm64.zip "Citrine v$(VERSION).app"
	@echo "MacOS app bundle v$(VERSION) created and signed"

win:
	@if [ -z "$(VERSION)" ]; then echo "Error: could not read version from Cargo.toml"; exit 1; fi
	cargo build --target x86_64-pc-windows-gnu --release --bin citrine-gb-app
	mkdir -p build/windows/v$(VERSION)
	cp target/x86_64-pc-windows-gnu/release/citrine-gb-app.exe "build/windows/v$(VERSION)/Citrine v$(VERSION).exe"
	cd build/windows/v$(VERSION) && zip -r citrine-v$(VERSION)-win-64.zip "Citrine v$(VERSION).exe"
	@echo "Windows executable built and zipped"

publish:
	cargo test -p citrine-gb --release
	cd lib && cargo publish