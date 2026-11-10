FROM rust:1-bookworm AS build
WORKDIR /src
COPY . .
RUN cargo build --release -p reelhouse-server

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ffmpeg ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=build /src/target/release/reelhouse-server /usr/local/bin/reelhouse-server
ENV REELHOUSE_DATA_DIR=/data REELHOUSE_BIND=0.0.0.0:8420
VOLUME /data
EXPOSE 8420
ENTRYPOINT ["reelhouse-server"]
