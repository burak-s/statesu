FROM node:24-alpine AS web

WORKDIR /src/web

COPY web/package.json web/package-lock.json ./
RUN npm ci

COPY web/ ./
RUN npm run build

FROM golang:1.27-alpine AS builder

WORKDIR /src

COPY go.mod go.sum ./
RUN go mod download

COPY . .
COPY --from=web /src/web/build ./internal/spa/dist

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
