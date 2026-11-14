// Report the size of every .wasm and .js file in a build directory, raw, gzip -9 and
// brotli -11 (what Cloudflare serves), and fail when brotli wasm exceeds a budget.
//
//   node report-bundle-size.mjs <dir> [--budget-brotli <bytes>]
//
// In GitHub Actions the table is appended to the job summary as well.
import { appendFile, readdir, readFile, stat } from "node:fs/promises";
import { join, relative } from "node:path";
import { brotliCompressSync, constants, gzipSync } from "node:zlib";

async function* files(dir) {
    for (const entry of await readdir(dir, { withFileTypes: true })) {
        const path = join(dir, entry.name);
        if (entry.isDirectory()) yield* files(path);
        else if (/\.(wasm|js|mjs)$/.test(entry.name)) yield path;
    }
}

export async function measure(dir) {
    const rows = [];
    for await (const path of files(dir)) {
        const bytes = await readFile(path);
        rows.push({
            file: relative(dir, path),
            raw: (await stat(path)).size,
            gzip: gzipSync(bytes, { level: 9 }).length,
            brotli: brotliCompressSync(bytes, {
                params: { [constants.BROTLI_PARAM_QUALITY]: 11 },
            }).length,
        });
    }
    return rows.sort((a, b) => b.raw - a.raw);
}

const kib = (n) => `${(n / 1024).toFixed(1)} KiB`;

export function table(rows) {
    const lines = ["| file | raw | gzip | brotli |", "| --- | ---: | ---: | ---: |"];
    for (const row of rows) {
        lines.push(`| ${row.file} | ${kib(row.raw)} | ${kib(row.gzip)} | ${kib(row.brotli)} |`);
    }
    return `${lines.join("\n")}\n`;
}

if (import.meta.url === `file://${process.argv[1]}`) {
    const [dir, ...rest] = process.argv.slice(2);
    if (!dir) {
        console.error("usage: node report-bundle-size.mjs <dir> [--budget-brotli <bytes>]");
        process.exit(2);
    }
    const flag = rest.indexOf("--budget-brotli");
    const budget = flag >= 0 ? Number(rest[flag + 1]) : undefined;
    const rows = await measure(dir);
    const report = table(rows);
    process.stdout.write(report);
    if (process.env.GITHUB_STEP_SUMMARY) {
        await appendFile(process.env.GITHUB_STEP_SUMMARY, `### Web bundle size (${dir})\n\n${report}\n`);
    }
    const wasm = rows.filter((row) => row.file.endsWith(".wasm"));
    const over = budget === undefined ? [] : wasm.filter((row) => row.brotli > budget);
    if (over.length) {
        console.error(`brotli wasm over the ${kib(budget)} budget: ${over.map((r) => r.file).join(", ")}`);
        process.exit(1);
    }
}
