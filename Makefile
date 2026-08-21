.PHONY: version test test-mooneye build-tests lab lab-deps check fmt lint dev native up down build logs publish results significance
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
	git submodule update --init lab/SameBoy

build-tests:
	git submodule update --init tests/mooneye
	$(MAKE) -C tests/mooneye

test-mooneye: build-tests
	cargo test --release --test mooneye

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

publish:
	cargo test -p citrine-gb --release
	cd lib && cargo publish