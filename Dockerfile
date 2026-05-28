FROM alpine:3.23 AS tailwind

ARG TAILWIND_VERSION=v3.4.17
ARG TARGETARCH=amd64

RUN apk add --no-cache curl ca-certificates \
 && case "$TARGETARCH" in \
        amd64)  TW_ARCH=x64 ;; \
        arm64)  TW_ARCH=arm64 ;; \
        *)      echo "unsupported arch: $TARGETARCH" && exit 1 ;; \
    esac \
 && curl -sSL -o /usr/local/bin/tailwindcss \
      "https://github.com/tailwindlabs/tailwindcss/releases/download/${TAILWIND_VERSION}/tailwindcss-linux-${TW_ARCH}" \
 && chmod +x /usr/local/bin/tailwindcss

WORKDIR /src
COPY tailwind.config.js ./
COPY assets ./assets
COPY internal/view/templates ./internal/view/templates

RUN mkdir -p internal/view/static \
 && tailwindcss \
      -i assets/css/app.css \
      -o internal/view/static/app.css \
      --minify

FROM golang:1.26.3-alpine3.23 AS builder

WORKDIR /src

COPY go.mod go.sum ./
RUN go mod download

COPY . .
COPY --from=tailwind /src/internal/view/static/app.css ./internal/view/static/app.css

RUN CGO_ENABLED=0 GOOS=linux go build \
    -trimpath \
    -ldflags="-s -w" \
    -o /out/statesu \
    ./cmd/statesu

FROM alpine:3.23

RUN apk add --no-cache ca-certificates tzdata \
 && addgroup -S app \
 && adduser -S -G app -H -h /app app \
 && mkdir -p /app /data \
 && chown -R app:app /app /data

WORKDIR /app

COPY --from=builder --chown=app:app /out/statesu /app/statesu

ENV STATESU_ADDR=:8088

EXPOSE 8088
USER app:app

ENTRYPOINT ["/app/statesu"]
