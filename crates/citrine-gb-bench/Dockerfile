FROM debian:trixie-slim@sha256:1e1a22175fbf1b32157ddaf0219e1db4e5401fab2b94ed285add64d18587cd2d
ARG RUST=1.99.1
RUN apt-get -o Acquire::Retries=5 update \
    && apt-get install -y --no-install-recommends valgrind ca-certificates curl gcc libc6-dev pkg-config libasound2-dev libudev-dev libclang-dev g++ nasm \
    && rm -rf /var/lib/apt/lists/*
ENV RUSTUP_HOME=/opt/rustup CARGO_HOME=/opt/cargo PATH=/opt/cargo/bin:$PATH
RUN curl -sSf https://sh.rustup.rs | sh -s -- -y --no-modify-path --profile minimal --default-toolchain ${RUST} \
    && chmod -R a+rX /opt/rustup /opt/cargo
ENV GLIBC_TUNABLES=glibc.cpu.hwcaps=-AVX512F,-AVX2,-AVX,-FMA,-FMA4,-F16C,-ERMS,-FSRM,-AVX_Fast_Unaligned_Load,-Fast_Unaligned_Copy:glibc.cpu.x86_data_cache_size=32768:glibc.cpu.x86_shared_cache_size=4194304:glibc.cpu.x86_non_temporal_threshold=3145728:glibc.cpu.x86_rep_movsb_threshold=4294967295:glibc.cpu.x86_rep_stosb_threshold=4294967295
