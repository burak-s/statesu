.PHONY: build run test clean web dev fmt clippy docker-build docker-run docker-shell docker-clean

BIN          := bin/statesu
IMAGE        := statesu
TAG          := latest
CONTAINER    := statesu
VOLUME       := statesu-data-rust
PORT         := 8080

WEB_DIR      := web
WEB_BUILD    := $(WEB_DIR)/build
WEB_MODULES  := $(WEB_DIR)/node_modules
SPA_DIST     := assets/dist

$(WEB_MODULES): $(WEB_DIR)/package-lock.json
	cd $(WEB_DIR) && npm ci

web: $(WEB_MODULES)
	cd $(WEB_DIR) && npm run build
	@mkdir -p $(SPA_DIST)
	@find $(SPA_DIST) -mindepth 1 ! -name .gitkeep -delete
	@cp -r $(WEB_BUILD)/. $(SPA_DIST)/

# Build the docs and the Rust binary; docs are embedded at compile time.
build: web
	@mkdir -p bin
	cargo build --release --locked
	@cp target/release/statesu $(BIN)

# Run the API. Docs are served from assets/dist; run `make web` after
# frontend changes, or use `make dev` for the Vite dev server.
run: web
	cargo run --release --locked

# Frontend dev server with HMR, proxying the API to the Rust server.
dev: $(WEB_MODULES)
	cd $(WEB_DIR) && npm run dev

test:
	cargo test --all-targets --locked

fmt:
	cargo fmt --all --check

clippy:
	cargo clippy --all-targets --locked -- -D warnings

clean:
	rm -rf bin target $(WEB_BUILD) $(WEB_DIR)/.svelte-kit
	rm -f db/statesu.db db/statesu.db-wal db/statesu.db-shm
	find $(SPA_DIST) -mindepth 1 ! -name .gitkeep -delete

docker-build:
	docker build -t $(IMAGE):$(TAG) .

docker-run: docker-build
	docker run --rm -it \
		--name $(CONTAINER) \
		-p $(PORT):8088 \
		-v $(VOLUME):/data \
		--env-file .env \
		$(IMAGE):$(TAG)

docker-shell:
	docker run --rm -it --entrypoint /bin/sh $(IMAGE):$(TAG)

docker-clean:
	-docker rm -f $(CONTAINER)
	-docker volume rm $(VOLUME)
	-docker rmi $(IMAGE):$(TAG)
