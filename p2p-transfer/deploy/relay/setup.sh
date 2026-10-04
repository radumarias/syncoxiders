#!/bin/sh
# Oxfer relay host setup for Debian 12/13 and Ubuntu 24.04. Run as root.
#
# Idempotent. cloud-init.yaml runs it on first boot from /opt/oxfer-relay/.
# Run it by hand on a provider without user data, after an upgrade of the
# pins below, and in the yearly rebuild rehearsal:
#   cd p2p-transfer/deploy/relay
#   tar -cf - . | ssh root@HOST \
#     'mkdir -p /opt/oxfer-relay && tar -C /opt/oxfer-relay --no-same-owner -xf - && sh /opt/oxfer-relay/setup.sh'
# After editing /etc/oxfer-relay/denylist.txt, apply only that:
#   sh /opt/oxfer-relay/setup.sh --denylist
#
# It installs a prebuilt, checksum-verified iroh-relay binary and never
# compiles anything on the host. It keeps the server's own state: the
# denylist, /etc/nftables.d/ (IP bans, SSH rules) and the certificate.
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

# The kit files besides this script. render.sh fails unless its FILES list
# names the same ones.
KIT_FILES='config.toml relay.env denylist.sh iroh-relay.service nftables.conf
journald-oxfer-relay.conf sshd-oxfer-relay.conf unattended-upgrades.conf
logrotate-oxfer-relay.conf tmpfiles-oxfer-relay.conf oxfer-relay-ban
oxfer-relay-ban.service oxfer-relay-ban.timer'

KIT_DIR=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
BIN=/usr/local/bin/iroh-relay
CONF=/etc/iroh-relay/config.toml
DENYLIST=/etc/oxfer-relay/denylist.txt

log() { printf 'oxfer-relay-setup: %s\n' "$*"; }
die() { printf 'oxfer-relay-setup: ERROR: %s\n' "$*" >&2; exit 1; }

[ "$(id -u)" -eq 0 ] || die "run as root"
case "${1:-}" in '' | --denylist) ;; *) die "usage: sh setup.sh [--denylist]" ;; esac
# One run at a time. A --denylist run made while the first-boot run is still
# going could otherwise be overwritten by the config that run built from the
# older denylist.
exec 8>/run/oxfer-relay-setup.lock
flock -n 8 || { log "waiting for another setup.sh run to finish"; flock 8; }
for f in $KIT_FILES; do
	[ -f "$KIT_DIR/$f" ] || die "missing $KIT_DIR/$f"
done
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

# install_file SRC DST MODE; returns 0 when DST changed.
# Callers test its status, and sh ignores set -e inside a function called
# from an if or !, so every step that must not fail dies explicitly; here
# and in install_relay_config.
install_file() {
	if cmp -s "$1" "$2"; then return 1; fi
	install -D -m "$3" -o root -g root "$1" "$2" || die "could not install $2"
	log "updated $2"
}

# The relay config is the kit's config.toml with the endpoint IDs from
# DENYLIST in place of access = "everyone" (denylist.sh). Returns 0 when
# CONF changed; keeps the old one as CONF.prev.
install_relay_config() {
	# A CONF.prev left by a run that stopped before check_relay is older than
	# CONF; check_relay must never put that one back.
	rm -f "$CONF.prev"
	list=/dev/null
	if [ -f "$DENYLIST" ]; then list=$DENYLIST; fi
	n=$(sh "$KIT_DIR/denylist.sh" merge "$KIT_DIR/config.toml" "$WORK/relay.toml" "$DENYLIST" <"$list") ||
		die "$DENYLIST not applied; $CONF is unchanged"
	if cmp -s "$WORK/relay.toml" "$CONF"; then return 1; fi
	# A new list is parsed by the relay binary itself first (2 s): an ID
	# that is not a valid key would keep the relay from starting.
	sh "$KIT_DIR/denylist.sh" check "$BIN" "$WORK/relay.toml" ||
		die "iroh-relay rejects $DENYLIST (an entry is not a valid endpoint ID); nothing changed"
	if [ -f "$CONF" ]; then cp -p "$CONF" "$CONF.prev" || die "could not back up $CONF"; fi
	if ! install -D -m 0644 -o root -g root "$WORK/relay.toml" "$CONF"; then
		if [ -f "$CONF.prev" ]; then mv -f "$CONF.prev" "$CONF"; fi
		die "could not install $CONF; the previous one is kept"
	fi
	log "updated $CONF ($n denylisted endpoint IDs)"
}

# Waits for the plain-HTTP captive-portal endpoint, which answers even
# before the certificate exists. If a new config keeps the relay from
# starting, puts the previous one back.
check_relay() {
	i=0
	until [ "$(curl -s -o /dev/null -w '%{http_code}' --max-time 3 http://127.0.0.1/generate_204 || true)" = 204 ]; do
		i=$((i + 1))
		if [ "$i" -ge 15 ]; then
			if [ -f "$CONF.prev" ]; then
				mv -f "$CONF.prev" "$CONF"
				systemctl restart iroh-relay.service
				die "the relay did not start with the new $CONF; the previous one is back. See: journalctl -u iroh-relay"
			fi
			die "relay does not answer on port 80; see: journalctl -u iroh-relay"
		fi
		sleep 1
	done
	rm -f "$CONF.prev"
	log "relay is running: $("$BIN" --version)"
}

if [ "${1:-}" = --denylist ]; then
	[ -x "$BIN" ] || die "iroh-relay is not installed; run setup.sh without --denylist first"
	if ! install_relay_config; then
		log "denylist unchanged"
	elif systemctl is-active --quiet iroh-relay.service; then
		systemctl restart iroh-relay.service
		check_relay
	else
		rm -f "$CONF.prev"
		log "the relay is not running; it will use the new config when it starts"
	fi
	exit 0
fi

# shellcheck source=/dev/null
. /etc/os-release
case "${ID:-}:${VERSION_ID:-}" in
debian:12 | debian:13 | ubuntu:24.04) ;;
*) log "WARNING: untested system ${ID:-?} ${VERSION_ID:-?}; continuing" ;;
esac

# 1. Packages. The journal, capped at three days, is the only log store:
# rsyslog would copy it into /var/log with weeks of retention, and its purge
# leaves those files behind. libpam-lastlog2 (Debian 13, optional) keeps a
# permanent last-login database. On first boot apt's daily timers may hold
# the dpkg lock; wait for it.
export DEBIAN_FRONTEND=noninteractive
# 8>&-: a daemon a package script starts must not inherit, and so hold, the
# setup lock.
apt_get() { apt-get -o DPkg::Lock::Timeout=600 "$@" 8>&-; }
apt_get update -q
apt_get install -y -q --no-install-recommends ca-certificates curl iproute2 logrotate nftables unattended-upgrades
for p in rsyslog libpam-lastlog2; do
	if dpkg-query -W -f='${Status}' "$p" 2>/dev/null | grep -q 'ok installed'; then
		apt_get purge -y -q "$p"
	fi
done
for f in syslog auth.log kern.log mail.log mail.err mail.info mail.warn user.log \
	cron.log daemon.log lpr.log debug messages ufw.log; do
	for g in "/var/log/$f" "/var/log/$f".[0-9]*; do
		if [ -e "$g" ]; then rm -f "$g" && log "deleted $g"; fi
	done
done
rm -f /var/lib/lastlog/lastlog2.db /var/lib/lastlog/lastlog2.db-journal

# 2. Relay binary.
case "$(uname -m)" in
x86_64) target=x86_64-unknown-linux-musl sha=$RELAY_SHA256_X86_64 ;;
aarch64 | arm64) target=aarch64-unknown-linux-musl sha=$RELAY_SHA256_AARCH64 ;;
*) die "no pinned iroh-relay build for $(uname -m)" ;;
esac
restart_relay=0
if [ "$("$BIN" --version 2>/dev/null || true)" != "iroh-relay $RELAY_VERSION" ]; then
	asset="iroh-relay-v$RELAY_VERSION-$target.tar.gz"
	log "downloading $asset"
	curl -fsSL --proto '=https' --tlsv1.2 --retry 5 --retry-delay 3 -o "$WORK/$asset" \
		"https://github.com/n0-computer/iroh/releases/download/v$RELAY_VERSION/$asset"
	printf '%s  %s\n' "$sha" "$WORK/$asset" | sha256sum -c --quiet - ||
		die "SHA-256 mismatch for $asset; refusing to install"
	tar -xzf "$WORK/$asset" -C "$WORK" ./iroh-relay
	[ "$("$WORK/iroh-relay" --version)" = "iroh-relay $RELAY_VERSION" ] ||
		die "downloaded binary does not report version $RELAY_VERSION"
	install -m 0755 -o root -g root "$WORK/iroh-relay" "$BIN.new"
	mv -f "$BIN.new" "$BIN"
	restart_relay=1
	log "installed $("$BIN" --version)"
fi

# 3. Configuration files.
install -d -m 0700 /etc/oxfer-relay
if [ ! -e "$DENYLIST" ]; then
	printf '%s\n' '# Endpoint IDs the Oxfer relay refuses, one per line, as printed by' \
		"#   cargo run -q -p p2p-transfer --example ticket-endpoint-id -- '<ticket>'" \
		'# Apply with: sh /opt/oxfer-relay/setup.sh --denylist' >"$DENYLIST"
	chmod 0600 "$DENYLIST"
fi
if install_relay_config; then restart_relay=1; fi
if install_file "$KIT_DIR/relay.env" /etc/iroh-relay/relay.env 0644; then restart_relay=1; fi
if install_file "$KIT_DIR/iroh-relay.service" /etc/systemd/system/iroh-relay.service 0644; then restart_relay=1; fi
if install_file "$KIT_DIR/journald-oxfer-relay.conf" /etc/systemd/journald.conf.d/zz-oxfer-relay.conf 0644; then
	systemctl restart systemd-journald
	journalctl --rotate >/dev/null 2>&1 || true
	journalctl --vacuum-time=3d >/dev/null 2>&1 || true
fi
install_file "$KIT_DIR/unattended-upgrades.conf" /etc/apt/apt.conf.d/52oxfer-relay 0644 || true

# Login records. This logrotate file empties wtmp, btmp and wtmp.db daily
# and replaces the packages' monthly and yearly entries for them. When it
# is new, older records and rotated copies go at once.
for f in wtmp btmp wtmpdb; do
	if [ -e "/etc/logrotate.d/$f" ]; then rm -f "/etc/logrotate.d/$f" && log "deleted /etc/logrotate.d/$f"; fi
done
if install_file "$KIT_DIR/logrotate-oxfer-relay.conf" /etc/logrotate.d/oxfer-relay 0644; then
	rm -f /var/log/wtmp.[0-9]* /var/log/btmp.[0-9]* /var/log/wtmp.db.[0-9]*
	for f in /var/log/wtmp /var/log/btmp /var/log/wtmp.db; do
		if [ -f "$f" ]; then : >"$f"; fi
	done
fi
systemctl enable --now logrotate.timer >/dev/null 2>&1 || true
# lastlog: a link to /dev/null, now and at every boot.
install_file "$KIT_DIR/tmpfiles-oxfer-relay.conf" /etc/tmpfiles.d/oxfer-relay.conf 0644 || true
systemd-tmpfiles --create /etc/tmpfiles.d/oxfer-relay.conf

# IP bans (oxfer-relay-ban) and their hourly expiry check.
install_file "$KIT_DIR/oxfer-relay-ban" /usr/local/sbin/oxfer-relay-ban 0755 || true
install_file "$KIT_DIR/oxfer-relay-ban.service" /etc/systemd/system/oxfer-relay-ban.service 0644 || true
install_file "$KIT_DIR/oxfer-relay-ban.timer" /etc/systemd/system/oxfer-relay-ban.timer 0644 || true
systemctl daemon-reload

# 4. Firewall: check first, then load, so a bad file never drops the host's
# existing rules. Reload only when the file changed or the table is
# missing; a reload keeps bans, which come from /etc/nftables.d/bans.nft.
install -d -m 0755 /etc/nftables.d
# oxfer-relay-ban's lock: it moves a new bans.nft into place before checking
# it, so it must not do that while this run checks and loads the ruleset.
exec 9>/run/oxfer-relay-ban.lock
flock 9
nft -c -f "$KIT_DIR/nftables.conf" || die "nftables.conf (or a file in /etc/nftables.d) does not validate"
fw=0
# 0755, as the nftables package ships /etc/nftables.conf: it is an nft -f
# script (#!/usr/sbin/nft -f).
if install_file "$KIT_DIR/nftables.conf" /etc/nftables.conf 0755; then fw=1; fi
systemctl enable nftables >/dev/null 2>&1
if [ "$fw" -eq 1 ] || ! nft -t list table inet oxfer_relay >/dev/null 2>&1; then
	systemctl reload-or-restart nftables
fi
systemctl enable --now oxfer-relay-ban.timer >/dev/null 2>&1

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
	cp /etc/ssh/sshd_config.d/01-oxfer-relay.conf "$WORK/sshd.bak" 2>/dev/null || true
	if install_file "$KIT_DIR/sshd-oxfer-relay.conf" /etc/ssh/sshd_config.d/01-oxfer-relay.conf 0644; then
		if /usr/sbin/sshd -t; then
			systemctl try-reload-or-restart ssh.service
		else
			log "WARNING: sshd rejected the drop-in; restoring the previous one"
			if [ -f "$WORK/sshd.bak" ]; then
				mv -f "$WORK/sshd.bak" /etc/ssh/sshd_config.d/01-oxfer-relay.conf
			else
				rm -f /etc/ssh/sshd_config.d/01-oxfer-relay.conf
			fi
		fi
	fi
fi

# 6. Unattended upgrades and the relay itself. Without a certificate, the
# relay starts only once DNS for its name points at this host: each ACME
# attempt before that fails, and Let's Encrypt allows 5 failures per name
# per hour, used up within a minute by the client's retries.
systemctl enable --now unattended-upgrades.service >/dev/null 2>&1 || true
host=$(sed -n 's/^hostname = "\([^"]*\)"$/\1/p' "$KIT_DIR/config.toml")
dns=$(if command -v resolvectl >/dev/null 2>&1 && systemctl is-active --quiet systemd-resolved; then
	# --synthesize=no skips /etc/hosts, which may map the name to 127.0.1.1.
	resolvectl query --legend=no --synthesize=no --cache=no "$host" 2>/dev/null |
		awk '{ for (i = 1; i <= NF; i++) if ($i ~ /^[0-9a-f:.]+$/ && $i ~ /[.:]/ && $i !~ /:$/) print $i }'
else
	getent -s dns ahosts "$host" 2>/dev/null | awk '{ print $1 }'
fi | sort -u)
own=$(ip -o addr show scope global | awk '{ sub(/\/.*/, "", $4); print $4 }')
dns_here=0
if [ -n "$dns" ]; then
	dns_here=1
	for a in $dns; do printf '%s\n' "$own" | grep -qxF "$a" || dns_here=0; done
fi
if systemctl is-active --quiet iroh-relay.service ||
	ls /var/lib/private/iroh-relay/certs/cached_cert_* >/dev/null 2>&1 || [ "$dns_here" -eq 1 ]; then
	systemctl enable iroh-relay.service >/dev/null 2>&1
	if [ "$restart_relay" -eq 1 ]; then
		systemctl restart iroh-relay.service
	else
		systemctl start iroh-relay.service
	fi
	check_relay
	log "next: verify from outside, see README.md (First-boot verification)"
else
	rm -f "$CONF.prev"
	log "relay NOT started: no certificate yet, and DNS for $host gives" \
		"[$(printf '%s' "$dns" | tr '\n' ' ')], not only this host's [$(printf '%s' "$own" | tr '\n' ' ')]"
	log "once DNS points here: systemctl enable --now iroh-relay (or rerun setup.sh)"
fi
