import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { mkdtemp, mkdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { promisify } from "node:util";
import { measure, table } from "../report-bundle-size.mjs";

const script = fileURLToPath(new URL("../report-bundle-size.mjs", import.meta.url));
const run = promisify(execFile);

async function fixture() {
    const dir = await mkdtemp(join(tmpdir(), "oxfer-size-"));
    await mkdir(join(dir, "assets"));
    // Incompressible bytes, so the brotli size stays close to the raw size.
    const noise = Buffer.alloc(64 * 1024);
    for (let i = 0; i < noise.length; i++) noise[i] = (i * 2654435761) >>> 24;
    await writeFile(join(dir, "assets", "app-dxh0123456789abcdef.wasm"), noise);
    await writeFile(join(dir, "assets", "app.js"), "export const a = 1;\n".repeat(100));
    await writeFile(join(dir, "index.html"), "<!doctype html>");
    return dir;
}

test("measures wasm and js only, largest first, with smaller compressed sizes", async () => {
    const dir = await fixture();
    try {
        const rows = await measure(dir);
        assert.deepEqual(rows.map((r) => r.file), [
            join("assets", "app-dxh0123456789abcdef.wasm"),
            join("assets", "app.js"),
        ]);
        const js = rows[1];
        assert.ok(js.gzip < js.raw && js.brotli < js.raw);
        assert.match(table(rows), /^\| file \| raw \| gzip \| brotli \|/);
    } finally {
        await rm(dir, { recursive: true, force: true });
    }
});

test("fails only when brotli wasm exceeds the budget", async () => {
    const dir = await fixture();
    try {
        await run(process.execPath, [script, dir, "--budget-brotli", String(10 * 1024 * 1024)], {
            env: { ...process.env, GITHUB_STEP_SUMMARY: "" },
        });
        await assert.rejects(
            run(process.execPath, [script, dir, "--budget-brotli", "1024"], {
                env: { ...process.env, GITHUB_STEP_SUMMARY: "" },
            }),
            (error) => error.code === 1 && /over the 1\.0 KiB budget/.test(error.stderr),
        );
    } finally {
        await rm(dir, { recursive: true, force: true });
    }
});
