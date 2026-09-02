# Rustpad

[![Docker Pulls](https://img.shields.io/docker/pulls/ekzhang/rustpad)](https://hub.docker.com/r/ekzhang/rustpad/)
[![Docker Image Size](https://img.shields.io/docker/image-size/ekzhang/rustpad/latest)](https://hub.docker.com/r/ekzhang/rustpad/)
[![GitHub Workflow Status](https://img.shields.io/github/actions/workflow/status/ekzhang/rustpad/ci.yml)](https://github.com/ekzhang/rustpad/actions/workflows/ci.yml)

**Rustpad** is an _efficient_ and _minimal_ open-source collaborative text
editor based on the operational transformation algorithm. It lets users
collaborate in real time while writing code in their browser. Rustpad is
completely self-hosted and fits in a tiny Docker image, no database required.

<p align="center">
<a href="https://rustpad.io/">
<img src="https://i.imgur.com/WjU5UrP.png" width="800"><br>
<strong>rustpad.io</strong>
</a>
</p>

The server is written in Rust using [Axum](https://github.com/tokio-rs/axum)
and implements Quill's Delta format and operational transformation algorithm.
The frontend is written in TypeScript using [React](https://reactjs.org/) and
[Quill](https://quilljs.com/). Delta documents are used directly by the editor,
over WebSocket, and for persistence.

Architecturally, client-side code communicates via WebSocket with a central
server that keeps active documents in memory and persists document and user data
as JSON files on disk. Inactive documents are removed from memory after 24 hours
and are reloaded from storage when accessed again.

## Development setup

To run this application, install Rust and `bun`, then install the frontend
dependencies:

```
bun install
```

Next, compile and run the backend web server:

```
cargo run
```

While the backend is running, open another shell and run the following command
to start the frontend portion.

```
bun run dev
```

Open `http://localhost:5173` for the development frontend, which proxies API
and WebSocket requests to the backend at `localhost:3030`. Bun reloads the
frontend when files change.

## Testing

Run server tests with `cargo test` and check the frontend with `bun run check`.

## Configuration

The server accepts the following command-line options:

- `--host <ADDRESS>`: Bind address, defaulting to `0.0.0.0:3030`.
- `--storage <PATH>`: Directory for persisted document and user JSON files,
  defaulting to `storage`.
- `--auth <PATH>`: Optional path to an OpenID Connect configuration JSON file.
  When omitted, authentication is disabled.

For example:

```sh
cargo run -- --host 127.0.0.1:3030 --storage ./storage
```

An authentication configuration contains `client_id`, `client_secret`,
`issuer_url`, `host_url`, and `admin_group`. Keep this file outside version
control and pass its path with `--auth`. Set `RUST_LOG` to configure tracing
output.

## Deployment

Rustpad is distributed as a single 6 MB Docker image, which is built
automatically from the `Dockerfile` in this repository. You can pull the latest
version of this image from Docker Hub. It has multi-platform support for
`linux/amd64` and `linux/arm64`.

```
docker pull ekzhang/rustpad
```

(You can also manually build this image with `docker build -t rustpad .` in the
project root directory.) To run locally, execute the following command, then
open `http://localhost:3030` in your browser.

```
docker run --rm -dp 3030:3030 ekzhang/rustpad
```

We deploy a public instance of this image using [Fly.io](https://fly.io/).

## In the media

- **July 11, 2021:** Featured in
  [Console 61 - The open-source newsletter](https://console.substack.com/p/console-61).
- **June 5, 2021:** Front-page
  [Hacker News post](https://news.ycombinator.com/item?id=27408326). Reddit
  discussions in [r/rust](https://www.reddit.com/r/rust/comments/nt4p9f/) and
  [r/programming](https://www.reddit.com/r/programming/comments/nt4ws7/).

<br>

<sup>
All code is licensed under the <a href="LICENSE">MIT license</a>.
</sup>
