# RemoteCraft: the RustDesk Server OSS (hbbs + hbbr) and the support desk in one container.
#
#   docker compose up -d            (see docker-compose.yml and docs/deploy.md)
#
# hbbs and hbbr are the official, unmodified binaries copied from rustdesk/rustdesk-server
# (AGPL-3.0, source: https://github.com/rustdesk/rustdesk-server). RemoteCraft talks to them
# only over the network and reads their public key.

ARG RUSTDESK_TAG=1.1.16

FROM rustdesk/rustdesk-server:${RUSTDESK_TAG} AS rustdesk

FROM rust:1.97-bookworm AS build
WORKDIR /src
RUN rustup target add wasm32-unknown-unknown \
 && curl -sSL https://github.com/trunk-rs/trunk/releases/download/v0.21.14/trunk-x86_64-unknown-linux-gnu.tar.gz | tar -xz -C /usr/local/bin trunk
COPY . .
# The image's own toolchain (rust-toolchain.toml would download the same release again).
RUN RUSTUP_TOOLCHAIN="$(rustup default | cut -d' ' -f1)" cargo build --release -p remotecraft-cli \
 && cd apps/remotecraft-web && RUSTUP_TOOLCHAIN="$(rustup default | cut -d' ' -f1)" trunk build --release

FROM debian:bookworm-slim
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates tzdata \
 && rm -rf /var/lib/apt/lists/* \
 && useradd --system --home-dir /data --shell /usr/sbin/nologin remotecraft
COPY --from=rustdesk /usr/bin/hbbs /usr/bin/hbbr /usr/local/bin/
COPY --from=build /src/target/release/remotecraft-cli /usr/local/bin/remotecraft-cli
COPY --from=build /src/dist/web /app/web
COPY docker/entrypoint.sh /usr/local/bin/remotecraft-entrypoint
RUN chmod 0755 /usr/local/bin/remotecraft-entrypoint

# /data/rustdesk: the RustDesk server's key pair (back it up: losing it means reconfiguring
# every client). /data/desk: the desk (customers, PCs, interventions, GDPR record, audit log).
VOLUME ["/data/rustdesk", "/data/desk"]
ENV REMOTECRAFT_HOME=/data/desk \
    REMOTECRAFT_RUSTDESK_KEY_FILE=/data/rustdesk/id_ed25519.pub \
    REMOTECRAFT_RUSTDESK_CHECK_HOST=127.0.0.1 \
    REMOTECRAFT_BIND=0.0.0.0:7979 \
    REMOTECRAFT_STATIC=/app/web \
    TZ=Europe/Rome
# 21115 TCP NAT test · 21116 TCP+UDP IDs and connections · 21117 TCP relay · 7979 the desk API and web app
EXPOSE 21115/tcp 21116/tcp 21116/udp 21117/tcp 7979/tcp
HEALTHCHECK --interval=60s --timeout=10s --start-period=20s \
  CMD remotecraft-cli rustdesk >/dev/null 2>&1 || exit 1
ENTRYPOINT ["/usr/local/bin/remotecraft-entrypoint"]
