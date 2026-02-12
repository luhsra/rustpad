import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";
import wasm from "vite-plugin-wasm";
import tsconfigPaths from "vite-tsconfig-paths";

export default defineConfig({
  base: "",
  build: {
    chunkSizeWarningLimit: 1000,
  },
  plugins: [wasm(), react(), tsconfigPaths()],
  server: {
    proxy: {
      "/api": {
        target: "http://localhost:3030",
        changeOrigin: true,
        secure: false,
        ws: true,
      },
    },
  },
});
