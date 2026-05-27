.PHONY: build run test clean css css-watch tailwind docker-build docker-run docker-shell docker-clean

BIN          := bin/statesu
PKG          := ./cmd/statesu
IMAGE        := statesu
TAG          := latest
CONTAINER    := statesu
VOLUME       := statesu-data
PORT         := 8080

TAILWIND_VERSION := v3.4.17
TAILWIND_OS      := $(shell uname -s | tr '[:upper:]' '[:lower:]')
TAILWIND_ARCH    := $(shell uname -m)
ifeq ($(TAILWIND_ARCH),x86_64)
TAILWIND_ARCH := x64
endif
ifeq ($(TAILWIND_ARCH),aarch64)
TAILWIND_ARCH := arm64
endif
TAILWIND_BIN := bin/tailwindcss
TAILWIND_URL := https://github.com/tailwindlabs/tailwindcss/releases/download/$(TAILWIND_VERSION)/tailwindcss-$(TAILWIND_OS)-$(TAILWIND_ARCH)

CSS_IN  := assets/css/app.css
CSS_OUT := internal/view/static/app.css

$(TAILWIND_BIN):
	@mkdir -p bin
	curl -sSL -o $@ $(TAILWIND_URL)
	chmod +x $@

tailwind: $(TAILWIND_BIN)

css: $(TAILWIND_BIN)
	$(TAILWIND_BIN) -i $(CSS_IN) -o $(CSS_OUT) --minify

css-watch: $(TAILWIND_BIN)
	$(TAILWIND_BIN) -i $(CSS_IN) -o $(CSS_OUT) --watch

build: css
	@mkdir -p bin
	go build -o $(BIN) $(PKG)

run: css
	go run $(PKG)

test:
	go test ./...

clean:
	rm -rf bin statesu.db statesu.db-wal statesu.db-shm $(CSS_OUT)

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
