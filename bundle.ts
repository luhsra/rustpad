import pkg from "./package.json";

await Bun.build({
  entrypoints: ["./index.html"],
  outdir: "./dist",
  target: "browser",
  minify: true,
  sourcemap: true,
  define: {
    VERSION: `"${pkg.version}"`,
    BUNDLER: '"bun"',
  },
});
