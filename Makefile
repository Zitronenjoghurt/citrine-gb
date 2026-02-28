.PHONY: check dev native up down build logs release mac win

check:
	rustup target add wasm32-unknown-unknown aarch64-apple-darwin x86_64-unknown-linux-gnu x86_64-pc-windows-gnu
	cargo check -p citrine-gb
	cargo check -p citrine-gb --features debug
	cargo check -p citrine-gb --features serde
	cargo check -p citrine-gb-app
	cargo check -p citrine-gb-app --target wasm32-unknown-unknown
	cargo check -p citrine-gb-app --target aarch64-apple-darwin
	cargo check -p citrine-gb-app --target x86_64-unknown-linux-gnu
	cargo check -p citrine-gb-app --target x86_64-pc-windows-gnu
	cargo test -p citrine-gb --release

dev:
	cargo install trunk
	cd app && trunk serve --open

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
	@if [ -z "$(v)" ]; then echo "Error: Version parameter is required. Use 'make mac v=x.y.z'"; exit 1; fi
	cd app && CARGO_TARGET_DIR=../target cargo bundle --target aarch64-apple-darwin --release
	mkdir -p build/macos/v$(v)
	cp -r "target/aarch64-apple-darwin/release/bundle/osx/Citrine.app" "build/macos/v$(v)/Citrine v$(v).app"
	codesign --force --deep --sign "https://github.com/Zitronenjoghurt" "build/macos/v$(v)/Citrine v$(v).app"
	cd build/macos/v$(v) && zip -r citrine-v$(v)-mac-arm64.zip "Citrine v$(v).app"
	@echo "MacOS app bundle created and signed"

win:
	@if [ -z "$(v)" ]; then echo "Error: Version parameter is required. Use 'make win v=x.y.z'"; exit 1; fi
	cargo build --target x86_64-pc-windows-gnu --release --bin citrine-gb-app
	mkdir -p build/windows/v$(v)
	cp target/x86_64-pc-windows-gnu/release/citrine-gb-app.exe "build/windows/v$(v)/Citrine v$(v).exe"
	cd build/windows/v$(v) && zip -r citrine-v$(v)-win-64.zip "Citrine v$(v).exe"
	@echo "Windows executable built and zipped"