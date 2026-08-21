{ config, lib, pkgs, ... }:
let
  cfg = config.programs.yomi;
  tomlFormat = pkgs.formats.toml { };
in
{
  options.programs.yomi = {
    enable = lib.mkEnableOption "yomi, терминальную читалку манги и комиксов";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.yomi;
      defaultText = lib.literalExpression "pkgs.yomi";
      description = ''
        Пакет yomi. Чтобы отключить поддержку CBR/RAR (через unar),
        передай сюда `pkgs.yomi.override { withCbrSupport = false; }`.
      '';
    };

    settings = lib.mkOption {
      type = tomlFormat.type;
      default = { };
      example = lib.literalExpression ''
        {
          library.paths = [ "/home/archi/manga" ];
          reader.direction = "right-to-left";
        }
      '';
      description = ''
        Настройки yomi — пишутся в `~/.config/yomi/config.toml`.

        Конфиг разбирается строго: незнакомый ключ — это ошибка, а не
        молчаливо проигнорированная опечатка, yomi откажется запускаться
        с понятным сообщением.

        Файл, созданный этой опцией, доступен только для чтения —
        команда `yomi config init`, если её запустить поверх него,
        закономерно откажется перезаписывать. Это ожидаемо: конфиг
        теперь под управлением Nix, а не ручного редактирования.

        Пустой `settings` (значение по умолчанию) означает, что файл
        конфига вообще не создаётся — yomi в этом случае работает на
        встроенных умолчаниях самой программы.
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    home.packages = [ cfg.package ];

    xdg.configFile."yomi/config.toml" = lib.mkIf (cfg.settings != { }) {
      source = tomlFormat.generate "yomi-config.toml" cfg.settings;
    };
  };
}
