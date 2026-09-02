FROM rust:alpine AS backend
WORKDIR /home/rust/src
RUN apk --no-cache add musl-dev openssl-dev
COPY . .
RUN cargo test --release
RUN cargo build --release

FROM oven/bun:1-alpine AS frontend
WORKDIR /usr/src/app
COPY package.json bun.lock ./
RUN bun install --frozen-lockfile
COPY . .
RUN bun run check
RUN bun run build

FROM scratch
COPY --from=frontend /usr/src/app/dist dist
COPY --from=backend /home/rust/src/target/release/rustpad-server .
USER 1000:1000
CMD [ "./rustpad-server" ]
