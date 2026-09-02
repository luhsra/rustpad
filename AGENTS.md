# Rustpad

## Layout

- The root Cargo workspace contains the Axum backend in `rustpad-server/`; its library (`rustpad-server/src/lib.rs`) owns HTTP/WebSocket routes and collaborative-editor state, while `main.rs` wires CLI options and startup.
- The React frontend is in `src/`. `server.ts` is Bun's development server: it serves the frontend on port 5173 and proxies `/api/*` HTTP and WebSocket traffic to the Rust backend at `localhost:3030`.
- The Rust backend serves `dist/` in production. Regenerate it with `bun run build`; do not edit `dist/`.

## Tooling and verification

- Rust is pinned to `1.93.0` in `rust-toolchain.toml`; Bun manages frontend dependencies (`bun.lock`).
- Local development requires two processes: `cargo run` (backend on port 3030) and `bun run dev` (frontend/proxy on port 5173).
- Run backend tests with `cargo test`; target an integration suite with `cargo test -p rustpad-server --test sockets` (replace `sockets` as needed).
- Run `bun run check` for strict TypeScript checking. Use `bun run build` to verify the production frontend bundle. `bun run format` applies Prettier to the entire repository and sorts TypeScript imports.
- The Docker build is the release gate: it runs release Rust tests/build plus `bun run check` and `bun run build` before producing a scratch image.

## Runtime configuration

- The actual server CLI accepts `--host` (default `0.0.0.0:3030`), `--storage` (default `storage`), and optional `--auth <path>`. The `storage/` directory and `auth.json` are local, ignored state.
- Treat auth configuration as secret material. Do not commit or expose `auth.json`.
