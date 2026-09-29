// Package Trunk `dist/` as a cf Build Output Specification tree.
// `cf deploy --prebuilt` uploads this; it does not evaluate cloudflare.config.ts.
import { cp, mkdir, rm, writeFile } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = dirname(fileURLToPath(import.meta.url));

export const workerConfig = {
    name: "oxfer",
    compatibilityDate: "2026-09-29",
    workersDev: true,
    previewUrls: true,
    assets: {
        notFoundHandling: "single-page-application",
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
    await rm(outputRoot, { recursive: true, force: true });
    await mkdir(assets, { recursive: true });
    await cp(dist, assets, { recursive: true });
    const config = includeDomains
        ? { ...workerConfig, domains: [...productionDomains] }
        : { ...workerConfig };
    await writeFile(
        join(bos, "config.json"),
        `${JSON.stringify({ buildContext: { isPreview: false } })}\n`,
    );
    await writeFile(
        join(bos, "workers/default/worker.config.json"),
        `${JSON.stringify(config, null, 4)}\n`,
    );
    return { assets, config };
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
    await packageCfOutput({
        includeDomains: !process.argv.includes("--no-domains"),
    });
}
