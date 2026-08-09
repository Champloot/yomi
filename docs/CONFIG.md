# Конфигурация

## Где что лежит

Пути соответствуют стандарту XDG Base Directory. Проверить фактические
значения: `yomi config path`.

| Назначение | Путь по умолчанию | Переопределение |
|---|---|---|
| Конфигурация | `~/.config/yomi/config.toml` | `YOMI_CONFIG_DIR`, флаг `--config` |
| Данные (база, загрузки) | `~/.local/share/yomi/` | `YOMI_DATA_DIR` |
| Кэш (обложки) | `~/.cache/yomi/` | `YOMI_CACHE_DIR` |

Каталог кэша можно удалять целиком в любой момент — программа его
восстановит.

## Создание файла

```bash
manga config init          # создать со значениями по умолчанию
manga config init --force  # перезаписать существующий
manga config show          # показать действующие значения
```

Файла может не быть — программа работает на значениях по умолчанию.

## Параметры

### `[general]`

```toml
[general]
language = "ru"                      # язык интерфейса
content_languages = ["ru", "en"]     # языки переводов, по убыванию приоритета
```

### `[library]`

```toml
[library]
paths = ["/home/user/Манга", "/mnt/nas/manga"]
```

Каталоги, которые обходит `yomi library scan` без аргументов.

### `[reader]`

```toml
[reader]
renderer = "auto"            # auto | kitty | iterm2 | sixel | blocks
direction = "right-to-left"  # right-to-left | left-to-right | webtoon
double_page = false          # разворот из двух страниц
preload_pages = 2            # сколько страниц готовить заранее
```

О значениях `renderer` — [adr/0003-image-rendering.md](adr/0003-image-rendering.md).
Оставляйте `auto`, если не отлаживаете конкретный протокол.

`direction = "webtoon"` — вертикальная лента для манхвы и маньхуа.

### `[download]`

```toml
[download]
# directory = "/home/user/Манга"     # по умолчанию — каталог данных XDG
filename_template = "{manga}/{volume}-{chapter} {title}.cbz"
concurrency = 4                      # одновременных загрузок страниц
```

Подстановки в шаблоне: `{manga}`, `{volume}`, `{chapter}`, `{title}`,
`{scanlator}`, `{language}`.

Не задирайте `concurrency`: источники ограничивают частоту запросов,
и агрессивный клиент получит бан по адресу.

### `[network]`

```toml
[network]
timeout_secs = 30
retries = 3
user_agent = "yomi/0.1.0"
# proxy = "socks5://127.0.0.1:9050"
```

## Приоритет значений

Позднее переопределяет раннее:

1. значения по умолчанию в коде;
2. файл конфигурации;
3. флаги командной строки (`--renderer`, `--output` и прочие).

## Переменные окружения

| Переменная | Назначение |
|---|---|
| `YOMI_CONFIG` | путь к файлу конфигурации целиком |
| `YOMI_CONFIG_DIR` | каталог конфигурации |
| `YOMI_DATA_DIR` | каталог данных |
| `YOMI_CACHE_DIR` | каталог кэша |
| `YOMI_LOG` | фильтр логов, например `yomi_core=debug` |

Переменные каталогов задуманы прежде всего для тестов: они позволяют
прогонять проверки, не трогая настоящий домашний каталог.

## Диагностика

```bash
manga -vv sources list           # подробный лог в stderr
YOMI_LOG=trace manga search пример
manga config show > /tmp/cfg     # в файл попадёт только конфиг: логи идут в stderr
```

Опечатка в имени параметра — ошибка, а не молчаливое игнорирование:
`yomi config show` сообщит, какой ключ не опознан.
