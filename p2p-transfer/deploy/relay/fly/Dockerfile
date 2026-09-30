# Oxfer relay on Fly.io: the official iroh-relay 1.1.0 image, pinned by digest.
# The digest is the multi-arch index of n0computer/iroh-relay:v1.1.0 on
# Docker Hub, read from the registry on 2026-09-30. Its /iroh-relay (amd64)
# is byte-identical to the GitHub release asset
# iroh-relay-v1.1.0-x86_64-unknown-linux-musl.tar.gz that setup.sh installs.
# Nothing is compiled here.
FROM docker.io/n0computer/iroh-relay:v1.1.0@sha256:7805aaf67ca59e04cd54999d142c85ed72e38e5bd4515c902fa5c8d838674747

# config.toml is a generated copy of ../config.toml (render.sh); the
# entrypoint rewrites only the bind addresses at start.
COPY config.toml /etc/iroh-relay/config.base.toml
COPY entrypoint.sh /usr/local/bin/oxfer-relay-entrypoint
RUN chmod 0644 /etc/iroh-relay/config.base.toml \
	&& chmod 0755 /usr/local/bin/oxfer-relay-entrypoint

# iroh-relay shuts down gracefully only on SIGINT (src/main.rs:605-611); as
# PID 1 in a plain container it would ignore SIGTERM. fly.toml sets the same.
STOPSIGNAL SIGINT

ENTRYPOINT ["/usr/local/bin/oxfer-relay-entrypoint"]
# The base image sets CMD [""]; clear it so no empty argument is passed.
CMD []
