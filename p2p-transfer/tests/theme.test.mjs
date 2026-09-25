import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const source = await readFile(new URL("../assets/theme.js", import.meta.url), "utf8");
const { setBrowserTheme } = await import(
    `data:text/javascript;base64,${Buffer.from(source).toString("base64")}`
);

test("both theme choices update browser chrome in light and dark mode", () => {
    const metas = [{ content: null }, { content: null }];
    globalThis.document = {
        querySelectorAll(selector) {
            assert.equal(selector, 'meta[name="theme-color"]');
            return metas.map(meta => ({
                setAttribute(name, value) {
                    assert.equal(name, "content");
                    meta.content = value;
                },
            }));
        },
        body: { style: { backgroundColor: null } },
    };
    for (const [theme, dark, header, background] of [
        ["rusty", false, "#fffdfb", "#faf6f2"],
        ["rusty", true, "#231b17", "#100d0c"],
        ["clean", false, "#f8fafc", "#e5ebf1"],
        ["clean", true, "#1d2835", "#0e1621"],
    ]) {
        setBrowserTheme(theme, dark);
        assert.deepEqual(metas.map(meta => meta.content), [header, header]);
        assert.equal(document.body.style.backgroundColor, background);
    }
});
