{ config, lib, pkgs, ... }:
let
  cfg = config.programs.yomi;
in
{
  options.programs.yomi = {
    enable = lib.mkEnableOption "yomi для всех пользователей системы (environment.systemPackages)";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.yomi;
      defaultText = lib.literalExpression "pkgs.yomi";
      description = "Пакет yomi для системной установки.";
    };
  };

  config = lib.mkIf cfg.enable {
    environment.systemPackages = [ cfg.package ];
  };
}
