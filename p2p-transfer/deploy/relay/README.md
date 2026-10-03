# Oxfer relay: deployment kit and runbook

This directory takes a blank EU VPS to a working iroh relay at
`https://relay.oxfer.app` in about ten minutes, on any provider that offers
Debian 12, Debian 13 or Ubuntu 24.04 (OVH, IONOS, Hetzner, Scaleway). It
implements the server side of section 4 of
[`../../docs/compliance-plan.md`](../../docs/compliance-plan.md) (items A1 to
A3, A5, A10 and A11) and documents the production switch of item A6.

The legal pages (`privacy.html`, `terms.html`, `abuse.html`) and the records
in [`../../docs/compliance/`](../../docs/compliance/README.md) describe this
VPS variant. A Fly.io variant is in [`fly/`](fly/), but it would make several
of their statements false; read "Fly.io variant" before choosing it.

The relay is the official `iroh-relay` binary, version 1.1.0, which matches
the `iroh` version in the workspace `Cargo.lock`. Nothing is compiled on the
server: `setup.sh` installs the prebuilt release binary and checks its SHA-256.

## Files

| File | Installed as | Purpose |
| --- | --- | --- |
| `config.toml` | `/etc/iroh-relay/config.toml` | Relay configuration, with the server's denylist merged in by `setup.sh`. Every key has a comment naming the iroh-relay 1.1.0 source lines it comes from. |
| `iroh-relay.service` | `/etc/systemd/system/iroh-relay.service` | Hardened systemd unit: dynamic user, only `CAP_NET_BIND_SERVICE`, read-only system, no per-connection logging. |
| `nftables.conf` | `/etc/nftables.conf` | Firewall: inbound dropped except SSH, TCP 80 and 443, UDP 7842, DHCPv6 replies and essential ICMP; ban sets; per-source connection limits. |
| `journald-oxfer-relay.conf` | `/etc/systemd/journald.conf.d/zz-oxfer-relay.conf` | Journal capped at 200 MB and three days. |
| `logrotate-oxfer-relay.conf` | `/etc/logrotate.d/oxfer-relay` | Empties the login records `wtmp`, `btmp` and, on Debian 13, `wtmp.db` every day. |
| `tmpfiles-oxfer-relay.conf` | `/etc/tmpfiles.d/oxfer-relay.conf` | Points `/var/log/lastlog` at `/dev/null`, so no last-login record is kept. |
| `sshd-oxfer-relay.conf` | `/etc/ssh/sshd_config.d/01-oxfer-relay.conf` | Key-only SSH. |
| `unattended-upgrades.conf` | `/etc/apt/apt.conf.d/52oxfer-relay` | Automatic security upgrades, reboot at 03:30 when needed. |
| `oxfer-relay-ban` | `/usr/local/sbin/oxfer-relay-ban` | Adds, lists and lifts IP bans, each for 1 to 365 days (30 by default), and keeps them with their expiry in `/etc/nftables.d/bans.nft`. |
| `oxfer-relay-ban.service`, `oxfer-relay-ban.timer` | `/etc/systemd/system/` | Hourly removal of expired bans. |
| `setup.sh` | `/opt/oxfer-relay/setup.sh` | Idempotent installer for all of the above. `setup.sh --denylist` applies only `/etc/oxfer-relay/denylist.txt`. |
| `cloud-init.yaml` | provider "user data" | Generated: writes the kit to `/opt/oxfer-relay/` and runs `setup.sh` on first boot. |
| `render.sh` | not installed | Regenerates `cloud-init.yaml` and `fly/config.toml`; `--check` detects drift. |
| `fly/` | Fly.io | `fly.toml`, `Dockerfile`, `entrypoint.sh` and a generated copy of `config.toml`. |

Edit the source files, then run `sh render.sh`. Never edit `cloud-init.yaml`
or `fly/config.toml` by hand; `sh render.sh --check` fails when they are stale.
`cloud-init.yaml` holds each kit file gzip-compressed and base64-encoded
(cloud-init's `encoding: gz+b64`): as plain text the kit is over Hetzner's
32 KiB user-data limit. `--check` decodes every embedded file and compares it
with its source.

This repository is public. Some state lives only on the server, and
`setup.sh` never overwrites it, so upgrades and reruns keep it:

- `/etc/oxfer-relay/denylist.txt`: endpoint IDs the relay refuses.
- `/etc/nftables.d/`: `bans.nft`, written by `oxfer-relay-ban`, and any
  host rules such as `ssh.nft`.
- The ACME account and certificate in `/var/lib/private/iroh-relay/certs/`.

Each block is recorded in the private abuse log, which is kept off the
server and out of this repository. It holds only the date and time, the
kind of report, the endpoint ID or address blocked, and the outcome. A
rebuild must carry the denylist and the bans across ("Yearly rebuild
rehearsal").

## What the relay exposes

Derived from the iroh-relay 1.1.0 source (`~/.cargo/registry/src/*/iroh-relay-1.1.0`).

| Port | Served | Used by | Source |
| --- | --- | --- | --- |
| TCP 443 | HTTPS and WSS: `/relay` (WebSocket upgrade, subprotocols `iroh-relay-v1` and `iroh-relay-v2`), `/ping` (latency probe), `/healthz` (JSON with the version), `/`, `/robots.txt`. Also answers the ACME TLS-ALPN-01 challenge. | Browsers and native clients | `src/server.rs:731-740`, `src/http.rs:13-15`, `src/server/http_server.rs:586-622 and 1088-1096` |
| TCP 80 | Only `GET /generate_204` (captive-portal check, echoes `X-Iroh-Challenge` as `X-Iroh-Response`); 404 for everything else | Native clients | `src/server.rs:802-816, 1052-1074 and 1161-1186`; iroh-1.1.0 `src/net_report/reportgen.rs:620` |
| UDP 7842 | QUIC address discovery (QAD) | Native clients only | `src/defaults.rs:7`, `src/quic.rs:97-119` |
| TCP 9090 on 127.0.0.1 | Prometheus metrics, aggregate counters only | You, through SSH | `src/main.rs:142-148 and 344-347` |

Facts from the source that shape this kit:

- **Certificates use TLS-ALPN-01 only.** tokio-rustls-acme 0.9.1 selects the
  `tls-alpn-01` challenge and fails if it is not offered
  (`tokio-rustls-acme src/acme.rs:187-191`). Port 80 is not needed for
  certificates; it stays open for the native captive-portal check. TCP 443
  must reach the relay directly, without a TLS-terminating proxy in front.
- **`/generate_204` is on port 80 only.** Over HTTPS it returns 404; the
  HTTPS probe path is `/ping`.
- **Unknown config keys are ignored, not rejected.** The config structs have
  no `deny_unknown_fields`. A misplaced key silently falls back to its default.
  For example, `https_bind_addr` and `quic_bind_addr` belong under `[tls]`
  (`src/main.rs:394-457`), `key_cache_capacity` must stay above `[tls]`
  (`src/main.rs:150`), and the per-client limit is `[limits.client.rx]`
  (`src/main.rs:522-534`, test at 796-800).
- **The key cache is off.** By default the relay keeps an LRU cache of up to
  1,048,576 endpoint IDs parsed from relayed frames, with no time limit
  (`src/defaults.rs:23`, `src/server.rs:728-730`, `src/key_cache.rs:50-66`),
  so IDs would stay in memory after their connection ends.
  `key_cache_capacity = 0` disables it (`src/key_cache.rs:37-47`), and each
  frame's destination key is parsed afresh. In the local test below that
  cost about 10 to 20 percent more relay CPU for the same transfer.
- **Only the per-client receive limit is implemented.** `accept_conn_limit`
  and `accept_conn_burst` parse but have no effect (`src/server.rs:486-499`).
  `nftables.conf` limits new connections per source address instead.
- **Metrics default to every interface.** Without `metrics_bind_addr`, the
  metrics server binds the `http_bind_addr` IP on port 9090 (`src/main.rs:344-347`).
- **Graceful shutdown needs SIGINT.** The binary waits only for Ctrl-C
  (`src/main.rs:605-611`), so the unit and `fly.toml` send SIGINT.
- **Dual stack.** `[::]` binds serve IPv4 and IPv6 (TCP with Linux's default
  `bindv6only=0`; UDP because noq 1.2.0 clears `IPV6_V6ONLY`,
  `noq src/endpoint.rs:129-135`).
- **ACME retries quickly at first.** After a failed order the client waits
  `1 << n` seconds, up to `1 << 16` (about 18 hours;
  `tokio-rustls-acme src/state.rs:392-393`). Let's Encrypt allows 5 failed
  authorizations per hostname per account per hour, refilling one every 12
  minutes ([rate limits](https://letsencrypt.org/docs/rate-limits/)). A relay
  started before DNS points at it uses those up within a minute, which is
  why `setup.sh` waits for DNS (see "DNS"). Renewal starts at two thirds of
  the certificate lifetime (`src/state.rs:214`).
- **A denylist access mode exists.** `access.denylist = ["<endpoint id>"]`
  refuses those endpoints and admits everyone else
  (`src/main.rs:166-167 and 260-272`). It is read at start, and an entry that
  is not a valid key stops the relay from starting. See "Abuse blocking".

## Prerequisites

- A VPS with Debian 12, Debian 13 or Ubuntu 24.04, x86_64 or arm64, a public
  IPv4 address and ideally IPv6. The relay needs well under 100 MB of RAM
  (about 9 MB resident in the tests below), so the smallest plan is enough.
- Your SSH public key, added in the provider's panel before creation.
- Access to the `oxfer.app` zone in Cloudflare DNS.
- A mailbox for `ops@oxfer.app`, the ACME contact in `config.toml`. iroh-relay
  refuses to start in LetsEncrypt mode without a contact (`src/main.rs:644-647`).
  Let's Encrypt stopped sending expiry emails on 4 June 2025 and no longer
  stores contact addresses with the account, so the uptime monitor below is
  what warns you about certificate problems.
- The provider's data-processing agreement accepted in its account settings
  and saved with the processor list (plan item A1).

## DNS

Create these in Cloudflare DNS with the proxy status **DNS only** (grey
cloud) and TTL 300 seconds, ideally before the server first boots. Where the
provider lets you reserve addresses first (Hetzner primary IPs, Scaleway
flexible IPs), reserve them, create the records, then create the server with
those addresses:

| Name | Type | Value |
| --- | --- | --- |
| `relay.oxfer.app` | A | the VPS IPv4 address |
| `relay.oxfer.app` | AAAA | the VPS IPv6 address (omit if the server has none) |

Why DNS only:

- Certificates: the relay obtains its certificate with TLS-ALPN-01 on port
  443. Cloudflare's proxy terminates TLS itself, so the challenge would never
  reach the relay and no certificate would be issued.
- QUIC address discovery runs on UDP 7842, which Cloudflare's HTTP proxy does
  not carry.
- The point of self-hosting is that relay traffic reaches only infrastructure
  under your control and your EU provider's DPA. A proxied hostname would add
  Cloudflare as a TLS-terminating intermediary on the relay path.

**The relay waits for DNS.** While the relay has no certificate yet,
`setup.sh` starts it only if public DNS for `relay.oxfer.app` returns only
addresses assigned to this host. It asks DNS directly and skips
`/etc/hosts`, which cloud images often fill with `127.0.1.1` for the
server's own name. Otherwise it leaves the relay stopped and disabled, and
prints what DNS returned and what the host has:

```
oxfer-relay-setup: relay NOT started: no certificate yet, and DNS for relay.oxfer.app gives [...], not only this host's [...]
```

That saves Let's Encrypt's failed-validation budget (see the ACME fact
above). Once `dig +short relay.oxfer.app A` and `AAAA` return this server's
addresses, run `systemctl enable --now iroh-relay` or rerun `setup.sh`. On a
host behind 1:1 NAT, where the public address is not on an interface, the
check can never match: start the relay by hand once DNS is right.

If the relay did run before DNS pointed at it, restarting it at once does
not help: Let's Encrypt refuses new orders for the name until a failure
slot refills, one every 12 minutes. Restart it about 12 minutes after DNS is
right, and look for `rateLimited` in `journalctl -u iroh-relay` if it still
fails.

Optional: a CAA record `relay.oxfer.app CAA 0 issue "letsencrypt.org"`
limits which CA may issue for this name. It affects only this hostname.

## Provisioning

### With cloud-init (providers with a user-data field)

1. Reserve the addresses if the provider allows it, and create the DNS records.
2. Create the server: Debian 12, Debian 13 or Ubuntu 24.04, your SSH key, and
   the whole of `cloud-init.yaml` pasted into the user-data field.
   - Hetzner Cloud: "Cloud config" field; the documented limit is 32 KiB
     (`cloud-init.yaml` is about 26 KB).
   - IONOS: Cloud Panel, Server, Create, Advanced options, "Cloud-Init User Data".
   - Scaleway: the cloud-init field when creating the Instance.
   - OVH: Public Cloud instances accept user data. Check the VPS order form;
     if it has no user-data field, use the manual path below.
3. Wait two to five minutes, then follow "First-boot verification". Progress:
   `ssh root@relay.oxfer.app tail -f /var/log/cloud-init-output.log`
   (log in as `debian` or `ubuntu` with `sudo` where the image has no root login).
   If DNS did not point at the server yet, the log ends with "relay NOT
   started"; see "DNS".

### Manual (no user-data field, or a rebuild)

Run from `p2p-transfer/deploy/relay/` in a checkout of this repository
(on images without root login, use the `debian` or `ubuntu` user and prefix
the remote commands with `sudo`):

```sh
tar -cf - . | ssh root@SERVER_IP \
  'mkdir -p /opt/oxfer-relay && tar -C /opt/oxfer-relay --no-same-owner -xf - && sh /opt/oxfer-relay/setup.sh'
```

`setup.sh` does the same as first boot:

1. Installs `ca-certificates`, `curl`, `iproute2`, `logrotate`, `nftables`
   and `unattended-upgrades`. Removes `rsyslog` and deletes the log files it
   leaves behind (`/var/log/syslog`, `auth.log`, `kern.log`, `mail.*`,
   `user.log`, `cron.log`, `ufw.log` and their rotations), so the journal is
   the only log store. Removes `libpam-lastlog2` and its database where
   present (Debian 13).
2. Downloads `iroh-relay-v1.1.0-<arch>-unknown-linux-musl.tar.gz` from the
   n0-computer/iroh GitHub release. It checks the pinned SHA-256 and the
   binary's `--version` output, then installs `/usr/local/bin/iroh-relay`. A
   mismatch aborts without installing anything.
3. Creates `/etc/oxfer-relay/denylist.txt` if missing and installs the relay
   config with that denylist merged in (see "Abuse blocking"). Installs the
   unit, journald and apt files. Replaces the packages' monthly or yearly
   logrotate entries for `wtmp`, `btmp` and `wtmp.db` with its own daily one,
   and empties those files the first time. Links `/var/log/lastlog` to
   `/dev/null`. Installs `oxfer-relay-ban` and its hourly timer.
4. Checks `nftables.conf`, together with `/etc/nftables.d/*.nft`, with
   `nft -c`. It reloads the firewall only when `/etc/nftables.conf` changed
   or its table is missing, and a reload loads the bans again from
   `bans.nft`.
5. Installs the key-only sshd drop-in, but only if an authorized key exists
   and `sshd -t` accepts the result.
6. Starts the relay if it has a certificate or DNS points here ("DNS"), then
   waits until `http://127.0.0.1/generate_204` answers 204.

It is idempotent. A second run changes nothing: it restarts the relay only
when the binary, the unit or the rendered config changed, and leaves the
firewall, its bans and the denylist as they are.

Optional: mirror the firewall in the provider's network firewall (Hetzner
Cloud Firewall, Scaleway security groups, OVH Network Firewall). To restrict
SSH to your own addresses, put the rules in `/etc/nftables.d/ssh.nft` on the
server, not in this repository (example in `nftables.conf`). The firewall
accepts DHCPv6 replies (UDP 547 to 546 between link-local addresses), which
providers that hand out IPv6 by DHCPv6 need, such as IONOS Cloud ("DHCP
assigns the first public IPv6 address",
[IONOS docs](https://docs.ionos.com/cloud/network-services/vdc-networking/ip-address/ipv6)).

## First-boot verification

From your machine:

```sh
# Captive-portal endpoint on port 80: expect 204 and "x-iroh-response: response probe-1".
curl -si -H 'X-Iroh-Challenge: probe-1' http://relay.oxfer.app/generate_204 | head -3

# TLS and certificate: expect a Let's Encrypt issuer and a notAfter date.
openssl s_client -connect relay.oxfer.app:443 -servername relay.oxfer.app </dev/null 2>/dev/null \
  | openssl x509 -noout -issuer -subject -enddate

# HTTPS probes: expect 200, then {"status":"ok","version":"1.1.0",...}.
curl -s -o /dev/null -w '%{http_code}\n' https://relay.oxfer.app/ping
curl -s https://relay.oxfer.app/healthz

# WebSocket upgrade, as the relay client does: expect "HTTP/1.1 101" and
# "sec-websocket-protocol: iroh-relay-v1". Without a subprotocol the relay answers 400.
curl -si --http1.1 --max-time 3 \
  -H 'Connection: Upgrade' -H 'Upgrade: websocket' -H 'Sec-WebSocket-Version: 13' \
  -H 'Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==' -H 'Sec-WebSocket-Protocol: iroh-relay-v1' \
  https://relay.oxfer.app/relay | grep -a -i '^HTTP/\|sec-websocket-protocol'

# The same check assets/diagnostics.js runs in the browser (Node 22 or later):
node -e 'const u=process.argv[1]||"wss://relay.oxfer.app/relay",w=new WebSocket(u,["iroh-relay-v1","iroh-relay-v2"]);w.onopen=()=>{console.log("OK",u,w.protocol);process.exit(0)};w.onerror=()=>{console.log("FAIL",u);process.exit(1)};setTimeout(()=>{console.log("TIMEOUT",u);process.exit(2)},10000)'
```

If the TLS checks fail while `/generate_204` works, the certificate is not
there yet. See "Troubleshooting".

On the server:

```sh
systemctl status iroh-relay nftables oxfer-relay-ban.timer --no-pager
journalctl -u iroh-relay -n 50 --no-pager   # expect nothing but start/stop lines
ss -tulpn | grep iroh-relay                 # :80, :443, udp :7842, 127.0.0.1:9090
curl -s http://127.0.0.1:9090/metrics | grep '^relayserver_' | head
nft list ruleset | head -40
oxfer-relay-ban list                        # empty on a new server
ls -l /var/log/lastlog                      # a link to /dev/null
ls /etc/logrotate.d/                        # oxfer-relay; no wtmp, btmp or wtmpdb
systemd-analyze security iroh-relay.service | tail -1
```

After switching production (next section), open `https://oxfer.app/diags`.
Diagnostics should list `Relay wss://relay.oxfer.app/relay: WebSocket opened`,
and a paired diagnostic session should register with the relay.

## Switching production to the relay

Production builds use n0's public relays while the repository variable
`P2P_RELAY_URL` is unset. The browser build reads it at compile time
(`option_env!` in `src/node.rs`). A non-empty value selects
`RelayChoice::Custom`, which uses iroh's `presets::Minimal`: no n0 relay map,
and no pkarr publishing or lookup at `dns.iroh.link`. The deploy job also
renders the CSP `connect-src` from the same variable.

No deploy succeeds until every placeholder in the legal pages is filled:
`build-web.sh` refuses to package while any `[[...]]` token remains, and
once they are filled it refuses to package unless `P2P_RELAY_URL` lists
`https://relay.oxfer.app`
([relay go-live guard](../../docs/cloudflare-workers.md#relay-go-live-guard)).
Filling the pages and setting the variable are therefore one step.

1. Complete "First-boot verification".
2. Check the relay from the app with a local build, which is never
   deployed. From `p2p-transfer/`:
   ```sh
   OXFER_ALLOW_PLACEHOLDERS=1 P2P_RELAY_URL=https://relay.oxfer.app npm run build
   ```
   `OXFER_ALLOW_PLACEHOLDERS=1` lets it package the pages with their
   placeholders; never deploy that output. `P2P_RELAY_URL=https://relay.oxfer.app trunk serve`
   gives a debug build at `http://127.0.0.1:8080` whose **Diags** page probes
   the relay.
3. Set the variable, in GitHub under Settings, Secrets and variables, Actions,
   Variables, or with the CLI:
   ```sh
   gh variable set P2P_RELAY_URL --repo radumarias/syncoxiders --body https://relay.oxfer.app
   ```
4. Push the commit that fills every placeholder (owner actions 1 and 3 in
   [`docs/compliance/README.md`](../../docs/compliance/README.md#owner-actions)).
   That push deploys, and it is the first deploy that can succeed. If the
   pages were filled before the variable was set, that deploy failed at the
   relay go-live guard; re-run it now: Actions, `oxfer-web`, "Run workflow"
   on `main`, or
   `gh workflow run oxfer-web.yml --repo radumarias/syncoxiders --ref main`.
5. When the deploy job's own verification has passed (it checks that the
   served policy names the relay in `connect-src`), check
   `https://oxfer.app/diags` and do one real transfer between two different
   networks.

Share links created before the switch name the relay that was current when
they were created. Let in-flight transfers finish, or share again after the
switch.

## Rollback

Returning production to n0's relays takes a commit and the variable, which
must change together
([relay selection](../../docs/cloudflare-workers.md#relay-selection-p2p_relay_url)):

1. **The rollback commit.** Once the pages are filled, `build-web.sh`
   refuses to package unless `P2P_RELAY_URL` lists `https://relay.oxfer.app`,
   whatever the pages say
   ([relay go-live guard](../../docs/cloudflare-workers.md#relay-go-live-guard)).
   The guard does not read the page text. The rollback commit must:
   - edit `privacy.html`, and any other page or record that names
     `relay.oxfer.app` (in `docs/compliance/` too), to describe n0's relays;
   - change or remove the relay go-live guard: `operator_relay` in
     `build-web.sh`, between the `# BEGIN legal-page guards` and
     `# END legal-page guards` markers, and `OPERATOR_RELAY` in
     `package-cf-output.mjs`;
   - update its test in `tests/web-pages.test.mjs`, which fails when
     `privacy.html` stops naming `relay.oxfer.app`.
2. **The variable.** Delete it before the commit reaches `main`:
   ```sh
   gh variable delete P2P_RELAY_URL --repo radumarias/syncoxiders
   ```
   Then push the commit to `main`, which deploys, or run
   `gh workflow run oxfer-web.yml --repo radumarias/syncoxiders --ref main`
   after it has landed. If the commit deployed while the variable still
   listed the relay, production would keep using it while the pages
   describe n0's relays.

If the deploy runs with the variable deleted while the pages are filled and
the guard is unchanged, it fails at the guard and production stays on the
self-hosted relay. Deleting the variable alone therefore rolls nothing back.
The VPS can keep running while you investigate.

## Upgrades

**Operating system:** automatic. `unattended-upgrades` installs security
updates daily and reboots at 03:30 (server time, usually UTC) when an update
requires it; clients reconnect within about a minute. Bans come back from
`bans.nft` at boot, and the denylist is part of the installed config. Check
`/var/log/unattended-upgrades/` monthly.

**Relay:** keep it on the same version as `iroh` in the workspace
`Cargo.lock`. Upgrade when that version moves, when n0 publishes a security
advisory, or at the quarterly review. The relay protocol is versioned (the
client offers `iroh-relay-v1` and `iroh-relay-v2`, and the server picks the
newest it supports, `src/server/http_server.rs:612-622`). Still upgrade the
relay and the app together.

1. Download `iroh-relay-vX.Y.Z-x86_64-unknown-linux-musl.tar.gz` and
   `iroh-relay-vX.Y.Z-aarch64-unknown-linux-musl.tar.gz` from
   `https://github.com/n0-computer/iroh/releases/tag/vX.Y.Z`, run `sha256sum`
   on both, and compare with the digests GitHub shows next to each asset. Put
   the version and both hashes at the top of `setup.sh`.
2. Read the new `src/main.rs` config structs and diff them against
   `config.toml`. Unknown keys are ignored silently, so a renamed key (for
   example `key_cache_capacity` or `access`) does not produce an error. Check
   that the new version still accepts `access.denylist` and parses it at
   start, which `setup.sh`'s denylist check relies on.
3. For Fly, update the digest in `fly/Dockerfile` from
   `docker buildx imagetools inspect n0computer/iroh-relay:vX.Y.Z` (the
   "Digest" line of the index).
4. Run `sh render.sh`, commit, and copy the kit to the server and run
   `setup.sh` with the same `tar ... | ssh` command as in "Manual". The relay
   restarts once and connections drop for a few seconds. The denylist stays
   merged into the new config, and the bans stay loaded: the firewall is
   reloaded only if `nftables.conf` changed, and a reload reads `bans.nft`.
5. Repeat "First-boot verification", check `/healthz` reports the new
   version, and check `oxfer-relay-ban list` and the `access.denylist` block
   in `/etc/iroh-relay/config.toml`.

When this kit was written (30 September 2026), `n0computer/iroh-relay` also
had tags `v1.2.0` and `v1.3.0`. The workspace is on 1.1.0, so the kit pins 1.1.0.

## Abuse blocking

The relay forwards end-to-end encrypted traffic. It cannot see file contents
or share links, and it logs no addresses or endpoint IDs (see "What is
logged"). There are two ways to block, and both survive reboots, firewall
reloads, `setup.sh` reruns and upgrades. Record every block in the private
abuse log, not here: the date and time, the kind of report, the endpoint ID
or address blocked, and the outcome. Commands typed in an interactive shell
on the host also stay in its shell history (see "What is logged").

### A reported share: by endpoint ID

This is the take-down the abuse page and the incident runbook describe.
Every share has its own endpoint key, and a browser sender can be reached
only through the relay. Once the relay refuses that endpoint, new
recipients can no longer reach that share. It does not stop the same person's
later shares, which use new keys. A native sender's link also carries its
IP addresses, so a recipient that can reach those directly bypasses the
relay; the address block below is the remaining measure there.

1. **Get the endpoint ID.** The report carries the link, or the part of it
   that the abuse page asks for, which starts with `endpoint` and stops
   before `&cap=`. The ID is encoded inside that ticket. Do not open the
   link; decode it offline on your own machine, from `p2p-transfer/` in a
   checkout of this repository:
   ```sh
   cargo run -q -p p2p-transfer --example ticket-endpoint-id -- 'endpointaa...'
   ```
   It accepts the whole link, its fragment or the bare ticket, and prints one
   line: the endpoint ID as 64 lowercase hex digits, the form iroh-relay
   1.1.0 reads in `access.denylist`. It never prints the capability after
   `cap=`. The `-q` matters: without it, cargo repeats the command line, and
   with it anything you passed, in its "Running" line. Passing only the
   ticket keeps the capability out of your shell history too; `-` instead of
   the argument reads the input from standard input.
2. **Add it on the server** and apply:
   ```sh
   echo '<endpoint id>  # 2026-10-01 report 17' >> /etc/oxfer-relay/denylist.txt
   sh /opt/oxfer-relay/setup.sh --denylist
   ```
   The file takes one ID per line; `#` starts a comment. `--denylist` checks
   every entry with the relay binary itself (a throwaway relay on a loopback
   port parses the list), then installs the config with
   `access.denylist = [...]` in place of `access = "everyone"` and restarts
   the relay. All relay connections drop for a few seconds and reconnect;
   the denied endpoint cannot. If an entry is not a valid endpoint ID,
   nothing changes and the relay keeps running. If the relay does not come
   back after a change, the previous config is put back.
3. **Lift it** by deleting the line and running `setup.sh --denylist` again.

A full `setup.sh` run merges the same file, so upgrades, reruns and the
unattended reboots keep the denylist. On Fly, see "Fly.io variant".

### An address: IP bans

Use this for an address in a report or one seen live
(`ss -tn state established '( sport = :443 )'` lists the peers connected
right now; there is no history). A ban drops everything from the address,
including connections that are already open.

```sh
oxfer-relay-ban add 203.0.113.7                  # 30 days
oxfer-relay-ban add 2001:db8:1:2::/64 90         # a prefix, 90 days
oxfer-relay-ban list                             # address and expiry (UTC)
oxfer-relay-ban del 203.0.113.7
```

Every ban expires. `DAYS` is a whole number from 1 to 365, and there is no
permanent ban. To extend a ban, run `oxfer-relay-ban add ADDRESS DAYS` again:
the new expiry counts from then. Record the extension in the abuse log like
the ban itself.

`ADDRESS` must be an address or prefix literal. A host name is refused:
nft would resolve it every time it loads `bans.nft`, which fails at boot,
before the network is up, and leaves the host with no ruleset. `del` matches
the line as text, so give it the spelling `list` shows.

`oxfer-relay-ban` writes the element, with its absolute expiry, to
`/etc/nftables.d/bans.nft` (mode 0600), which `nftables.conf` includes, and
adds it to the `banned_v4` or `banned_v6` set with a timeout. It puts the
new file in place, checks the whole ruleset with `nft -c`, and puts the old
file back if that fails, all before it changes the loaded set, so a refused
change leaves both as they were. `setup.sh` takes the tool's lock while it
checks and loads the ruleset. The kernel
lifts a ban at its timeout. After a reload or reboot the line is loaded again
with the timeout it was written with, so `oxfer-relay-ban.timer` runs
`oxfer-relay-ban prune` hourly to delete lines past their expiry, and their
elements. A ban therefore ends at its expiry, or at most about an hour later
if the firewall was reloaded in between. The timer logs only how many bans
it lifted. A ban line in `bans.nft` without an expiry, which the tool never
writes, counts as expired: `list` marks it and the next `prune` lifts it. Do
not use a bare `nft add element`: such a ban is gone after the next reload.

## Monitoring

- **Uptime and certificate:** an external monitor (any free uptime service)
  checking `https://relay.oxfer.app/healthz` for status 200 and the text `"ok"`,
  plus certificate expiry, alerting by email. Let's Encrypt no longer sends
  expiry emails, so this is the only certificate warning.
- **Relay path (plan item A10):** the WebSocket check above (`node -e ...`),
  which opens `wss://relay.oxfer.app/relay` with the `iroh-relay-v1`
  subprotocol, the same check the Diags page runs. Run it from a machine or
  a scheduled job if your monitor cannot do WebSockets with a subprotocol.
- **Metrics:** aggregate counters on `127.0.0.1:9090`, reachable through SSH:
  `ssh -L 9090:127.0.0.1:9090 root@relay.oxfer.app`, then
  `curl -s localhost:9090/metrics | grep '^relayserver_'`. Useful series:
  `relayserver_accepts_total`, `relayserver_disconnects_total`,
  `relayserver_bytes_sent_total`, `relayserver_bytes_recv_total`,
  `relayserver_http_connections_errored_total`, `relayserver_qad_connections_total`.
- **Traffic:** the provider's traffic graph monthly. On plans with a quota,
  set the provider's alert at 80 percent.

## What is logged

Everything on the relay host that holds an IP address or an endpoint ID,
and how long it lasts:

| Where | What | Retention |
| --- | --- | --- |
| Relay log (journal) | Startup failures, ACME and certificate errors, relay task failures. One line per malformed HTTP request on port 80, with the parser error and no address. No client addresses, endpoint IDs or connection events. | Three days (`MaxRetentionSec=3day`, daily journal files, 200 MB cap) |
| Relay memory | The address and endpoint ID of each connected client. The key cache is off (`key_cache_capacity = 0`), so no endpoint ID stays after its connection ends. | While connected |
| Relay metrics | Aggregate counters in memory, on 127.0.0.1 only | Until restart |
| Rate-limit sets in the firewall (`relay_flood_*`, `ssh_flood_*`) | Source address (IPv6: its /64) of recent new connections to TCP 22, 80 and 443 | At most 60 seconds: each element times out one minute after it is created, and `add` never extends it |
| Ban sets and `/etc/nftables.d/bans.nft` | Addresses you banned, with expiry | Until the expiry, or until lifted. Every ban has one: 30 days by default, 1 to 365 days when given, and a later `add` of the same address sets a new one |
| `/etc/oxfer-relay/denylist.txt` and the relay config | Endpoint IDs you blocked | Until you remove them |
| sshd (journal) | Source address and user of SSH logins and failed attempts: administration of the host, not relay users | Three days |
| sudo (journal), on images where you log in as `debian` or `ubuntu` | Each command run with `sudo`, with its arguments, so the address or endpoint ID of a block command run that way | Three days |
| Login records: `/var/log/wtmp`, `/var/log/btmp`, and `/var/log/wtmp.db` on Debian 13 | Logins (user, terminal, source address). Debian 12 and Ubuntu 24.04 also write unknown-user attempts to `btmp`. | Emptied at each daily rotation (between 00:00 and 01:00, and at boot after downtime), so about a day; never more than three days |
| `lastlog` and `lastlog2` | Nothing: `/var/log/lastlog` is a link to `/dev/null`, and `libpam-lastlog2` is removed where present | None |
| rsyslog files (`/var/log/syslog`, `auth.log` and the rest) | None: `rsyslog` is removed and its old files deleted | None |
| Firewall log | Nothing: no rule logs | None |
| Shell history (`~/.bash_history` of root and of the `debian` or `ubuntu` user) | The commands typed in interactive shells on the host, including any address or endpoint ID in a block command typed there. The kit does not change bash's defaults. | No time limit: bash trims the file only by number of lines |
| Certificate Transparency logs | The certificate for `relay.oxfer.app`, published by Let's Encrypt as all public certificates are | Permanent, public |

sshd logs `lastlog_openseek: /var/log/lastlog is not a file or directory!`
once per login: that is the `/dev/null` link, and expected.

How the relay log policy is enforced: the relay has no access-log option; its
only log control is `RUST_LOG` (`src/main.rs:568-571`). Tested with 1.1.0:

- At `info`, every failed connection is logged with the client's address and
  port, because connection handling runs inside
  `info_span!("conn", peer = %peer_addr)` (`src/server/http_server.rs:499`).
- At `warn`, the address disappears, but each failed or probed connection
  (an uptime probe, a scanner, a TLS error) still writes one line.
- The filter in `iroh-relay.service` keeps the targets that report failures
  of the server itself and turns off the per-connection ones. A span-scoped
  filter such as `[conn]=off` cannot do this: tracing-subscriber's EnvFilter
  enables an event if a span directive *or* a target directive allows it
  (`tracing-subscriber-0.3.23 src/filter/env/mod.rs:498-540`).
- A refused (denylisted) connection fails in the handshake
  (`src/protos/handshake.rs:495-501`, called at
  `src/server/http_server.rs:878`), and the error is logged at
  `src/server/http_server.rs:643` under `iroh_relay::server::http_server`,
  a target the filter turns off.

Do not raise `RUST_LOG` on production except briefly while debugging. If you
do, clear the journal afterwards with `journalctl --rotate && journalctl --vacuum-time=1s`.

## Yearly rebuild rehearsal

A rebuild is also the disaster-recovery procedure. The relay's state is its
certificate, which it obtains again, plus the denylist and the bans, which
you carry across. Once a year, at a low-traffic time:

1. The day before, confirm the DNS TTL is 300 seconds.
2. Create a new server with `cloud-init.yaml` (or the manual path). DNS still
   points at the old server, so `setup.sh` leaves the new relay stopped
   ("relay NOT started" in the log). That is intended: an ACME attempt now
   would fail.
3. Copy the server-side state from the old server to the new one, through
   your machine and never into this repository:
   ```sh
   mkdir relay-state
   scp -p root@OLD:/etc/oxfer-relay/denylist.txt 'root@OLD:/etc/nftables.d/*' relay-state/
   scp -p relay-state/denylist.txt root@NEW:/etc/oxfer-relay/
   scp -p relay-state/*.nft root@NEW:/etc/nftables.d/   # skip if there were none
   ssh root@NEW 'systemctl reload nftables && sh /opt/oxfer-relay/setup.sh --denylist && oxfer-relay-ban list'
   rm -r relay-state
   ```
   `scp -p` keeps the files' 0600 mode.
4. Point the A and AAAA records at the new server. Wait at least five minutes
   (the TTL) and until `dig +short relay.oxfer.app A` and `AAAA` return the
   new addresses, then on the new server run
   `systemctl enable --now iroh-relay`. Its first certificate request should
   succeed. Let's Encrypt allows 5 certificates per exact hostname set per 7
   days, and 5 failed validations per hostname per account per hour, so do
   not loop on failures.
5. Run "First-boot verification" and one real transfer.
6. Copy any denylist or ban added on the old server since step 3, then
   delete the old server. Note the date and the time taken in the ops notes.

## Fly.io variant

**The legal pages and the compliance records describe the VPS variant.**
Deployed on Fly, the relay would make these statements false:

- **IP blocking and per-address limits.** With raw TCP passthrough the relay
  sees Fly's proxy address, not the client's
  ([Fly services](https://docs.fly.io/networking/services/): without
  handlers Fly "just forward[s] TCP to your app as-is"; only the
  `proxy_proto` handler passes the client IP, and iroh-relay does not parse
  PROXY). Fly's services documentation describes no IP deny list, and there
  is no host firewall, so
  neither `oxfer-relay-ban` nor the per-address connection limits of
  `nftables.conf` exist there. Endpoint-ID blocks still work (below).
- **Log retention.** The relay's output goes to Fly's log pipeline: "Fly.io
  app logs are the stdout from the processes run in your apps", and its log
  search "retains logs for 7 days"
  ([Fly logging](https://docs.fly.io/monitoring/logging-overview/)). The
  three-day journal, the host sshd and its login records do not exist there.
- **Processor and transfer.** Fly.io, Inc. is a US company, and its edge
  proxies handle every connection before the machine does, not necessarily
  in the EU. The privacy notice and the records would have to name it as
  the relay's processor, with its DPA and a transfer basis (Data Privacy
  Framework or Standard Contractual Clauses), as they do for Cloudflare.

Before using Fly for production, change all of the following; this is the
complete list:

- `privacy.html`: section 3 ("What data is processed, by whom and why": the
  relay, its blocking and log retention) and section 8 ("International
  transfers").
- `terms.html`: 5.2 ("Child sexual exploitation and abuse content": blocking
  the network addresses involved), 7.1 ("What we can do": IP blocks) and 7.3
  ("Repeat infringers": address blocks).
- `abuse.html`: the summary ("In short": blocking an address at the relay),
  section 3 ("What happens next": a block of network addresses as an
  outcome), section 4 ("What we can do, and how blocking works": IP blocks
  and connection limits), section 7 ("Copyright and other rights": blocking
  repeat infringers' addresses) and 9.1 ("What we hold": relay logs and
  blocked addresses).
- `docs/compliance/ropa.md`: 2.1 (relay logging), 3.2 (P3, rate limits and
  blocking), 4 (processors), 5 (international transfers) and 6 (retention).
- `docs/compliance/transparency.md`: sections 1 (what exists per user) and 2
  (what the operator can provide: address blocks).
- `docs/compliance/osa-illegal-content.md`: ICU C2 in section 5.1 (take-down
  by address blocks) and its "Configuration assessed" note.
- `docs/compliance/README.md`: owner action 3.

The files in `fly/` implement the eight conditions of plan section 4: one
machine, no auto-stop, raw TCP on 443 and 80 without handlers, a volume for
the certificate, `SIGINT` on stop, and QUIC address discovery available but
off. The image is the official `n0computer/iroh-relay:v1.1.0` pinned by
digest. Its `/iroh-relay` is byte-identical to the GitHub musl binary that
`setup.sh` installs, and `fly/config.toml` is the same config, key cache
off included.

```sh
cd p2p-transfer/deploy/relay/fly
fly auth login
fly config validate --strict           # needs the login
fly apps create oxfer-relay-eu
fly volumes create relay_certs --region fra --size 1 -a oxfer-relay-eu
fly ips allocate-v4 -a oxfer-relay-eu  # dedicated IPv4, billed monthly; needed for raw TCP and UDP
fly ips allocate-v6 -a oxfer-relay-eu
fly ips list -a oxfer-relay-eu         # release any shared v4 so DNS cannot point at it
# DNS: A -> the dedicated IPv4, AAAA -> the IPv6, both DNS only.
fly deploy --ha=false                  # --ha=false: exactly one machine
fly machine list -a oxfer-relay-eu     # must show one machine
fly logs -a oxfer-relay-eu
```

- Always pass `--ha=false`. `fly deploy` creates spare machines by default,
  and two machines behind one hostname would split the peers of a transfer.
- Do not run `fly certs add`. That is for Fly-terminated TLS; here the relay
  terminates TLS and runs ACME itself.
- **Endpoint-ID blocks** come from the Fly secret `OXFER_RELAY_DENYLIST`,
  so the IDs stay out of the public image. Separate IDs with spaces or
  commas. Setting the secret restarts the machine, and the entrypoint
  replaces `access = "everyone"` with the list:
  ```sh
  fly secrets set OXFER_RELAY_DENYLIST="<endpoint id> <endpoint id>" -a oxfer-relay-eu
  fly logs -a oxfer-relay-eu   # "2 denylisted endpoint IDs"
  ```
  An entry that is not 64 hex digits (or 52 base32 characters) stops the
  machine from starting. The entrypoint does not run `setup.sh`'s full
  check, so use the IDs the `ticket-endpoint-id` example prints.
- QUIC address discovery (native clients only): set `OXFER_RELAY_QAD = "true"`
  in `fly.toml`, uncomment the UDP service, and redeploy. The entrypoint
  resolves `fly-global-services` and binds UDP 7842 there. Fly carries UDP only
  on the dedicated IPv4, not on IPv6.
- Upgrades: update the digest in `fly/Dockerfile` (see "Upgrades") and redeploy.

## Troubleshooting

- **No certificate** (TLS errors while `/generate_204` works): check that
  both records resolve to this server and are DNS only
  (`dig +short relay.oxfer.app A` and `AAAA`), and that TCP 443 is open in any
  provider firewall. Then watch `journalctl -u iroh-relay -f` for ACME
  errors. If they say `rateLimited`, earlier attempts used up Let's
  Encrypt's failed-validation budget: wait about 12 minutes, then
  `systemctl restart iroh-relay`. Otherwise restart at once to reset the
  backoff. To debug without using up production limits, set
  `prod_tls = false` in the kit's `config.toml` in `/opt/oxfer-relay/` (Let's
  Encrypt staging), run `setup.sh`, and set it back once it works.
  `/etc/iroh-relay/config.toml` is generated; `setup.sh` overwrites edits
  there.
- **"relay NOT started" after `setup.sh`:** there is no certificate yet and
  DNS does not point at this host (see "DNS").
- **`setup.sh --denylist` says an entry is not a valid endpoint ID:** the
  line is mistyped, or it is a ticket rather than an ID. Decode the ticket
  with the `ticket-endpoint-id` example.
- **`Address family not supported by protocol (os error 97)` at start:** the
  kernel has IPv6 disabled (`ipv6.disable=1` on its command line), so `[::]`
  cannot be bound. Remove that option and reboot. Standard images of the four
  providers ship with the IPv6 stack enabled.
- **`Address already in use`:** another service holds 80 or 443. This host
  should run nothing else.
- **No IPv6 after a reboot:** check `ip -6 addr` and that the DHCPv6 rule is
  loaded (`nft list chain inet oxfer_relay input | grep 547`).
- **Files placed by hand in `/var/lib/iroh-relay`** must belong to the
  directory's owner (`chown -R --reference=/var/lib/private/iroh-relay /var/lib/private/iroh-relay`),
  because the unit runs as a dynamic user. The relay writes its own ACME
  files, so this only matters for manual restores.
- **SSH password prompts still offered:** `setup.sh` installs the key-only
  drop-in only when an authorized key exists. Add your key, then rerun
  `setup.sh`.

## How this kit was verified (30 September 2026)

Verified in a sandbox without IPv6 and without a public IP.

Relay binary and config:

- iroh-relay 1.1.0 built from crates.io (`--features server`), and the
  prebuilt GitHub musl binary, both reporting `iroh-relay 1.1.0`. SHA-256 of
  all four Linux assets computed from the downloads and matched against the
  digests GitHub lists. Docker Hub index digest confirmed against the
  registry; the image's amd64 binary is byte-identical to the GitHub asset.
- `config.toml` parses and starts in LetsEncrypt mode (ACME directory
  redirected to a dead local port through `IROH_RELAY_ACME_URL`, so Let's
  Encrypt was never contacted). The debug dump shows
  `key_cache_capacity: Some(0)`.
- A local copy with a test CA certificate (`cert_mode = "Manual"`, high
  ports): `/ping` 200, `/healthz` 200, `/generate_204` 204 on HTTP and 404 on
  HTTPS. `/relay` returned 101 with `iroh-relay-v1` when only v1 was
  offered, 101 with `iroh-relay-v2` when both were, and 400 with none. With
  the current config (key cache off), two iroh 1.1.0 endpoints with IP
  transports disabled exchanged 8 MiB each way through the relay, and a
  third endpoint's net report received QUIC address discovery, in three
  runs. The relay used 0.42 to 0.53 s of CPU per run, against 0.38 to 0.45 s
  with the default cache, and wrote no log line under the unit's filter.
- The denylist check: valid IDs keep the throwaway relay running, a
  mistyped ID and a 32-byte value that is not an Ed25519 key make it exit at
  once with "config must be valid toml".

`setup.sh`, in privileged systemd containers of Debian 12 (systemd 252),
Debian 13 (systemd 257) and Ubuntu 24.04 (systemd 255). rsyslog was installed
beforehand, with its own and planted old log files, and on Debian 13 also
`libpam-lastlog2`:

- The real download and SHA-256 check ran. The sandbox reaches GitHub
  through a TLS-intercepting proxy, whose CA was added to the containers'
  trust store for this.
- Removed rsyslog and deleted its files; removed `libpam-lastlog2`, its PAM
  line and its database. Deleted the packages' `wtmp`, `btmp` and `wtmpdb`
  logrotate files; `logrotate -d /etc/logrotate.conf` reports no error.
  Ubuntu's group-writable `/var/log` is handled by its global `su root adm`.
  A daily run, forced by backdating logrotate's state, emptied `wtmp` and
  `btmp`, and on Debian 13 `wtmp.db`, which sshd then created again, with
  their modes kept. `/var/log/lastlog` is a link to `/dev/null` after setup
  and after a restart. sshd then writes no lastlog and logs one
  `lastlog_openseek` line per login.
- DNS gate: with no record the relay stayed stopped and disabled. With a
  local DNS server answering the container's address, and `/etc/hosts`
  mapping the name to 127.0.1.1, it started. Debian used `getent -s dns`;
  Ubuntu, with systemd-resolved installed, used
  `resolvectl --synthesize=no`.
- `oxfer-relay-ban`: add (IPv4, IPv6 prefix with an uppercase spelling,
  re-add with a new expiry), refusal of a bad address or day count, list
  and del. Bans survived `systemctl reload nftables`,
  `systemctl restart nftables`, a `setup.sh` rerun and a container restart.
  An expired line was loaded, then removed from the file and the set by the
  timer's service, which logged only a count. The DHCPv6 rule is loaded.
- `setup.sh --denylist` merged two IDs, one written in uppercase, and
  restarted the relay, which answered. A bad key and a malformed line were
  refused with the config unchanged and the relay running. A full rerun kept
  the denylist, kept the same relay process, and did not reload the
  firewall. A changed `nftables.conf` caused one reload, not a restart, and
  the bans stayed. A config that parses but cannot start (`Manual`
  certificates with none present) was put back automatically, and the relay
  answered again. No line of the relay journal held an address or an
  endpoint ID.

Test-only changes: the proxy CA; `[::]` rewritten to `0.0.0.0` by a unit
drop-in, since the sandbox has no IPv6; ACME pointed at a dead port; and, for
the DNS gate, a local dnsmasq.

First boot from `cloud-init.yaml`, in systemd containers of Debian 12
(cloud-init 22.4.2), Debian 13 (25.1.4) and Ubuntu 24.04 (26.1) with the file
as NoCloud user data:

- All 12 files landed in `/opt/oxfer-relay/`, byte-identical to the kit, with
  the intended modes, and `runcmd` ran `setup.sh`.
- On Debian 12, with no DNS record, the relay was left stopped as designed,
  and `systemctl enable --now iroh-relay` then brought it up.
- On Debian 13 and Ubuntu 24.04, a placeholder certificate file stood in for
  an existing certificate, and the relay started and answered.
- The iroh-relay binary was pre-installed (the same GitHub asset). The only
  module failure was cloud-init's `locale` module on the minimal Debian 12
  image.
- `cloud-init schema` (cloud-init 26.1) reports the file valid.
  `sh render.sh --check` passes, and fails when a source, the YAML around the
  payloads or a payload is changed.

Other checks:

- `nftables.conf`: `nft -c` with nftables 1.0.9, and loaded in containers
  with 1.0.6, 1.1.3 and 1.0.9.
- `iroh-relay.service`, `oxfer-relay-ban.service` and `.timer`:
  `systemd-analyze verify`. Shell scripts: shellcheck 0.11.0 clean.
- `oxfer-relay-ban` without the former `permanent` option, run in a network
  namespace with nftables 1.0.9 (a copy with its paths pointed at a scratch
  directory): it refused `DAYS` of 0, 366, 030, 1.5, `permanent` and a
  20-digit number, and an add while the firewall was not loaded. It added an
  IPv4 address, an IPv6 prefix written in uppercase and a re-add with a new
  expiry, then listed and deleted them. A planted line without an expiry
  was listed as having none and lifted by `prune` together with an expired
  one, and a reload kept the remaining ban.
- The rate-limit sets' 60-second bound: in a network namespace, an element
  of a `dynamic, timeout` set under continuous traffic counted down and was
  created again at its timeout, never extended by `add`.
- `fly/`: `fly.toml` parses as TOML. The image built and ran locally with
  QAD off (no UDP socket) and on (UDP 7842 bound to the address given for
  `fly-global-services`), and fails closed when that name does not resolve.
  With `OXFER_RELAY_DENYLIST` set it rendered `access.denylist` and answered;
  a malformed value stops it. It stops cleanly on SIGINT.

Not verified: a real VPS or real provider image (package sets were taken
from the official Debian 12 and 13 genericcloud and Ubuntu 24.04 cloud-image
manifests: the Debian images ship neither rsyslog nor logrotate, Ubuntu ships
both), a real Let's Encrypt issuance, IPv6 traffic, DHCPv6 on a real IONOS
host (the rule loads but was not exercised), the timed midnight run of
`logrotate.timer` (simulated), a refused connection's log output (checked
in the source only), the arm64 binary at runtime, a Fly deployment, and
`fly config validate`, which needs a Fly login.
