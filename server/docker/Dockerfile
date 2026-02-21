FROM lukemathwalker/cargo-chef:latest-rust-1 AS chef
WORKDIR /citrine
RUN rustup target add wasm32-unknown-unknown
RUN cargo install trunk

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /citrine/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json
COPY . .
RUN cargo build --release --bin citrine-gb-server

WORKDIR /citrine/app
RUN trunk build --release

FROM gcr.io/distroless/cc-debian13:nonroot AS runtime
WORKDIR /citrine
COPY --from=builder /citrine/target/release/citrine-gb-server /usr/local/bin/server
COPY --from=builder /citrine/app/dist /citrine/static
CMD ["/usr/local/bin/server"]