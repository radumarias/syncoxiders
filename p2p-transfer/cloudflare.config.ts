import { bindings, defineConfig } from "cf/config";

// Static assets plus one small Worker (worker/index.mjs) that answers Markdown to
// agents on `/` and `/diags`. Trunk output and the Worker are packaged into
// `.cloudflare/output` by `package-cf-output.mjs` and uploaded with
// `cf deploy --prebuilt`. Keep this file in sync with that packager.
export default defineConfig({
    worker: {
        name: "oxfer",
        compatibilityDate: "2026-09-29",
        workersDev: true,
        previewUrls: true,
        entrypoint: "./worker/index.mjs",
        env: {
            ASSETS: bindings.assets(),
        },
        assets: {
            notFoundHandling: "single-page-application",
            runWorkerFirst: ["/index.md", "/", "/index.html", "/diags.md", "/diags", "/diags/"],
        },
        domains: ["oxfer.app", "www.oxfer.app"],
    },
});
