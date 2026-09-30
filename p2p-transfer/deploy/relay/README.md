# Oxfer relay: deployment kit and runbook

This directory takes a blank EU VPS to a working iroh relay at
`https://relay.oxfer.app` in about ten minutes, on any provider that offers
Debian 12, Debian 13 or Ubuntu 24.04 (OVH, IONOS, Hetzner, Scaleway). A Fly.io
variant is in [`fly/`](fly/). It implements the server side of section 4
of [`../../docs/compliance-plan.md`](../../docs/compliance-plan.md) (items A1
to A3, A5, A10 and A11) and documents the production switch of item A6.

The relay is the official `iroh-relay` binary, version 1.1.0, which matches
the `iroh` version in the workspace `Cargo.lock`. Nothing is compiled on the
server: `setup.sh` installs the prebuilt release binary and checks its SHA-256.

## Files

| File | Installed as | Purpose |
| --- | --- | --- |
| `config.toml` | `/etc/iroh-relay/config.toml` | Relay configuration. Every key has a comment naming the iroh-relay 1.1.0 source lines it comes from. |
| `iroh-relay.service` | `/etc/systemd/system/iroh-relay.service` | Hardened systemd unit: dynamic user, only `CAP_NET_BIND_SERVICE`, read-only system, no per-connection logging. |
| `nftables.conf` | `/etc/nftables.conf` | Firewall: inbound dropped except SSH, TCP 80 and 443, UDP 7842 and essential ICMP; ban sets; per-source connection limits. |
| `journald-oxfer-relay.conf` | `/etc/systemd/journald.conf.d/zz-oxfer-relay.conf` | Journal capped at 200 MB and three days. |
| `sshd-oxfer-relay.conf` | `/etc/ssh/sshd_config.d/01-oxfer-relay.conf` | Key-only SSH. |
| `unattended-upgrades.conf` | `/etc/apt/apt.conf.d/52oxfer-relay` | Automatic security upgrades, reboot at 03:30 when needed. |
| `setup.sh` | `/opt/oxfer-relay/setup.sh` | Idempotent installer for all of the above. |
| `cloud-init.yaml` | provider "user data" | Generated: writes the kit to `/opt/oxfer-relay/` and runs `setup.sh` on first boot. |
| `render.sh` | not installed | Regenerates `cloud-init.yaml` and `fly/config.toml`; `--check` detects drift. |
| `fly/` | Fly.io | `fly.toml`, `Dockerfile`, `entrypoint.sh` and a generated copy of `config.toml`. |

Edit the source files, then run `sh render.sh`. Never edit `cloud-init.yaml`
or `fly/config.toml` by hand; `sh render.sh --check` fails when they are stale.

Do not commit anything host-specific here, such as your own IP addresses,
banned addresses or abuse-log entries. This repository is public. Those
belong on the server in `/etc/nftables.d/` and in a private abuse log.

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
  (`src/main.rs:394-457`), and the per-client limit is `[limits.client.rx]`
  (`src/main.rs:522-534`, test at 796-800).
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
- **ACME retries back off up to about 18 hours** (`1 << 16` seconds,
  `tokio-rustls-acme src/state.rs:392-393`). If the first attempts fail, fix
  the cause and restart the service to reset the backoff. Renewal starts at
  two thirds of the certificate lifetime (`src/state.rs:214`).
- **A denylist access mode exists.** `access.denylist = ["<endpoint id>"]`
  (`src/main.rs:166-167 and 260-272`). See "Abuse blocking".

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

If the addresses are known only after the server exists, add the records
as soon as they are, wait until `dig +short relay.oxfer.app` returns them,
then run `systemctl restart iroh-relay` on the server. The restart makes the
ACME client retry at once instead of waiting out its backoff.

Optional: a CAA record `relay.oxfer.app CAA 0 issue "letsencrypt.org"`
limits which CA may issue for this name. It affects only this hostname.

## Provisioning

### With cloud-init (providers with a user-data field)

1. Reserve the addresses if the provider allows it, and create the DNS records.
2. Create the server: Debian 12, Debian 13 or Ubuntu 24.04, your SSH key, and
   the whole of `cloud-init.yaml` pasted into the user-data field.
   - Hetzner Cloud: "Cloud config" field; the documented limit is 32 KiB
     (`cloud-init.yaml` is about 25 KB).
   - IONOS: Cloud Panel, Server, Create, Advanced options, "Cloud-Init User Data".
   - Scaleway: the cloud-init field when creating the Instance.
   - OVH: Public Cloud instances accept user data. Check the VPS order form;
     if it has no user-data field, use the manual path below.
3. Wait two to five minutes, then follow "First-boot verification". Progress:
   `ssh root@relay.oxfer.app tail -f /var/log/cloud-init-output.log`
   (log in as `debian` or `ubuntu` with `sudo` where the image has no root login).

### Manual (no user-data field, or a rebuild)

Run from `p2p-transfer/deploy/relay/` in a checkout of this repository
(on images without root login, use the `debian` or `ubuntu` user and prefix
the remote commands with `sudo`):

```sh
tar -cf - . | ssh root@SERVER_IP \
  'mkdir -p /opt/oxfer-relay && tar -C /opt/oxfer-relay --no-same-owner -xf - && sh /opt/oxfer-relay/setup.sh'
```

`setup.sh` does the same as first boot:

1. Installs `ca-certificates`, `curl`, `nftables` and `unattended-upgrades`,
   and removes `rsyslog`, so the journal is the only log store.
2. Downloads `iroh-relay-v1.1.0-<arch>-unknown-linux-musl.tar.gz` from the
   n0-computer/iroh GitHub release. It checks the pinned SHA-256 and the
   binary's `--version` output, then installs `/usr/local/bin/iroh-relay`. A
   mismatch aborts without installing anything.
3. Installs the config, unit, journald and apt files.
4. Checks `nftables.conf` with `nft -c` before loading it.
5. Installs the key-only sshd drop-in, but only if an authorized key exists
   and `sshd -t` accepts the result.
6. Enables and starts the relay, then waits until `http://127.0.0.1/generate_204`
   answers 204.

It is idempotent. A second run changes nothing and does not restart the
relay unless the binary, config or unit changed.

Optional: mirror the firewall in the provider's network firewall (Hetzner
Cloud Firewall, Scaleway security groups, OVH Network Firewall). To restrict
SSH to your own addresses, put the rules in `/etc/nftables.d/ssh.nft` on the
server, not in this repository (example in `nftables.conf`).

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
systemctl status iroh-relay nftables --no-pager
journalctl -u iroh-relay -n 50 --no-pager   # expect nothing but start/stop lines
ss -tulpn | grep iroh-relay                 # :80, :443, udp :7842, 127.0.0.1:9090
curl -s http://127.0.0.1:9090/metrics | grep '^relayserver_' | head
nft list ruleset | head -40
systemd-analyze security iroh-relay.service | tail -1
```

After switching production (next section), open `https://oxfer.app/diags`.
Diagnostics should list `Relay wss://relay.oxfer.app/relay: WebSocket opened`,
and a paired diagnostic session should register with the relay.

## Switching production to the relay

Production uses n0's public relays until the repository variable
`P2P_RELAY_URL` is set. The browser build reads it at compile time
(`option_env!` in `src/node.rs`). A non-empty value selects
`RelayChoice::Custom`, which uses iroh's `presets::Minimal`: no n0 relay map,
and no pkarr publishing or lookup at `dns.iroh.link`. The deploy job also
renders the CSP `connect-src` from the same variable.

1. Complete "First-boot verification".
2. Set the variable, in GitHub under Settings, Secrets and variables, Actions,
   Variables, or with the CLI:
   ```sh
   gh variable set P2P_RELAY_URL --repo radumarias/syncoxiders --body https://relay.oxfer.app
   ```
3. Re-run the deploy: Actions, `oxfer-web`, "Run workflow" on `main`, or
   `gh workflow run oxfer-web.yml --repo radumarias/syncoxiders --ref main`.
4. When the deploy job's own verification has passed, check `https://oxfer.app/diags`
   and do one real transfer between two different networks.

Share links created before the switch name the relay that was current when
they were created. Let in-flight transfers finish, or share again after the
switch.

## Rollback

Delete the variable and re-run the deploy:

```sh
gh variable delete P2P_RELAY_URL --repo radumarias/syncoxiders
gh workflow run oxfer-web.yml --repo radumarias/syncoxiders --ref main
```

The build returns to n0's relays. The VPS can keep running while you
investigate.

## Upgrades

**Operating system:** automatic. `unattended-upgrades` installs security
updates daily and reboots at 03:30 (server time, usually UTC) when an update
requires it; clients reconnect within about a minute. Check
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
   `config.toml`. Unknown keys are ignored silently, so a renamed key does not
   produce an error.
3. For Fly, update the digest in `fly/Dockerfile` from
   `docker buildx imagetools inspect n0computer/iroh-relay:vX.Y.Z` (the
   "Digest" line of the index).
4. Run `sh render.sh`, commit, and copy the kit to the server and run
   `setup.sh` with the same `tar ... | ssh` command as in "Manual". The relay
   restarts once and connections drop for a few seconds.
5. Repeat "First-boot verification" and check `/healthz` reports the new version.

When this kit was written (30 September 2026), `n0computer/iroh-relay` also
had tags `v1.2.0` and `v1.3.0`. The workspace is on 1.1.0, so the kit pins 1.1.0.

## Abuse blocking

The relay forwards end-to-end encrypted traffic. It cannot see file contents
or share links, and it logs no addresses (see "What is logged"). Blocking
therefore works on addresses you receive in a report or observe live.

- **By address, at the firewall.** Takes effect at once and also cuts open
  connections:
  ```sh
  nft add element inet oxfer_relay banned_v4 '{ 203.0.113.7 timeout 30d }'
  nft add element inet oxfer_relay banned_v6 '{ 2001:db8:1:2::/64 timeout 30d }'
  nft list set inet oxfer_relay banned_v4
  nft delete element inet oxfer_relay banned_v4 '{ 203.0.113.7 }'
  ```
  Runtime bans are lost on reboot or `systemctl reload nftables`. For a
  permanent ban, add `add element inet oxfer_relay banned_v4 { 203.0.113.7 }`
  to `/etc/nftables.d/bans.nft` on the server and reload.
- **By endpoint ID, in the relay.** iroh-relay 1.1.0 has a denylist access
  mode: replace `access = "everyone"` with
  `access.denylist = ["<endpoint id>"]` (`src/main.rs:166-167 and 260-272`).
  It is read at start, so every change restarts the relay. Oxfer uses a fresh
  endpoint key for every share, so this helps only against a long-lived
  endpoint, for example a native client that reuses its key. Keep such an
  edit on the server, not in this repository.
- **Seeing current connections:** `ss -tn state established '( sport = :443 )'`
  lists the peers connected right now. There is no history.
- Record every action (date, address or endpoint ID, reason, expiry) in the
  private abuse log, not in this repository.

## Monitoring

- **Uptime and certificate:** an external monitor (any free uptime service)
  checking `https://relay.oxfer.app/healthz` for status 200 and the text `"ok"`,
  plus certificate expiry, alerting by email. Let's Encrypt no longer sends
  expiry emails, so this is the only certificate warning.
- **Relay path:** the WebSocket check above (`node -e ...`) is the same one
  the Diags page runs (plan item A10). Run it from a machine or a scheduled
  job if your monitor cannot do WebSockets with a subprotocol.
- **Metrics:** aggregate counters on `127.0.0.1:9090`, reachable through SSH:
  `ssh -L 9090:127.0.0.1:9090 root@relay.oxfer.app`, then
  `curl -s localhost:9090/metrics | grep '^relayserver_'`. Useful series:
  `relayserver_accepts_total`, `relayserver_disconnects_total`,
  `relayserver_bytes_sent_total`, `relayserver_bytes_recv_total`,
  `relayserver_http_connections_errored_total`, `relayserver_qad_connections_total`.
- **Traffic:** the provider's traffic graph monthly. On plans with a quota,
  set the provider's alert at 80 percent.

## What is logged

| Where | What | Retention |
| --- | --- | --- |
| Relay (journal) | Startup failures, ACME and certificate errors, relay task failures. One line per malformed HTTP request on port 80, with the parser error and no address. No client addresses, endpoint IDs or connection events. | Three days (`MaxRetentionSec=3day`, daily rotation, 200 MB cap) |
| Relay metrics | Aggregate counters in memory, on 127.0.0.1 only | Until restart |
| sshd (journal) | Source address and user of SSH logins and failed attempts: administration of the host, not relay users | Three days |
| Firewall | Nothing: no rule logs | None |
| Certificate Transparency logs | The certificate for `relay.oxfer.app`, published by Let's Encrypt as all public certificates are | Permanent, public |

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

Do not raise `RUST_LOG` on production except briefly while debugging. If you
do, clear the journal afterwards with `journalctl --rotate && journalctl --vacuum-time=1s`.

## Yearly rebuild rehearsal

The relay holds no state beyond its certificate, which it obtains again, so
a rebuild is also the disaster-recovery procedure. Once a year, at a
low-traffic time:

1. The day before, confirm the DNS TTL is 300 seconds.
2. Create a new server with `cloud-init.yaml` (or the manual path).
3. Point the A and AAAA records at the new server, then on it run
   `systemctl restart iroh-relay` so it requests the certificate at once.
   Let's Encrypt allows 5 failed validations per hostname per account per
   hour, and 5 certificates per exact hostname set per 7 days, so do not
   loop on failures.
4. Run "First-boot verification" and one real transfer.
5. Delete the old server. Note the date and the time taken in the ops notes.

## Fly.io variant

Plan section 4 lists the eight conditions. The files in `fly/` implement
them: one machine, no auto-stop, raw TCP on 443 and 80 without handlers, a
volume for the certificate, `SIGINT` on stop, and QUIC address discovery
available but off. The image is the official `n0computer/iroh-relay:v1.1.0`
pinned by digest. Its `/iroh-relay` is byte-identical to the GitHub musl
binary that `setup.sh` installs.

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
- With raw TCP passthrough the relay sees Fly's proxy address instead of the
  client's. Fly's edge handles every connection, and its terms and DPA govern
  what it records.
- QUIC address discovery (native clients only): set `OXFER_RELAY_QAD = "true"`
  in `fly.toml`, uncomment the UDP service, and redeploy. The entrypoint
  resolves `fly-global-services` and binds UDP 7842 there. Fly carries UDP only
  on the dedicated IPv4, not on IPv6.
- Upgrades: update the digest in `fly/Dockerfile` (see "Upgrades") and redeploy.

## Troubleshooting

- **No certificate** (TLS errors while `/generate_204` works): check that
  both records resolve to this server and are DNS only
  (`dig +short relay.oxfer.app A` and `AAAA`), and that TCP 443 is open in any
  provider firewall. Then run `systemctl restart iroh-relay` to reset the
  backoff, and watch `journalctl -u iroh-relay -f` for ACME errors. To debug
  without using up production limits, set `prod_tls = false` in
  `/etc/iroh-relay/config.toml` (Let's Encrypt staging), restart, and set it
  back once it works.
- **`Address family not supported by protocol (os error 97)` at start:** the
  kernel has IPv6 disabled (`ipv6.disable=1` on its command line), so `[::]`
  cannot be bound. Remove that option and reboot. Standard images of the four
  providers ship with the IPv6 stack enabled.
- **`Address already in use`:** another service holds 80 or 443. This host
  should run nothing else.
- **Files placed by hand in `/var/lib/iroh-relay`** must belong to the
  directory's owner (`chown -R --reference=/var/lib/private/iroh-relay /var/lib/private/iroh-relay`),
  because the unit runs as a dynamic user. The relay writes its own ACME
  files, so this only matters for manual restores.
- **SSH password prompts still offered:** `setup.sh` installs the key-only
  drop-in only when an authorized key exists. Add your key, then rerun
  `setup.sh`.

## How this kit was verified (30 September 2026)

Verified in a sandbox without IPv6 and without a public IP:

- iroh-relay 1.1.0 built from crates.io (`--features server`), and the
  prebuilt GitHub musl binary, both reporting `iroh-relay 1.1.0`. SHA-256 of
  all four Linux assets computed from the downloads and matched against the
  digests GitHub lists. Docker Hub index digest confirmed against the
  registry; the image's amd64 binary is byte-identical to the GitHub asset.
- `config.toml` unchanged: parses, starts in LetsEncrypt mode (ACME directory
  redirected to a dead local port through `IROH_RELAY_ACME_URL`, so Let's
  Encrypt was never contacted) and binds TCP 80 and 443, UDP 7842 and
  127.0.0.1:9090. The `[::]` binds were changed to `0.0.0.0` only because the
  sandbox kernel has no IPv6.
- A local copy with a test CA certificate (`cert_mode = "Manual"`, high
  ports): `/ping` 200, `/healthz` 200, `/generate_204` 204 on HTTP and 404 on
  HTTPS. `/relay` returned 101 with `iroh-relay-v1` when only v1 was
  offered, 101 with `iroh-relay-v2` when both were, and 400 with none. Two
  iroh 1.1.0 endpoints with IP transports disabled exchanged 8 MiB each way
  through the relay, and a third endpoint's net report received QUIC address
  discovery (`QadIpv4`) and HTTPS latency results.
- `setup.sh` run under systemd in privileged containers of Debian 12
  (systemd 252), Debian 13 (systemd 257) and Ubuntu 24.04 (systemd 255, with
  rsyslog pre-installed). All three:
  - Exit 0 and are idempotent (same main PID after a second run).
  - Run the relay as a dynamic UID with only `CAP_NET_BIND_SERVICE`,
    NoNewPrivs and a seccomp filter; `systemd-analyze security` gives 1.6.
  - Load the firewall and show the effective key-only sshd, journald and
    apt settings; rsyslog is removed.
  - With a test certificate, the relay answered through the container's
    firewall from outside: HTTPS, WSS, the iroh end-to-end transfer and QAD
    all worked, and port 9090 was unreachable.
  - After probes, scanners, TLS errors and an idle connection, the relay
    journal held no client address.

  Test-only changes were: the binary pre-installed (containers had no
  HTTPS egress), `[::]` changed to `0.0.0.0`, and a systemd drop-in pointing
  ACME at a dead port. The download-and-verify code of `setup.sh` was run
  separately. It installed on a correct hash, skipped when the version was
  present, and refused a wrong hash.
- First boot from `cloud-init.yaml`: systemd containers of Debian 12, Debian
  13 and Ubuntu 24.04 booted with the file as NoCloud user data, and
  cloud-init's own boot services ran it. The files landed in
  `/opt/oxfer-relay/` with the intended modes, `runcmd` ran `setup.sh`, and it
  finished with the relay active and answering. The only module failure was
  cloud-init's `locale` module on Debian 12, caused by the minimal container
  image, not by this file.
- `nftables.conf`: `nft -c` with nftables 1.0.9, loaded in a network
  namespace, runtime ban added. `iroh-relay.service`: `systemd-analyze
  verify`. `cloud-init.yaml`: valid YAML, byte-identical round trip of every
  embedded file, and `cloud-init schema` (cloud-init 26.1) valid. Shell
  scripts: shellcheck 0.11.0 clean.
- `fly/`: `fly.toml` parses as TOML. The image built and ran locally with
  QAD off (no UDP socket) and on (UDP 7842 bound to the address given for
  `fly-global-services`), and fails closed when that name does not resolve.
  It stops cleanly on SIGINT.

Not verified: a real VPS, a real Let's Encrypt issuance, IPv6 traffic, the
arm64 binary at runtime, a Fly deployment, and `fly config validate`, which
needs a Fly login.
