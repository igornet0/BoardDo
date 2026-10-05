# BoardDo — SBDO (React canvas) + SmartDo (Rust engine)
#
#   make            — список команд
#   make setup      — подмодули + зависимости (npm + cargo)
#   make dev        — backend + frontend одной командой (Ctrl+C останавливает оба)
#   make browser    — собрать RustBrowser (Servo) для чтения JS-страниц (опционально, долго)

SHELL := /bin/bash
.SHELLFLAGS := -eu -o pipefail -c
.DEFAULT_GOAL := help

FRONTEND_DIR  := SBDO
BROWSER_DIR   := vendor/RustBrowser
# Servo's C/C++ build scripts (jemalloc, mozjs) fail on paths with spaces, so the
# browser builds outside the repo. BoardDo looks for the binary here too.
BROWSER_TARGET_DIR ?= $(HOME)/.cache/boarddo/rust-browser
BROWSER_BIN   := $(BROWSER_TARGET_DIR)/release/rust-browser
BACKEND_PKG   := smartdo
CARGO         ?= cargo
NPM           ?= npm

# Runtime configuration (override: make dev LISTEN_ADDR=0.0.0.0:8080)
LISTEN_ADDR   ?= 127.0.0.1:8080
DATABASE_URL  ?= sqlite:boarddo.db?mode=rwc
DATA_DIR      ?= data
FRONTEND_PORT ?= 5173
RUST_LOG      ?= info

FRONTEND_ENV := BOARDDO_BACKEND=$(LISTEN_ADDR)
BACKEND_ENV := LISTEN_ADDR=$(LISTEN_ADDR) DATABASE_URL='$(DATABASE_URL)' BOARDDO_DATA_DIR=$(DATA_DIR) RUST_LOG=$(RUST_LOG)

# ---------------------------------------------------------------------------

.PHONY: help
help: ## Показать доступные команды
	@printf "\n\033[1mBoardDo\033[0m — SBDO + SmartDo\n\n"
	@awk 'BEGIN {FS = ":.*##"} /^[a-zA-Z0-9_.-]+:.*##/ { printf "  \033[36m%-18s\033[0m %s\n", $$1, $$2 } /^##@/ { printf "\n\033[1m%s\033[0m\n", substr($$0, 5) }' $(MAKEFILE_LIST)
	@printf "\nПеременные: LISTEN_ADDR=$(LISTEN_ADDR) FRONTEND_PORT=$(FRONTEND_PORT) DATABASE_URL=$(DATABASE_URL)\n\n"

##@ Установка

.PHONY: setup
setup: submodules install ## Подмодули + все зависимости

.PHONY: submodules
submodules: ## Инициализировать git-подмодули (gram_api, RustBrowser)
	git submodule update --init --recursive

.PHONY: install
install: install-frontend install-backend ## Установить зависимости frontend и backend

.PHONY: install-frontend
install-frontend: ## npm install для SBDO
	cd $(FRONTEND_DIR) && $(NPM) install

.PHONY: install-backend
install-backend: ## cargo fetch для workspace
	$(CARGO) fetch

.PHONY: config
config: ## Создать config/telegram.toml из примера (если нет)
	@if [ -f config/telegram.toml ]; then echo "config/telegram.toml уже существует"; \
	else cp config/telegram.toml.example config/telegram.toml && echo "Создан config/telegram.toml — заполните api_id / api_hash"; fi

##@ Запуск

.PHONY: dev
dev: ## Backend + frontend вместе (mock Telegram)
	@$(MAKE) --no-print-directory _ensure-frontend-deps
	@echo "▶ SmartDo  → http://$(LISTEN_ADDR)"
	@echo "▶ SBDO     → http://127.0.0.1:$(FRONTEND_PORT)"
	@trap 'kill 0' INT TERM EXIT; \
	  $(BACKEND_ENV) $(CARGO) run -p $(BACKEND_PKG) & \
	  (cd $(FRONTEND_DIR) && $(FRONTEND_ENV) $(NPM) run dev -- --port $(FRONTEND_PORT) --strictPort) & \
	  wait

.PHONY: dev-tdlib
dev-tdlib: ## Backend (с TDLib) + frontend вместе
	@$(MAKE) --no-print-directory _ensure-frontend-deps
	@trap 'kill 0' INT TERM EXIT; \
	  $(BACKEND_ENV) $(CARGO) run -p $(BACKEND_PKG) --features tdlib & \
	  (cd $(FRONTEND_DIR) && $(FRONTEND_ENV) $(NPM) run dev -- --port $(FRONTEND_PORT) --strictPort) & \
	  wait

.PHONY: backend
backend: ## Только SmartDo (mock Telegram)
	$(BACKEND_ENV) $(CARGO) run -p $(BACKEND_PKG)

.PHONY: backend-tdlib
backend-tdlib: ## Только SmartDo с нативным TDLib (нужен tdjson)
	$(BACKEND_ENV) $(CARGO) run -p $(BACKEND_PKG) --features tdlib

.PHONY: frontend
frontend: _ensure-frontend-deps ## Только SBDO (Vite dev server)
	cd $(FRONTEND_DIR) && $(FRONTEND_ENV) $(NPM) run dev -- --port $(FRONTEND_PORT) --strictPort

.PHONY: prod
prod: build ## Собрать всё и запустить release-backend + preview frontend
	@trap 'kill 0' INT TERM EXIT; \
	  $(BACKEND_ENV) ./target/release/$(BACKEND_PKG) & \
	  (cd $(FRONTEND_DIR) && $(FRONTEND_ENV) $(NPM) run preview -- --port $(FRONTEND_PORT) --strictPort) & \
	  wait

##@ Сборка

.PHONY: build
build: build-backend build-frontend ## Release-сборка backend и frontend

.PHONY: build-backend
build-backend: ## cargo build --release (smartdo)
	$(CARGO) build --release -p $(BACKEND_PKG)

.PHONY: build-frontend
build-frontend: _ensure-frontend-deps ## tsc + vite build → SBDO/dist
	cd $(FRONTEND_DIR) && $(NPM) run build

##@ Проверки

.PHONY: check
check: ## cargo check + проверка типов TypeScript
	$(CARGO) check --workspace
	cd $(FRONTEND_DIR) && npx tsc -b

.PHONY: check-tdlib
check-tdlib: ## cargo check с TDLib (нужен tdjson)
	$(CARGO) check -p $(BACKEND_PKG) --features tdlib

.PHONY: test
test: ## cargo test --workspace
	$(CARGO) test --workspace

.PHONY: test-telegram
test-telegram: ## Тесты Telegram vertical slice
	$(CARGO) test -p boarddo-telegram

.PHONY: lint
lint: ## clippy + oxlint
	$(CARGO) clippy --workspace --all-targets
	cd $(FRONTEND_DIR) && $(NPM) run lint

.PHONY: fmt
fmt: ## cargo fmt
	$(CARGO) fmt --all

.PHONY: ci
ci: check lint test build-frontend ## Полный прогон как в CI

##@ Обслуживание

.PHONY: ports
ports: ## Показать, кто занимает порты backend/frontend
	@lsof -nP -iTCP:$(lastword $(subst :, ,$(LISTEN_ADDR))) -iTCP:$(FRONTEND_PORT) -sTCP:LISTEN || echo "Порты свободны"

##@ Браузер (RustBrowser / Servo)

.PHONY: browser
browser: ## Собрать RustBrowser (release) — BoardDo запустит его сам, когда понадобится
	@test -f $(BROWSER_DIR)/Cargo.toml || git submodule update --init $(BROWSER_DIR)
	cd $(BROWSER_DIR) && CARGO_TARGET_DIR="$(BROWSER_TARGET_DIR)" $(CARGO) build --release -p browser-app
	@echo "Готово: $(BROWSER_BIN)"

.PHONY: browser-status
browser-status: ## Собран ли RustBrowser и запущен ли он backend-ом
	@test -x $(BROWSER_BIN) && echo "Собран: $(BROWSER_BIN)" || echo "Не собран — make browser"
	@curl -fsS http://$(LISTEN_ADDR)/api/browser/status 2>/dev/null || echo "(backend не запущен)"

.PHONY: clean
clean: ## Удалить артефакты сборки (target, SBDO/dist)
	$(CARGO) clean
	rm -rf $(FRONTEND_DIR)/dist

.PHONY: reset-db
reset-db: ## Удалить локальную SQLite-базу (с подтверждением)
	@read -p "Удалить boarddo.db* ? [y/N] " ans && [ "$$ans" = "y" ] && rm -f boarddo.db boarddo.db-shm boarddo.db-wal && echo "База удалена" || echo "Отменено"

# ---------------------------------------------------------------------------

.PHONY: _ensure-frontend-deps
_ensure-frontend-deps:
	@if [ ! -d $(FRONTEND_DIR)/node_modules ]; then echo "→ npm install ($(FRONTEND_DIR))"; cd $(FRONTEND_DIR) && $(NPM) install; fi
