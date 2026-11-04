// Human / Machine switch. "Machine" covers the canvas with the Markdown an agent gets
// for this page (`/index.md` or `/diags.md`, served by worker/index.mjs); "Human"
// returns to the app. The URL, including a share link's fragment, is never changed.
//
// Hidden under automation (navigator.webdriver): oxfer-agent.mjs clicks the canvas by
// screenshot, and a floating pill could sit over a button it needs.
(() => {
    if (navigator.webdriver) return;

    const css = `
#oxfer-view-toggle{position:fixed;left:50%;bottom:calc(104px + env(safe-area-inset-bottom));
  transform:translateX(-50%);z-index:20;display:flex;gap:2px;padding:4px;border-radius:999px;
  background:rgba(38,38,40,.94);border:1px solid rgba(255,255,255,.12);
  box-shadow:0 4px 18px rgba(0,0,0,.28);font:13px/1 ui-monospace,SFMono-Regular,Menlo,Consolas,monospace;
  -webkit-user-select:none;user-select:none}
#oxfer-view-toggle button{all:unset;cursor:pointer;display:flex;align-items:center;gap:7px;
  padding:7px 13px;border-radius:999px;color:#a3a3a8;letter-spacing:.02em}
#oxfer-view-toggle button::before{content:"";width:6px;height:6px;border-radius:50%;background:#6b6b70}
#oxfer-view-toggle button[aria-pressed="true"]{background:rgba(255,255,255,.1);color:#f4f4f5}
#oxfer-view-toggle button[aria-pressed="true"]::before{background:#ff6a3d}
#oxfer-view-toggle button:focus-visible{outline:2px solid #8ab4ff;outline-offset:1px}
#oxfer-machine-view{position:fixed;inset:0;z-index:10;overflow:auto;-webkit-overflow-scrolling:touch;
  overscroll-behavior:contain;background:#0b0b0c;color:#e4e4e7;
  font:14px/1.75 ui-monospace,SFMono-Regular,Menlo,Consolas,monospace}
#oxfer-machine-view[hidden]{display:none}
#oxfer-machine-view .inner{box-sizing:border-box;max-width:780px;margin:0 auto;
  padding:56px 16px calc(140px + env(safe-area-inset-bottom))}
#oxfer-machine-view .source{color:#8b8b93;margin:0 0 28px}
#oxfer-machine-view pre{margin:0;font:inherit;white-space:pre-wrap;overflow-wrap:anywhere}`;

    // Page → [Markdown view, its source file]. Mirrors PAGES in worker/index.mjs, which a
    // classic script cannot import; tests/markdown_worker.test.mjs checks they agree.
    const VIEWS = { "/diags": ["/diags.md", "/assets/diags.md"] };
    const INDEX = ["/index.md", "/llms.txt"];
    const viewFor = () => VIEWS[location.pathname.replace(/\/+$/, "")] ?? INDEX;

    async function loadMarkdown([path, source]) {
        const accept = { headers: { Accept: "text/markdown" }, cache: "no-cache" };
        const response = await fetch(path, accept);
        const type = response.headers.get("content-type") ?? "";
        if (response.ok && type.startsWith("text/markdown")) return response.text();
        // `trunk serve` has no Worker and answers with the HTML shell: show the source file.
        response.body?.cancel();
        const fallback = await fetch(source, accept);
        if (!fallback.ok) throw new Error(`HTTP ${fallback.status}`);
        return fallback.text();
    }

    function mount() {
        const style = document.createElement("style");
        style.textContent = css;
        document.head.append(style);

        const view = document.createElement("section");
        view.id = "oxfer-machine-view";
        view.hidden = true;
        view.setAttribute("aria-label", "Markdown version of this page");
        const inner = document.createElement("div");
        inner.className = "inner";
        const source = document.createElement("p");
        source.className = "source";
        const pre = document.createElement("pre");
        inner.append(source, pre);
        view.append(inner);

        const bar = document.createElement("div");
        bar.id = "oxfer-view-toggle";
        bar.setAttribute("role", "group");
        bar.setAttribute("aria-label", "View this page as");
        const human = document.createElement("button");
        human.type = "button";
        human.textContent = "Human";
        human.title = "The app, for people";
        const machine = document.createElement("button");
        machine.type = "button";
        machine.textContent = "Machine";
        machine.title = "The Markdown an AI agent gets for this page";
        bar.append(human, machine);

        let loaded = null;
        let generation = 0;
        async function show(asMachine) {
            human.setAttribute("aria-pressed", String(!asMachine));
            machine.setAttribute("aria-pressed", String(asMachine));
            view.hidden = !asMachine;
            const run = ++generation;
            if (!asMachine) return;
            const target = viewFor();
            const [path] = target;
            if (loaded === path) return;
            source.textContent = `${location.host}${path}`;
            pre.textContent = "Loading…";
            try {
                const text = await loadMarkdown(target);
                if (run !== generation) return;
                pre.textContent = text;
                loaded = path;
            } catch (error) {
                if (run === generation) pre.textContent = `Could not load ${path}: ${error.message}`;
            }
        }
        human.addEventListener("click", () => show(false));
        machine.addEventListener("click", () => show(true));
        document.addEventListener("keydown", event => {
            if (event.key === "Escape" && !view.hidden) show(false);
        });

        show(false);
        document.body.append(view, bar);
    }

    if (document.body) mount();
    else document.addEventListener("DOMContentLoaded", mount, { once: true });
})();
