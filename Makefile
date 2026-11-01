.PHONY: build run api dev dev-cli test test-browser clean web fmt clippy docker-build docker-run docker-shell docker-clean

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

# Full-stack dev ports: the API binds to the loopback interface only, and the
# Vite dev server proxies /auth, /blogs and /public to it.
API_PORT   ?= 8080
VITE_PORT  ?= 5173
ARGS       ?= --help

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

# Run the API with the embedded docs from assets/dist; running `web` first
# refreshes the docs, or use `make dev` for the HMR workflow.
run: web
	cargo run --release --locked --bin statesu

# Backend-only runner: starts the API directly without touching the docs
# build, so repeated runs stay fast.
api:
	cargo run --release --locked --bin statesu

# Full-stack development: Rust API in the background plus the Vite dev server
# with HMR proxying /auth, /blogs and /public. Ctrl+C or an exit stops both.
# .env is exported so required keys are always loaded (same as docker-run).
dev: $(WEB_MODULES)
	@bash -c ' \
		set -a; if test -f .env; then . ./.env; fi; set +a; \
		STATESU_ADDR=127.0.0.1:$(API_PORT) cargo run --release --locked --bin statesu & \
		API_PID=$$!; \
		trap "kill $$API_PID 2>/dev/null" EXIT; \
		printf "waiting for the API"; \
		until curl -s -o /dev/null http://127.0.0.1:$(API_PORT)/public/blogs/dev-ready; do \
			if ! kill -0 $$API_PID 2>/dev/null; then \
				echo; echo "the API exited during startup; see its output above"; exit 1; \
			fi; \
			printf "."; sleep 0.5; \
		done; \
		echo " ready"; \
		cd $(WEB_DIR) && STATESU_API=http://127.0.0.1:$(API_PORT) npm run dev -- --port $(VITE_PORT)'

# CLI-only development; e.g. make dev-cli ARGS="blog list".
dev-cli:
	cargo run --locked --bin stscli -- $(ARGS)

test:
	cargo test --all-targets --locked

# Builds the frontend and starts isolated API/consumer servers automatically.
test-browser: $(WEB_MODULES)
	cd $(WEB_DIR) && npm run test:browser

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
