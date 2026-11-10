# Debian trixie: the prebuilt dioxus-cli needs glibc 2.39 or newer (bookworm ships 2.36).
FROM rust:1-trixie AS build
WORKDIR /src
RUN rustup target add wasm32-unknown-unknown \
    && curl -L --proto '=https' --tlsv1.2 -sSf https://raw.githubusercontent.com/cargo-bins/cargo-binstall/main/install-from-binstall-release.sh | bash \
    && cargo binstall -y dioxus-cli@0.7.10
COPY . .
RUN cargo build --release -p reelhouse-server \
    && cd apps/reelhouse && dx build --platform web --features web --release

FROM debian:trixie-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ffmpeg ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=build /src/target/release/reelhouse-server /usr/local/bin/reelhouse-server
COPY --from=build /src/target/dx/reelhouse-app/release/web/public /usr/share/reelhouse/web
ENV REELHOUSE_DATA_DIR=/data REELHOUSE_BIND=0.0.0.0:8420 REELHOUSE_WEB_DIR=/usr/share/reelhouse/web
VOLUME /data
EXPOSE 8420
ENTRYPOINT ["reelhouse-server"]
