{
  description = "yomi — терминальная читалка манги и комиксов";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    let
      overlay = final: prev: {
        yomi = final.callPackage ./nix/package.nix { };
      };
    in
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ overlay ];
        };
      in
      {
        packages = {
          default = pkgs.yomi;
          yomi = pkgs.yomi;
        };

        devShells.default = pkgs.mkShell {
          inputsFrom = [ pkgs.yomi ];
          packages = with pkgs; [ cargo-watch clippy rustfmt ];
        };
      }
    ) // {
      overlays.default = overlay;
      nixosModules.default = import ./nix/module.nix;
      homeManagerModules.default = import ./nix/hm-module.nix;
    };
}
