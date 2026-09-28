import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import vm from "node:vm";

const read = path => readFile(new URL(path, import.meta.url), "utf8");
const source = await read("../assets/favicon.js");

for (const page of ["index.html", "theme.html"]) {
    test(`${page} advertises only the theme-aware tab icon`, async () => {
        const html = await read(`../${page}`);
        const icons = [...html.matchAll(/<link\b[^>]*>/g)]
            .map(([link]) => link)
            .filter(link => {
                const rel = link.match(/\brel="([^"]+)"/)?.[1] ?? "";
                return rel.split(/\s+/).includes("icon");
            });
        assert.equal(icons.length, 1, "a competing ICO can override the themed SVG");
        assert.match(icons[0], /\bid="favicon"/);
        // Do not publish the obsolete pale-tile icon for implicit /favicon.ico requests.
        assert.doesNotMatch(html, /favicon\.ico/);
    });

    test(`${page} selects the favicon on load and when the browser preference changes`, async () => {
        const html = await read(`../${page}`);
        const link = html.match(/<link id="favicon"[^>]+>/)[0];
        const dataset = Object.fromEntries(
            [...link.matchAll(/data-(light|dark)="([^"]+)"/g)].map(([, key, value]) => [key, value]),
        );
        assert.deepEqual(dataset, {
            light: "assets/oxfer-favicon-light.svg",
            dark: "assets/oxfer-favicon-dark.svg",
        });
        assert.match(html, /<script defer src="assets\/favicon\.js"><\/script>/);
        for (const initialDark of [false, true]) {
            let onChange;
            const icon = { dataset, href: "" };
            const scheme = {
                matches: initialDark,
                addEventListener(event, listener) {
                    assert.equal(event, "change");
                    onChange = listener;
                },
            };
            vm.runInNewContext(source, {
                document: {
                    getElementById(id) {
                        assert.equal(id, "favicon");
                        return icon;
                    },
                },
                window: {
                    matchMedia(query) {
                        assert.equal(query, "(prefers-color-scheme: dark)");
                        return scheme;
                    },
                },
            });
            assert.equal(icon.href, initialDark ? dataset.dark : dataset.light);
            for (const dark of [!initialDark, initialDark]) {
                scheme.matches = dark;
                onChange();
                assert.equal(icon.href, dark ? dataset.dark : dataset.light);
            }
        }
    });
}

test("favicon variants share the OX geometry and have fixed contrasting colors", async () => {
    const light = await read("../assets/oxfer-favicon-light.svg");
    const dark = await read("../assets/oxfer-favicon-dark.svg");
    assert.match(light, /fill="#151724"/);
    assert.match(dark, /fill="#f5f6f8"/);
    assert.equal(light.replace("#151724", "#f5f6f8"), dark);
    assert.doesNotMatch(light + dark, /@media|<rect/);
});
