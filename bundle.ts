import pkg from "./package.json";

await Bun.build({
    entrypoints: [
        './index.html',
        './src/monaco/editor.worker.ts',
        './src/monaco/html.worker.ts',
        './src/monaco/json.worker.ts',
        './src/monaco/css.worker.ts',
        './src/monaco/ts.worker.ts',
    ],
    outdir: './dist',
    target: 'browser',
    minify: true,
    define: {
        "VERSION": `"${pkg.version}"`,
        "BUNDLER": "\"bun\"",
    },
});
