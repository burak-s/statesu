.PHONY: build run test clean web dev docker-build docker-run docker-shell docker-clean

BIN          := bin/statesu
PKG          := ./cmd/statesu
IMAGE        := statesu
TAG          := latest
CONTAINER    := statesu
VOLUME       := statesu-data
PORT         := 8080

WEB_DIR      := web
WEB_BUILD    := $(WEB_DIR)/build
WEB_MODULES  := $(WEB_DIR)/node_modules
SPA_DIST     := internal/spa/dist

$(WEB_MODULES): $(WEB_DIR)/package-lock.json
	cd $(WEB_DIR) && npm ci

web: $(WEB_MODULES)
	cd $(WEB_DIR) && npm run build
	@mkdir -p $(SPA_DIST)
	@cp -r $(WEB_BUILD)/. $(SPA_DIST)/

# Build the SPA and the Go binary (the SPA is embedded into it).
build: web
	@mkdir -p bin
	go build -o $(BIN) $(PKG)

# Run the API. The SPA is served from internal/spa/dist; run `make web` first
# after frontend changes, or use `make dev` for live reload.
run: web
	go run $(PKG)

# Frontend dev server with HMR, proxying the API to the Go server.
dev: $(WEB_MODULES)
	cd $(WEB_DIR) && npm run dev

test:
	go test ./...

clean:
	rm -rf bin statesu.db statesu.db-wal statesu.db-shm $(WEB_BUILD) $(WEB_DIR)/.svelte-kit
	find $(SPA_DIST) -mindepth 1 ! -name .gitkeep -delete

docker-build:
	docker build -t $(IMAGE):$(TAG) .

docker-run: docker-build
	docker run --rm -it \
		--name $(CONTAINER) \
		-p $(PORT):8080 \
		-v $(VOLUME):/data \
		--env-file .env \
		$(IMAGE):$(TAG)

docker-shell:
	docker run --rm -it --entrypoint /bin/sh $(IMAGE):$(TAG)

docker-clean:
	-docker rm -f $(CONTAINER)
	-docker volume rm $(VOLUME)
	-docker rmi $(IMAGE):$(TAG)
