# Web UI cut-over: egui (Trunk) → Dioxus (`web/`)

The Dioxus UI (#67) builds and deploys next to the egui web build until it reaches parity
(phase 2 of #76: #68, #69 and the screens they cover). Production keeps serving egui until
every box below is ticked. Native desktop stays on egui until the native-wrapper decision.

## How the switch works

`build-web.sh` reads `OXFER_WEB_UI` (`egui` by default, or `dioxus`) and writes the same
`dist/` layout either way, so `package-cf-output.mjs`, `_headers`, `sw.js` and the Worker's
single-page fallback are unchanged. The deploy workflow passes the repository variable
`OXFER_WEB_UI`, so production can switch (and switch back) without a code change.

Try a production-shaped Dioxus build locally:

```sh
OXFER_WEB_UI=dioxus bash build-web.sh   # release bundle, size report, .cloudflare/output
npx cf deploy --prebuilt                # only for a preview Worker; see cloudflare-workers.md
```

## Before switching

- [ ] Parity: every flow in the egui web app works in Dioxus (send, receive with each sink
      `sink=fsa|sw|mem`, resume from local copies, chat host/guest/agent rooms,
      diagnostics peer test, received list, wake lock prompts).
- [ ] `agent/oxfer-agent.mjs` drives the DOM (buttons by role and name) instead of
      screenshots and `PRIMARY_BLUE`; `tests/agent_cli.test.mjs` updated.
- [ ] `assets/llms.txt`: drop "the page is a canvas … sees Loading…"; describe the DOM UI
      and the path routes; keep the fragment rules.
- [ ] `docs/agent-transfer.md`: replace the canvas/screenshot section with the DOM
      selectors; keep the `oxfer:chat` bridge section (unchanged in Dioxus).
- [ ] `verify-deployment.mjs` checks the Dioxus shell (`<div id="main">`, hashed
      `/assets/oxfer-web-dxh….js`) instead of `p2p-transfer_bg.wasm`.
- [ ] Lighthouse mobile: PWA and accessibility pass.
- [ ] First load on a mid-range Android over 4G measured before and after (CI's bundle-size
      summary gives the wasm size; budget 2 MiB brotli in `build-web.sh`).
- [ ] Service worker re-verified on a clean profile and on an upgrade from the egui build
      (#38, #43): `sw.js` cache `oxfer-v4` drops the old shell; hashed dx assets are
      cache-first, navigations network-first with the `/` shell as the offline fallback.

## Switch

1. Set the repository variable `OXFER_WEB_UI=dioxus` and run the `oxfer-web` workflow.
2. `verify-deployment.mjs` against the three production URLs.
3. Then, in a follow-up PR: make `dioxus` the default in `build-web.sh`, delete the Trunk
   path (`index.html`, `Trunk.toml`, `trunk build` in `check.sh` and the OSS scanner
   Dockerfile, `assets/keyboard-focus.js`, `assets/theme.js`), and move the egui-only web
   code in `src/app.rs` behind `cfg(not(target_arch = "wasm32"))`.

Rollback: set `OXFER_WEB_UI=egui` and rerun the workflow.
