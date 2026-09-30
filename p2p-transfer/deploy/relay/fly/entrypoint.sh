#!/bin/sh
# Fly.io entrypoint for the Oxfer relay. Writes the runtime config from the
# VPS config (same keys, different bind addresses), then execs iroh-relay.
#
# Fly specifics (https://docs.fly.io/networking/udp-and-tcp/):
#  - TCP services bind 0.0.0.0.
#  - UDP must bind the address that fly-global-services resolves to, on the
#    same port as the public one, and works only on a dedicated IPv4.
#    quic_bind_addr takes an IP, so the name is resolved here.
# QUIC address discovery is off unless OXFER_RELAY_QAD=true (fly.toml [env]);
# only native clients use it.
set -eu

BASE=/etc/iroh-relay/config.base.toml
OUT=/etc/iroh-relay/config.toml

qad=false
quic_addr="0.0.0.0:7842"
if [ "${OXFER_RELAY_QAD:-false}" = true ]; then
	ip=$(getent ahostsv4 fly-global-services | awk 'NR == 1 { print $1 }')
	if [ -z "$ip" ]; then
		echo "oxfer-relay: cannot resolve fly-global-services; QAD needs a Fly machine" >&2
		exit 1
	fi
	qad=true
	quic_addr="$ip:7842"
fi

sed -e 's|^http_bind_addr = .*|http_bind_addr = "0.0.0.0:80"|' \
	-e 's|^https_bind_addr = .*|https_bind_addr = "0.0.0.0:443"|' \
	-e "s|^enable_quic_addr_discovery = .*|enable_quic_addr_discovery = $qad|" \
	-e "s|^quic_bind_addr = .*|quic_bind_addr = \"$quic_addr\"|" \
	"$BASE" > "$OUT"

# The config structs ignore unknown or misplaced keys, so fail closed if the
# base file no longer has the shape this script expects.
for line in \
	'http_bind_addr = "0.0.0.0:80"' \
	'https_bind_addr = "0.0.0.0:443"' \
	"enable_quic_addr_discovery = $qad" \
	"quic_bind_addr = \"$quic_addr\"" \
	'metrics_bind_addr = "127.0.0.1:9090"' \
	'cert_mode = "LetsEncrypt"' \
	'cert_dir = "/var/lib/iroh-relay/certs"'; do
	grep -qxF "$line" "$OUT" || {
		echo "oxfer-relay: expected line missing from $OUT: $line" >&2
		exit 1
	}
done

# Same log policy as iroh-relay.service: no per-connection lines.
export RUST_LOG="${RUST_LOG:-off,iroh_relay=warn,iroh_relay::server::http_server=off,iroh_relay::server::client=off,iroh_relay::server::streams=off,iroh_relay::protos=off,iroh_metrics=warn}"
export NO_COLOR=1

# One file descriptor per relayed WebSocket connection. The image's /bin/sh
# is BusyBox ash, which supports ulimit -n.
# shellcheck disable=SC3045
ulimit -n 65536 2>/dev/null || true

# shellcheck disable=SC3045
echo "oxfer-relay: $(/iroh-relay --version), QUIC address discovery $qad, open files limit $(ulimit -n)"
exec /iroh-relay --config-path "$OUT"
