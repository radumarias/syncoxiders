import assert from "node:assert/strict";
import { mkdtemp, mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { packageCfOutput, productionDomains, workerConfig } from "../package-cf-output.mjs";

const read = path => readFile(new URL(path, import.meta.url), "utf8");

test("cloudflare.config.ts matches the packaged Worker identity", async () => {
    const source = await read("../cloudflare.config.ts");
    assert.match(source, /from "cf\/config"/);
    assert.match(source, new RegExp(`name: "${workerConfig.name}"`));
    assert.match(source, new RegExp(`compatibilityDate: "${workerConfig.compatibilityDate}"`));
    assert.match(source, /notFoundHandling: "single-page-application"/);
    assert.match(source, /domains: \["oxfer\.app", "www\.oxfer\.app"\]/);
    assert.doesNotMatch(source, /\bentrypoint\s*:/);
    assert.doesNotMatch(source, /wrangler\.(jsonc?|toml)/);
});

test("package-cf-output copies Trunk dist into an assets-only cf Build Output", async () => {
    const dir = await mkdtemp(join(tmpdir(), "oxfer-cf-"));
    try {
        const dist = join(dir, "dist");
        await mkdir(join(dist, "assets"), { recursive: true });
        await writeFile(join(dist, "index.html"), "<!doctype html><title>oxfer</title>\n");
        await writeFile(join(dist, "sw.js"), "self.addEventListener('fetch', () => {});\n");
        await writeFile(join(dist, "_headers"), "/*\n  Referrer-Policy: no-referrer\n");
        await writeFile(join(dist, "assets/favicon.js"), "/* favicon */\n");
        const { assets, config } = await packageCfOutput({
            dist,
            outputRoot: join(dir, ".cloudflare/output"),
            includeDomains: false,
        });
        assert.deepEqual(config, workerConfig);
        assert.equal(
            await readFile(join(dir, ".cloudflare/output/v0/config.json"), "utf8"),
            `${JSON.stringify({ buildContext: { isPreview: false } })}\n`,
        );
        assert.equal(
            await readFile(join(assets, "index.html"), "utf8"),
            await readFile(join(dist, "index.html"), "utf8"),
        );
        assert.equal(
            await readFile(join(assets, "_headers"), "utf8"),
            await readFile(join(dist, "_headers"), "utf8"),
        );
        assert.equal(
            JSON.parse(await readFile(join(dir, ".cloudflare/output/v0/workers/default/worker.config.json"), "utf8")).assets.notFoundHandling,
            "single-page-application",
        );
    } finally {
        await rm(dir, { recursive: true, force: true });
    }
});

test("production packaging keeps custom domains so cf deploy cannot drop oxfer.app", async () => {
    const dir = await mkdtemp(join(tmpdir(), "oxfer-cf-domains-"));
    try {
        const dist = join(dir, "dist");
        await mkdir(dist, { recursive: true });
        await writeFile(join(dist, "index.html"), "<!doctype html>\n");
        const withDomains = await packageCfOutput({
            dist,
            outputRoot: join(dir, "out-domains"),
        });
        assert.deepEqual(withDomains.config.domains, productionDomains);
        const without = await packageCfOutput({
            dist,
            outputRoot: join(dir, "out-plain"),
            includeDomains: false,
        });
        assert.equal("domains" in without.config, false);
    } finally {
        await rm(dir, { recursive: true, force: true });
    }
});
