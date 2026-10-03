#!/bin/sh
# Endpoint-ID denylist of the Oxfer relay, for both variants: setup.sh feeds
# it /etc/oxfer-relay/denylist.txt, fly/entrypoint.sh the Fly secret
# OXFER_RELAY_DENYLIST.
#   sh denylist.sh merge BASE OUT NAME <LIST
#       Writes BASE to OUT with its line access = "everyone" replaced by
#       access.denylist = [...] holding the IDs in LIST (BASE unchanged when
#       LIST has none), and prints how many IDs there are. LIST holds
#       endpoint IDs separated by spaces, commas or new lines, as the
#       ticket-endpoint-id example prints them; "#" starts a comment. NAME
#       stands for LIST in messages.
#   sh denylist.sh check RELAY CONFIG
#       Has the iroh-relay binary RELAY parse the access.denylist of CONFIG,
#       if it has one. Only the relay can tell that 64 hex digits are a valid
#       Ed25519 key, and it would not start with one that is not.
# Neither ever prints an entry: a pasted share link carries its capability,
# which must not reach a terminal or a log.
set -eu

usage='usage: sh denylist.sh merge BASE OUT NAME <LIST | check RELAY CONFIG'
fail() { printf 'oxfer-relay-denylist: %s\n' "$*" >&2; exit 1; }
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

case "${1:-}" in
merge)
	[ $# -eq 4 ] || fail "$usage"
	base=$2 out=$3 name=$4
	# One "LINE ID" pair per entry, in lower case, then one pass over them
	# all for anything that is not an endpoint ID.
	awk '{ sub(/#.*/, ""); gsub(/,/, " "); for (i = 1; i <= NF; i++) print NR, tolower($i) }' >"$tmp/entries"
	bad=$(grep -Evx '[0-9]+ ([0-9a-f]{64}|[a-z2-7]{52})' "$tmp/entries" | head -n 1 | cut -d ' ' -f 1)
	[ -z "$bad" ] ||
		fail "$name line $bad: not an endpoint ID (64 hex digits, as ticket-endpoint-id prints); entry not shown"
	cut -d ' ' -f 2 "$tmp/entries" | sort -u >"$tmp/ids"
	n=$(grep -c . "$tmp/ids" || true)
	if [ "$n" -eq 0 ]; then
		cp "$base" "$out"
	else
		grep -qx 'access = "everyone"' "$base" || fail "$base lacks the line: access = \"everyone\""
		{
			echo 'access.denylist = ['
			sed 's/.*/  "&",/' "$tmp/ids"
			echo ']'
		} >"$tmp/deny.toml"
		awk -v f="$tmp/deny.toml" '$0 == "access = \"everyone\"" {
			while ((getline l < f) > 0) print l; next } { print }' "$base" >"$out"
	fi
	echo "$n"
	;;
check)
	[ $# -eq 3 ] || fail "$usage"
	sed -n '/^access\.denylist = \[$/,/^\]$/p' "$3" >"$tmp/deny.toml"
	[ -s "$tmp/deny.toml" ] || exit 0
	# A throwaway plain-HTTP relay on a loopback port with only the list:
	# still running after 2 s means it accepted the list. GNU timeout then
	# returns 124. BusyBox timeout (the Fly image) runs the relay in its own
	# process, which exits 0 on SIGINT. A rejected list exits 1 at once.
	{
		printf 'http_bind_addr = "127.0.0.1:0"\nenable_metrics = false\n'
		cat "$tmp/deny.toml"
	} >"$tmp/check.toml"
	rc=0
	timeout -s INT 2 "$2" --config-path "$tmp/check.toml" >"$tmp/check.log" 2>&1 || rc=$?
	case "$rc" in
	0 | 124) ;;
	*)
		cat "$tmp/check.log" >&2
		fail "iroh-relay rejects the denylist: an entry is not a valid endpoint ID"
		;;
	esac
	;;
*) fail "$usage" ;;
esac
