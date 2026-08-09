# Разработка на NixOS

Флейк в корне даёт две вещи: окружение разработки (`devShell`) и сам пакет.
Ставить Rust в систему через `rustup` не нужно и нежелательно — на NixOS
это как раз источник проблем.

## Включить флейки

Если ещё не включены, в `/etc/nixos/configuration.nix`:

```nix
nix.settings.experimental-features = [ "nix-command" "flakes" ];
```

Затем `sudo nixos-rebuild switch`.

## Окружение разработки

```bash
cd yomi
nix develop
```

Внутри доступны `cargo`, `rustc`, `rustfmt`, `clippy`, `rust-analyzer`, `just`,
а также `chafa` и ImageMagick — пригодятся при работе над выводом картинок
на этапе M1. Ничего из этого не попадает в систему: вышли из оболочки —
всё исчезло.

## direnv: чтобы не набирать `nix develop` каждый раз`

```nix
# configuration.nix
environment.systemPackages = with pkgs; [ direnv nix-direnv ];
programs.direnv.enable = true;   # если используете home-manager — там же
```

Затем в каталоге проекта один раз:

```bash
direnv allow
```

После этого окружение подхватывается автоматически при заходе в каталог
и снимается при выходе. Файл `.envrc` в репозитории уже лежит.

## Сборка пакета

```bash
nix build .#yomi
./result/bin/yomi --help
```

Результат — символическая ссылка `result` на неизменяемый путь в `/nix/store`.

## Запуск без установки

```bash
nix run . -- sources list
```

## Установка в систему

Когда проект дорастёт до ежедневного использования, в `configuration.nix`:

```nix
{
  inputs.yomi.url = "github:USERNAME/yomi";
  # ...
  environment.systemPackages = [ inputs.yomi.packages.${system}.default ];
}
```

До тех пор проще держать бинарник в `~/.cargo/bin` или запускать из `result/`.

## Частые сложности

**«Cargo.lock не найден» при `nix build`.** Файл `Cargo.lock` обязан быть
в репозитории (в `.gitignore` его нет — так и должно остаться). Без него
сборка невоспроизводима, и Nix откажется собирать.

**Нужны системные библиотеки.** На этапе M3 появится `reqwest`, которому
нужен OpenSSL. Раскомментируйте в `flake.nix`:

```nix
nativeBuildInputs = [ pkgs.pkg-config ];
buildInputs = [ pkgs.openssl ];
```

Те же строки добавьте в `devShell`, иначе `cargo build` вне `nix build`
не найдёт библиотеку.

**Когда версии Rust не хватает.** В `nixos-unstable` Rust достаточно свежий.
Если понадобится конкретная версия или nightly, добавьте вход
`rust-overlay` — но не раньше, чем это действительно потребуется: лишний
вход во флейке ломается чаще, чем помогает.

**`rust-analyzer` не видит стандартную библиотеку.** Переменная
`RUST_SRC_PATH` уже задана в `devShell`. Редактор нужно запускать
**изнутри** окружения (или пользоваться direnv), иначе он её не увидит.

**Установщик Claude Code не работает.** Это ожидаемо: стандартные скрипты
рассчитывают на `/usr/lib` и подобное, чего в NixOS нет. Ставьте через
`pkgs.claude-code` в `configuration.nix` либо добавьте пакет в `devShell`
этого флейка — тогда он будет доступен только внутри проекта.
