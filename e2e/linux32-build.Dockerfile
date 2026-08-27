FROM rust:1.97.1-bookworm

RUN rustup target add i686-unknown-linux-gnu \
    && apt-get update \
    && apt-get install --yes --no-install-recommends g++-i686-linux-gnu \
    && rm -rf /var/lib/apt/lists/*
