import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";
import topLevelAwait from "vite-plugin-top-level-await";
import wasm from "vite-plugin-wasm";
import tsconfigPaths from "vite-tsconfig-paths"

export default defineConfig({
  base: "",
  build: {
    chunkSizeWarningLimit: 1000,
  },
  plugins: [wasm(), topLevelAwait(), react(), tsconfigPaths()],
  server: {
    proxy: {
      "/api": {
        target: "http://127.0.0.1:3030",
        changeOrigin: true,
        secure: false,
        ws: true,
      },
    },
  },
});
