# syntax=docker/dockerfile:1.7
#
# Multi-stage build for agent-pinball.
# Builder: rust:1.95-slim with BuildKit caches for the registry and target dir.
# Runtime: debian:bookworm-slim, non-root user owning /data.
FROM rust:1.95-slim-bookworm AS builder
WORKDIR /work
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates ./crates
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/work/target \
    cargo build --locked --release --bin agent-pinball \
 && cp target/release/agent-pinball /usr/local/bin/agent-pinball

FROM debian:bookworm-slim AS runtime
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates \
 && rm -rf /var/lib/apt/lists/* \
 && useradd --system --uid 1000 --user-group --home-dir /data \
         --shell /usr/sbin/nologin pinball \
 && mkdir -p /data \
 && chown -R pinball:pinball /data
COPY --from=builder /usr/local/bin/agent-pinball /usr/local/bin/agent-pinball

# Publishing a port requires binding the namespace's wildcard address. The
# container flag tells `serve` that this bind is not network exposure; what
# decides reachability is the host-side `-p`, which must stay on 127.0.0.1.
ENV AGENT_PINBALL_BIND=0.0.0.0:4242
ENV AGENT_PINBALL_IN_CONTAINER=1
# Docker Desktop's host alias joins the loopback names so the host can reach
# the panel; anything wider replaces this value entirely.
ENV AGENT_PINBALL_ALLOWED_HOSTS="localhost,127.0.0.1,::1,host.docker.internal"

USER pinball
WORKDIR /data
EXPOSE 4242
HEALTHCHECK --interval=30s --timeout=5s --start-period=5s --retries=3 \
    CMD ["/usr/local/bin/agent-pinball", "status"]
ENTRYPOINT ["/usr/local/bin/agent-pinball"]
CMD ["serve"]
