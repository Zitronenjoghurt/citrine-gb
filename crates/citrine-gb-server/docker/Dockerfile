FROM lukemathwalker/cargo-chef:latest-rust-1 AS chef
WORKDIR /citrine
RUN apt-get update && apt-get install -y pkg-config libudev-dev libasound2-dev && rm -rf /var/lib/apt/lists/*
RUN rustup target add wasm32-unknown-unknown
RUN curl -L --proto '=https' --tlsv1.2 -sSf https://raw.githubusercontent.com/cargo-bins/cargo-binstall/main/install-from-binstall-release.sh | bash
RUN cargo binstall -y trunk

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /citrine/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json --bin citrine-gb-server
COPY . .
RUN cargo build --release --bin citrine-gb-server
RUN mkdir -p /citrine/empty

WORKDIR /citrine/crates/citrine-gb-app
RUN trunk build --release

FROM gcr.io/distroless/cc-debian13:nonroot AS runtime
WORKDIR /citrine
COPY --from=builder /citrine/target/release/citrine-gb-server /usr/local/bin/server
COPY --from=builder /citrine/crates/citrine-gb-app/dist /citrine/static
COPY --from=builder --chown=nonroot:nonroot /citrine/empty /citrine/data
EXPOSE 13707
CMD ["/usr/local/bin/server"]