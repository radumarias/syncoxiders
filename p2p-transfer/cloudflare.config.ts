import { defineConfig } from "cf/config";

// Assets-only Worker: no server entrypoint. Trunk output is packaged into
// `.cloudflare/output` by `package-cf-output.mjs` and uploaded with
// `cf deploy --prebuilt`. Keep this file in sync with that packager.
export default defineConfig({
    worker: {
        name: "oxfer",
        compatibilityDate: "2026-09-29",
        workersDev: true,
        previewUrls: true,
        assets: {
            notFoundHandling: "single-page-application",
        },
        domains: ["oxfer.app", "www.oxfer.app"],
    },
});
