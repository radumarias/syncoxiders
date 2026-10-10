import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import vm from "node:vm";

// theme.html is served from the app origin, so a shared theme file must not be able to inject markup.
const html = await readFile(new URL("../theme.html", import.meta.url), "utf8");
const script = html.match(/<script>([\s\S]*?)<\/script>/)[1];
const ids = new Set([...html.matchAll(/\bid="([^"]+)"/g)].map(([, id]) => id));
const HEX = /^#[0-9a-fA-F]{6}$/;
const ATTACK = '" autofocus onfocus="alert(document.domain)';

function loadLab() {
    const markup = [];
    class FakeElement {
        constructor(tagName) {
            this.tagName = tagName.toUpperCase();
            this.children = [];
            this.attributes = {};
            this.style = { setProperty(name, value) { this[name] = value; } };
            this.classList = { toggle() {} };
            this.value = "";
            this.textContent = "";
        }
        set innerHTML(source) {
            markup.push(source);
            this.children = [];
        }
        replaceChildren(...nodes) { this.children = nodes; }
        append(...nodes) { this.children.push(...nodes); }
        setAttribute(name, value) { this.attributes[name] = String(value); }
        addEventListener() {}
    }
    const elements = new Map();
    const document = {
        getElementById(id) {
            assert.ok(ids.has(id), `theme.html has no #${id}`);
            if (!elements.has(id)) elements.set(id, new FakeElement("div"));
            return elements.get(id);
        },
        createElement: tagName => new FakeElement(tagName),
    };
    const context = vm.createContext({ document });
    vm.runInContext(script, context);
    // Round-trip through JSON so assertions compare plain objects from this realm.
    const evaluate = code => {
        const json = vm.runInContext(`JSON.stringify(${code})`, context);
        return json === undefined ? undefined : JSON.parse(json);
    };
    const importTheme = file => {
        context.input = JSON.stringify(file);
        return evaluate("applyBundle(JSON.parse(input))");
    };
    const tokenRows = () => document.getElementById("tokens").children;
    return { document, markup, evaluate, importTheme, tokenRows };
}

test("imported theme files keep only well-formed tokens and radii", () => {
    const lab = loadLab();
    const clean = lab.evaluate("CLEAN");
    const ignored = lab.importTheme({
        format: "oxfer-theme",
        name: "<img src=x onerror=alert(1)>",
        intent: ATTACK,
        author: { toString: "nope" },
        control_radius: "1px; background: url(https://example.invalid/)",
        card_radius: 12,
        light: { bg: ATTACK, primary: "#123456", surface: ["#abcdef"], on_primary: "#ABCDEF" },
        dark: { error: "</script><script>alert(1)</script>", extra: ATTACK },
    });
    assert.deepEqual(ignored, ["author", "control_radius", "light.bg", "light.surface", "dark.error"]);

    const state = lab.evaluate("state");
    const tokenKeys = lab.evaluate("TOKENS").map(([key]) => key);
    for (const mode of ["light", "dark"]) {
        assert.deepEqual(Object.keys(state[mode]), tokenKeys, `${mode} carries only the token API`);
        for (const value of Object.values(state[mode])) assert.match(value, HEX);
    }
    assert.equal(state.light.bg, clean.light.bg);
    assert.equal(state.light.surface, clean.light.surface);
    assert.equal(state.light.primary, "#123456");
    assert.equal(state.light.on_primary, "#ABCDEF");
    assert.equal(state.dark.error, clean.dark.error);
    assert.equal(state.control_radius, 18);
    assert.equal(state.card_radius, 12);
    assert.equal(state.author, "");

    // Free-text metadata survives, but only as form values.
    assert.equal(lab.document.getElementById("name").value, "<img src=x onerror=alert(1)>");
    assert.equal(lab.document.getElementById("intent").value, ATTACK);
    for (const source of lab.markup) assert.doesNotMatch(source, /alert|onerror|onfocus/);
});

test("token rows carry values as properties, never as markup", () => {
    const lab = loadLab();
    for (const mode of ["light", "dark"]) {
        // A value typed into the hex field is not validated, so rendering must not trust it either.
        lab.evaluate(`(state.edit = ${JSON.stringify(mode)}, state.${mode}.bg = ${JSON.stringify(ATTACK)}, renderTokens(), renderAll())`);
        const rows = lab.tokenRows();
        assert.equal(rows.length, 13);
        const [name, picker, text] = rows[0].children;
        assert.equal(name.textContent, "bg");
        assert.equal(picker.type, "color");
        assert.equal(picker.value, "#000000");
        assert.equal(text.type, "text");
        assert.equal(text.value, ATTACK);
        for (const row of rows.slice(1)) assert.match(row.children[2].value, HEX);
    }
    for (const source of lab.markup) assert.doesNotMatch(source, /onfocus/);
});

test("an exported theme imports back unchanged", () => {
    const lab = loadLab();
    lab.evaluate("(applyPreset(RUSTY, 'Rusty'), state.intent = 'warm workshop', state.author = 'Ada')");
    const exported = lab.evaluate("bundle()");
    lab.evaluate("(applyPreset(CLEAN, ''), state.name = '', state.intent = '', state.author = '')");
    assert.deepEqual(lab.importTheme(exported), []);
    assert.deepEqual(lab.evaluate("bundle()"), exported);
});

test("files that are not oxfer themes are rejected", () => {
    const lab = loadLab();
    for (const file of [null, [], "oxfer-theme", { format: "other" }]) {
        assert.throws(() => lab.importTheme(file), /Not an oxfer-theme file/);
    }
});
