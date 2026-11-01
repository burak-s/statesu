# --- Documentation build (static SvelteKit output) -------------------------
FROM node:24-alpine AS web

WORKDIR /src/web

COPY web/package.json web/package-lock.json ./
RUN npm ci

COPY web/ ./
ARG PUBLIC_TURNSTILE_SITE_KEY=""
ENV PUBLIC_TURNSTILE_SITE_KEY=${PUBLIC_TURNSTILE_SITE_KEY}
RUN npm run build

# --- Static musl backend build ---------------------------------------------
FROM rust:1.99.1-alpine AS builder

RUN apk add --no-cache build-base ca-certificates pkgconf

WORKDIR /src

# Warm the dependency cache with a stub crate before copying application code.
COPY Cargo.toml Cargo.lock ./
RUN mkdir -p src \
 && touch src/lib.rs \
 && printf 'fn main() {}\n' > src/main.rs \
 && cargo build --release --locked --target x86_64-unknown-linux-musl

COPY src ./src
COPY migrations ./migrations
COPY --from=web /src/web/build ./assets/dist

# Rebuild only the application crate; dependencies stay cached.
RUN touch src/lib.rs src/main.rs \
 && cargo build --release --locked --target x86_64-unknown-linux-musl

# --- Minimal runtime -------------------------------------------------------
FROM alpine:3.23

RUN apk add --no-cache ca-certificates \
 && mkdir -p /app /data \
 && chown -R 10001:10001 /app /data

WORKDIR /app

COPY --from=builder --chown=10001:10001 \
    /src/target/x86_64-unknown-linux-musl/release/statesu /app/statesu

ENV STATESU_ADDR=:8080

EXPOSE 8080
USER 10001:10001

ENTRYPOINT ["/app/statesu"]
