.PHONY: bench bench-image bench-ir bench-wall bench-record bench-compare bench-flame web web-plain web-threaded web-threaded-check version test test-mooneye ledger ledger-check ledger-nds ledger-bios ledger-bios-check ledger-intro ledger-intro-check arm7tdmi build-tests lab lab-deps check fmt lint dev native up down build logs publish results significance release bundle-mac icons
VERSION := $(shell sed -n '/^\[workspace.package\]/,/^\[/s/^version = "\(.*\)"/\1/p' Cargo.toml)
WEB_NIGHTLY := nightly-2026-11-01
PUBLIC_URL ?= /
WEB_THREADED_LINK := $(foreach a,--shared-memory --import-memory --max-memory=1073741824 --export=__wasm_init_tls --export=__tls_size --export=__tls_align --export=__tls_base --export=__heap_base --export=__data_end,-C link-arg=$(a))
THREADED := RUSTUP_TOOLCHAIN=$(WEB_NIGHTLY) CARGO_TARGET_DIR=$(CURDIR)/target/threaded \
	CARGO_UNSTABLE_BUILD_STD=panic_abort,std \
	CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUSTFLAGS="-C target-feature=+atomics,+bulk-memory,+simd128 $(WEB_THREADED_LINK)"

version:
	@echo $(VERSION)

BENCH := cargo run -q --profile dist -p citrine-gb-bench --
BENCH_OUT := target/bench
LEDGER_DIR ?= $(abspath $(or $(LEDGER_CACHE),$(or $(XDG_CACHE_HOME),$(HOME)/.cache)/citrine-gb/ledger))

bench: ledger-nds
	$(BENCH) run --repeat 3

bench-image:
	docker build -q -t localhost/citrine-gb-bench -f crates/citrine-gb-bench/Dockerfile crates/citrine-gb-bench

bench-ir: bench-image ledger-nds
	mkdir -p $(LEDGER_DIR)
	docker run --rm --user $$(id -u):$$(id -g) -v $(LEDGER_DIR):/ledger -e LEDGER_CACHE=/ledger -e PROFILES -e REPEAT -e JOBS -e IDS -e APP -e EXTRA -e HOME=/tmp -e GLIBC_TUNABLES -v $(CURDIR):/w -w /w localhost/citrine-gb-bench sh crates/citrine-gb-bench/callgrind.sh $(BENCH_OUT)/ir

bench-wall:
	sh crates/citrine-gb-bench/pairs.sh $(BENCH_OUT)/wall . $(BASE_TREE)

bench-record:
	$(BENCH) record --out $(BENCH_OUT)/reading.json --readings $(BENCH_OUT)/wall/head.json \
		--ir dist=$(BENCH_OUT)/ir/dist --ir release=$(BENCH_OUT)/ir/release \
		--meta version=$(VERSION) --meta commit=$$(git rev-parse --short HEAD) \
		--meta "rustc=$$(cat $(BENCH_OUT)/ir/rustc.txt)" --meta "valgrind=$$(cat $(BENCH_OUT)/ir/valgrind.txt)"

bench-compare:
	$(BENCH) compare $(BASE) $(or $(HEAD),$(BENCH_OUT)/reading.json)

bench-flame:
	$(BENCH) flame $(BENCH_OUT)/$(ID).svg $(BENCH_OUT)/ir/dist/$(ID).1.out --title $(ID) $(if $(BASE_IR),--base $(BASE_IR)/dist/$(ID).1.out)

results: build-tests lab-deps
	cargo run --release -p citrine-gb-lab --bin collect

significance: lab-deps
	cargo run --release -p citrine-gb-lab --bin analyze

lab: lab-deps
	cargo clippy -p citrine-gb-lab --all-targets
	cargo test --release -p citrine-gb-lab

lab-deps:
	git submodule update --init crates/sameboy-sys/SameBoy crates/mgba-sys/mGBA crates/melonds-sys/melonDS

build-tests:
	git submodule update --init tests/mooneye
	$(MAKE) -C tests/mooneye

test-mooneye: build-tests
	cargo test --release --test mooneye

ledger: ledger-nds
	cargo test --release -p citrine-gb --features nds --test ledger -- --write

ledger-nds:
	sh tests/ledger/build-nds.sh

ledger-check: ledger-nds
	cargo test --release -p citrine-gb --features nds --test ledger
	cargo test --release -p citrine-gb --features jit-lockstep --test jit_lockstep

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

web:
	$(MAKE) web-plain
	$(MAKE) web-threaded

web-plain:
	rustup target add wasm32-unknown-unknown
	cd crates/citrine-gb-app && trunk build --release --public-url $(PUBLIC_URL)

web-threaded:
	rustup toolchain install $(WEB_NIGHTLY) --profile minimal --component rust-src,clippy --target wasm32-unknown-unknown
	cd crates/citrine-gb-app && $(THREADED) trunk --config Trunk-threaded.toml build --release --public-url $(PUBLIC_URL)threaded/

web-threaded-check:
	rustup toolchain install $(WEB_NIGHTLY) --profile minimal --component rust-src,clippy --target wasm32-unknown-unknown
	$(THREADED) cargo clippy -p citrine-gb-threads -p citrine-gb-app --target wasm32-unknown-unknown -- -D warnings

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