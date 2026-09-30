#!/bin/sh
# Oxfer relay host setup for Debian 12/13 and Ubuntu 24.04. Run as root.
#
# Idempotent. cloud-init.yaml runs it on first boot from /opt/oxfer-relay/.
# Run it by hand on a provider without user data, after an upgrade of the
# pins below, and in the yearly rebuild rehearsal:
#   cd p2p-transfer/deploy/relay
#   tar -cf - . | ssh root@HOST \
#     'mkdir -p /opt/oxfer-relay && tar -C /opt/oxfer-relay --no-same-owner -xf - && sh /opt/oxfer-relay/setup.sh'
#
# It installs a prebuilt, checksum-verified iroh-relay binary and never
# compiles anything on the host.
set -eu

# Relay release, pinned by version and SHA-256. Keep the version equal to
# the iroh version in the workspace Cargo.lock. Assets are the static musl
# builds published on the n0-computer/iroh GitHub release:
#   https://github.com/n0-computer/iroh/releases/download/v$RELAY_VERSION/iroh-relay-v$RELAY_VERSION-<target>.tar.gz
# Hashes computed from the downloaded assets on 2026-09-30; they equal the
# digests GitHub lists for the release assets.
RELAY_VERSION=1.1.0
RELAY_SHA256_X86_64=9a68108b824e4164ad2eec729cf0e8167e4cb50581cb745a242bace135df7614
RELAY_SHA256_AARCH64=1b4261b6dd0d17ae9a7516aa6d122b296a77678bac174ce30262702c9f91cb00

KIT_DIR=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
BIN=/usr/local/bin/iroh-relay

log() { printf 'oxfer-relay-setup: %s\n' "$*"; }
die() { printf 'oxfer-relay-setup: ERROR: %s\n' "$*" >&2; exit 1; }

[ "$(id -u)" -eq 0 ] || die "run as root"
for f in config.toml iroh-relay.service nftables.conf journald-oxfer-relay.conf \
	sshd-oxfer-relay.conf unattended-upgrades.conf; do
	[ -f "$KIT_DIR/$f" ] || die "missing $KIT_DIR/$f"
done

# shellcheck source=/dev/null
. /etc/os-release
case "${ID:-}:${VERSION_ID:-}" in
debian:12 | debian:13 | ubuntu:24.04) ;;
*) log "WARNING: untested system ${ID:-?} ${VERSION_ID:-?}; continuing" ;;
esac

# 1. Packages. rsyslog would copy the journal into /var/log with weeks of
# retention; the journal, capped at three days, is the only log store here.
# On first boot apt's daily timers may hold the dpkg lock; wait for it.
export DEBIAN_FRONTEND=noninteractive
apt_get() { apt-get -o DPkg::Lock::Timeout=600 "$@"; }
apt_get update -q
apt_get install -y -q --no-install-recommends ca-certificates curl nftables unattended-upgrades
if dpkg-query -W -f='${Status}' rsyslog 2>/dev/null | grep -q 'ok installed'; then
	apt_get purge -y -q rsyslog
fi

# 2. Relay binary.
case "$(uname -m)" in
x86_64) target=x86_64-unknown-linux-musl sha=$RELAY_SHA256_X86_64 ;;
aarch64 | arm64) target=aarch64-unknown-linux-musl sha=$RELAY_SHA256_AARCH64 ;;
*) die "no pinned iroh-relay build for $(uname -m)" ;;
esac
restart_relay=0
if [ "$("$BIN" --version 2>/dev/null || true)" != "iroh-relay $RELAY_VERSION" ]; then
	asset="iroh-relay-v$RELAY_VERSION-$target.tar.gz"
	tmp=$(mktemp -d)
	trap 'rm -rf "$tmp"' EXIT
	log "downloading $asset"
	curl -fsSL --proto '=https' --tlsv1.2 --retry 5 --retry-delay 3 -o "$tmp/$asset" \
		"https://github.com/n0-computer/iroh/releases/download/v$RELAY_VERSION/$asset"
	printf '%s  %s\n' "$sha" "$tmp/$asset" | sha256sum -c --quiet - ||
		die "SHA-256 mismatch for $asset; refusing to install"
	tar -xzf "$tmp/$asset" -C "$tmp" ./iroh-relay
	[ "$("$tmp/iroh-relay" --version)" = "iroh-relay $RELAY_VERSION" ] ||
		die "downloaded binary does not report version $RELAY_VERSION"
	install -m 0755 -o root -g root "$tmp/iroh-relay" "$BIN.new"
	mv -f "$BIN.new" "$BIN"
	restart_relay=1
	log "installed $("$BIN" --version)"
fi

# 3. Configuration files. install_file SRC DST MODE; returns 0 when DST changed.
install_file() {
	if cmp -s "$1" "$2"; then return 1; fi
	install -D -m "$3" -o root -g root "$1" "$2"
	log "updated $2"
}

if install_file "$KIT_DIR/config.toml" /etc/iroh-relay/config.toml 0644; then restart_relay=1; fi
if install_file "$KIT_DIR/iroh-relay.service" /etc/systemd/system/iroh-relay.service 0644; then restart_relay=1; fi
if install_file "$KIT_DIR/journald-oxfer-relay.conf" /etc/systemd/journald.conf.d/zz-oxfer-relay.conf 0644; then
	systemctl restart systemd-journald
	journalctl --rotate >/dev/null 2>&1 || true
	journalctl --vacuum-time=3d >/dev/null 2>&1 || true
fi
install_file "$KIT_DIR/unattended-upgrades.conf" /etc/apt/apt.conf.d/52oxfer-relay 0644 || true

# 4. Firewall: check first, then load, so a bad file never drops the host's
# existing rules.
install -d -m 0755 /etc/nftables.d
nft -c -f "$KIT_DIR/nftables.conf" || die "nftables.conf does not validate"
install_file "$KIT_DIR/nftables.conf" /etc/nftables.conf 0755 || true
systemctl enable nftables >/dev/null 2>&1
systemctl restart nftables

# 5. SSH: key-only, but only once a key is actually installed.
has_key=0
for k in /root/.ssh/authorized_keys /home/*/.ssh/authorized_keys; do
	if [ -s "$k" ] && grep -Eq '^(ssh-|ecdsa-|sk-)' "$k"; then has_key=1; fi
done
if [ ! -x /usr/sbin/sshd ]; then
	log "sshd not installed; skipping SSH hardening"
elif [ "$has_key" -eq 0 ]; then
	log "WARNING: no authorized SSH key found; password login left as is"
else
	# Ubuntu 24.04 starts sshd through ssh.socket, so its privilege
	# separation directory (RuntimeDirectory=sshd) may not exist yet, and
	# "sshd -t" fails without it.
	install -d -m 0755 /run/sshd
	cp /etc/ssh/sshd_config.d/01-oxfer-relay.conf /tmp/oxfer-sshd.bak 2>/dev/null || true
	if install_file "$KIT_DIR/sshd-oxfer-relay.conf" /etc/ssh/sshd_config.d/01-oxfer-relay.conf 0644; then
		if /usr/sbin/sshd -t; then
			systemctl try-reload-or-restart ssh.service
		else
			log "WARNING: sshd rejected the drop-in; restoring the previous one"
			if [ -f /tmp/oxfer-sshd.bak ]; then
				mv -f /tmp/oxfer-sshd.bak /etc/ssh/sshd_config.d/01-oxfer-relay.conf
			else
				rm -f /etc/ssh/sshd_config.d/01-oxfer-relay.conf
			fi
		fi
	fi
	rm -f /tmp/oxfer-sshd.bak
fi

# 6. Unattended upgrades and the relay itself.
systemctl enable --now unattended-upgrades.service >/dev/null 2>&1 || true
systemctl daemon-reload
systemctl enable iroh-relay.service >/dev/null 2>&1
if [ "$restart_relay" -eq 1 ]; then
	systemctl restart iroh-relay.service
else
	systemctl start iroh-relay.service
fi

# 7. Local smoke test: the plain-HTTP captive-portal endpoint answers even
# before the certificate exists.
i=0
until [ "$(curl -s -o /dev/null -w '%{http_code}' --max-time 3 http://127.0.0.1/generate_204 || true)" = 204 ]; do
	i=$((i + 1))
	[ "$i" -lt 10 ] || die "relay does not answer on port 80; see: journalctl -u iroh-relay"
	sleep 1
done
log "relay is running: $("$BIN" --version)"
log "next: verify from outside, see README.md (First-boot verification)"
