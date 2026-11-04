// Package Trunk `dist/` as a cf Build Output Specification tree.
// `cf deploy --prebuilt` uploads this; it does not evaluate cloudflare.config.ts.
import { cp, mkdir, rm, writeFile } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { WORKER_FIRST } from "./worker/index.mjs";

const root = dirname(fileURLToPath(import.meta.url));

// The Worker (worker/index.mjs) runs first for WORKER_FIRST paths: Markdown content
// negotiation for `/` and `/diags`. Other requests are free static asset requests,
// except non-navigation requests that match no asset, which also reach the Worker.
export const workerConfig = {
    name: "oxfer",
    compatibilityDate: "2026-09-29",
    workersDev: true,
    previewUrls: true,
    env: {
        ASSETS: { type: "assets" },
    },
    assets: {
        notFoundHandling: "single-page-application",
        runWorkerFirst: [...WORKER_FIRST],
    },
};

export const workerManifest = {
    type: "complete",
    mainModule: "index.mjs",
    modules: {
        "index.mjs": { type: "esm" },
    },
};

export const productionDomains = ["oxfer.app", "www.oxfer.app"];

export async function packageCfOutput({
    includeDomains = true,
    dist = join(root, "dist"),
    outputRoot = join(root, ".cloudflare/output"),
} = {}) {
    const bos = join(outputRoot, "v0");
    const assets = join(bos, "workers/default/assets");
    const bundle = join(bos, "workers/default/bundle");
    await rm(outputRoot, { recursive: true, force: true });
    await mkdir(assets, { recursive: true });
    await mkdir(bundle, { recursive: true });
    await cp(dist, assets, { recursive: true });
    await cp(join(root, "worker/index.mjs"), join(bundle, "index.mjs"));
    const base = { ...workerConfig, manifest: workerManifest };
    const config = includeDomains
        ? { ...base, domains: [...productionDomains] }
        : base;
    await writeFile(
        join(bos, "config.json"),
        `${JSON.stringify({ buildContext: { isPreview: false } })}\n`,
    );
    await writeFile(
        join(bos, "workers/default/worker.config.json"),
        `${JSON.stringify(config, null, 4)}\n`,
    );
    return { assets, bundle, config };
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
    await packageCfOutput({
        includeDomains: !process.argv.includes("--no-domains"),
    });
}
