# Обёртки над cargo для частых операций.
# Ничего волшебного: любую цель можно выполнить руками.

.PHONY: help build release test fmt lint check run clean install doc audit

help:  ## Показать эту справку
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | \
		awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-12s\033[0m %s\n", $$1, $$2}'

build:  ## Отладочная сборка
	cargo build

release:  ## Релизная сборка (оптимизация, ~2,8 МБ)
	cargo build --release

test:  ## Прогнать тесты
	cargo test

fmt:  ## Отформатировать код
	cargo fmt --all

lint:  ## Проверка clippy, предупреждения считаются ошибками
	cargo clippy --all-targets --all-features -- -D warnings

check: fmt lint test  ## Полная проверка перед коммитом

run:  ## Запустить: make run ARGS="sources list"
	cargo run -- $(ARGS)

doc:  ## Собрать и открыть документацию по коду
	cargo doc --no-deps --open

clean:  ## Удалить артефакты сборки
	cargo clean

install:  ## Установить в ~/.cargo/bin
	cargo install --path crates/yomi-cli

watch:  ## Пересобирать и прогонять тесты при каждой правке
	cargo watch -x test

audit:  ## Проверить зависимости на известные уязвимости
	cargo audit
