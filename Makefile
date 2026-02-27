.PHONY: check dev native up down build logs

check:
	rustup target add wasm32-unknown-unknown aarch64-apple-darwin x86_64-unknown-linux-gnu x86_64-pc-windows-msvc
	cargo check -p citrine-gb
	cargo check -p citrine-gb --features debug
	cargo check -p citrine-gb --features serde
	cargo check -p citrine-gb-app
	cargo check -p citrine-gb-app --target wasm32-unknown-unknown
	cargo check -p citrine-gb-app --target aarch64-apple-darwin
	cargo check -p citrine-gb-app --target x86_64-unknown-linux-gnu
	cargo check -p citrine-gb-app --target x86_64-pc-windows-msvc
	cargo test -p citrine-gb

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