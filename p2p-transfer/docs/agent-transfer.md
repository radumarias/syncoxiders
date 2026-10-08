# Scripted transfers for automation and AI agents

Oxfer's UI is an `egui` canvas. Nothing in it is reachable through the DOM, so
a script cannot find the "Send files" button by selector or read the share link
from a text node. `agent/oxfer-agent.mjs` drives the real app in a Playwright
browser instead, and the site serves it at `https://oxfer.app/oxfer-agent.mjs`
so an agent that only has the link can fetch it. `https://oxfer.app/llms.txt`
carries the short version of this page, and `index.html` points there from a
`<noscript>` block, which is what an HTTP-only client sees.

## Usage

```sh
npm ci                                 # installs playwright and pngjs
npm run agent -- send ./file.bin       # prints LINK https://oxfer.app/#…, keeps sharing
npm run agent -- recv "https://oxfer.app/#…" ./downloads
```

Outside the repository:

```sh
curl -fsSLO https://oxfer.app/oxfer-agent.mjs
npm init -y >/dev/null && npm i playwright@1.56.1 pngjs@7.0.0
node oxfer-agent.mjs send ./file.bin
```

The script launches Google Chrome through Playwright by default, then falls
back to Playwright's own Chromium (`npx playwright install chromium`). Pass
`--browser firefox` or `--browser webkit` to use those. Headless Chromium needs
SwiftShader for WebGL; the script adds those flags itself.

| Mode | Positionals | Flags | Stdout |
| --- | --- | --- | --- |
| `send` | one or more files | `--once`, `--count N`, `--link-file path` | `LINK <url>`, `SHARING <names>`, `SERVED <n>`, `COMPLETE <n>` |
| `recv` | share link, output dir (default `.`) | | `SAVED <path> <bytes> <sha256>`, `VERIFIED` |

Common flags: `--browser`, `--headed`, `--timeout <seconds>` (default 120),
`--origin <url>` for a local `trunk serve`, `--snap <dir>` to save a screenshot
after each step, `--verbose` to echo the app's console log to stderr.

Exit codes: `0` success, `1` usage error, `2` failure (a screenshot is saved to
`--snap` when given), `3` the receiver saved the file but saw no verification
receipt. `SERVED` means the sender finished writing the bytes; only `COMPLETE`
proves the receiver verified them, and `--once` waits for it. On the receiving
side, a `SAVED` line without `VERIFIED` is not success.

## How it works

- **Buttons** are found by colour. Every step's call to action is the only
  large primary-blue (`#1674d8`) box below the header, so the script screenshots
  the canvas, decodes the PNG, and clicks the centre of the largest such box.
  The header's own blue "Choose File" button is excluded by position.
- **Sending** clicks "Send files", answers the browser file chooser with the
  given paths, waits for the `ready to share` log line (hashing is done), clicks
  "Copy link", and reads the link from a `navigator.clipboard.writeText` hook
  installed before the app starts. The hook works in every Playwright browser;
  `clipboard-read` permission is only a fallback on Chromium.
- **Completion** on the sender is the `receiver verified …` log line, emitted
  when the receiver's verified receipt arrives. `serve complete` fires earlier,
  when the sender has merely finished writing, and is reported as `SERVED`.
- **Receiving** waits for the "Choose where to save" button, which only appears
  once the manifest arrived. The init script removes `showSaveFilePicker`
  because headless Chromium exposes the API but cannot show the dialog, which
  would hang the click. Oxfer then uses its service-worker streaming sink, which
  Playwright surfaces as a `download` event. The script saves every download,
  then waits for one `verified and saved` log line per file.
- **Share links** are validated with `classifyLink`: a fragment of
  `endpoint<base32>&cap=<32 hex>` is a share link, an empty fragment (or only
  `dev`/`sink=` flags) is the home page, `/diags` is diagnostics.

## How an agent finds out

An agent that is handed a bare `https://oxfer.app/` or a share link will
usually fetch it first. Both return the same shell, because the capability
lives in the fragment and never reaches the server. The shell therefore says
so itself:

1. `<meta name="description">` and a `<noscript>` block in `index.html` explain
   that the page is an app shell, how to tell a share link from the home page,
   and point at `/llms.txt`.
2. `/llms.txt` gives the rules and the four commands above.
3. `/oxfer-agent.mjs` is the script itself.

Tell agents directly when you can. A sending agent needs: run `send`, keep the
process alive, pass the `LINK` line on verbatim, exit after `COMPLETE`. A
receiving agent needs: run `recv` with the link in quotes (the `&` would
otherwise be a shell operator), treat `VERIFIED` and exit code 0 as done.

## Limits

- The receiver cannot resume a partial download through this script; rerun it.
- Multiple files per share work only if the browser's file chooser accepts
  several paths; Oxfer's picker currently takes one file.
- Firefox and WebKit have not been exercised as much as Chromium.
- Dark mode changes the button colour. The script forces a light colour scheme.
