import pkg from "./package.json";

await Bun.build({
    entrypoints: ['./index.html'],
    outdir: './dist',
    define: {
        // Mimic Vite's import.meta.env
        "import.meta.env.VERSION": JSON.stringify(pkg.version),
    }
});
