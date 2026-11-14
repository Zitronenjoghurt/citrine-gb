FROM lukemathwalker/cargo-chef:latest-rust-1 AS chef
WORKDIR /citrine
RUN apt-get update && apt-get install -y pkg-config libudev-dev libasound2-dev binaryen && rm -rf /var/lib/apt/lists/*
RUN rustup target add wasm32-unknown-unknown
ARG BINSTALL_VERSION=1.27.0
RUN curl -fsSL --proto '=https' --tlsv1.2 "https://github.com/cargo-bins/cargo-binstall/releases/download/v${BINSTALL_VERSION}/cargo-binstall-$(uname -m)-unknown-linux-musl.tgz" | tar xz -C /usr/local/cargo/bin cargo-binstall
ENV BINSTALL_DISABLE_TELEMETRY=true
RUN cargo binstall --no-confirm --disable-strategies quick-install,compile trunk@=0.22.1

FROM chef AS wasm-bindgen-version
COPY Cargo.lock .
RUN sed -n '/^name = "wasm-bindgen"$/{n;s/^version = "\(.*\)"$/\1/p;}' Cargo.lock > /wasm-bindgen-version

FROM chef AS tools
COPY --from=wasm-bindgen-version /wasm-bindgen-version /wasm-bindgen-version
RUN cargo binstall --no-confirm --disable-strategies quick-install,compile --root /tools wasm-bindgen-cli@="$(cat /wasm-bindgen-version)"

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=tools /tools/bin/wasm-bindgen /usr/local/bin/wasm-bindgen
COPY --from=planner /citrine/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json --bin citrine-gb-server
COPY . .
RUN cargo build --release --bin citrine-gb-server
RUN mkdir -p /citrine/empty

RUN make web

FROM gcr.io/distroless/cc-debian13:nonroot AS runtime
WORKDIR /citrine
COPY --from=builder /citrine/target/release/citrine-gb-server /usr/local/bin/server
COPY --from=builder /citrine/crates/citrine-gb-app/dist /citrine/static
COPY --from=builder --chown=nonroot:nonroot /citrine/empty /citrine/data
EXPOSE 13707
CMD ["/usr/local/bin/server"]