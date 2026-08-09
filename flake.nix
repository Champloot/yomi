{
  description = "yomi — терминальная читалка манги";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; };

        # Библиотеки, которые понадобятся на следующих этапах:
        # openssl — для HTTP-клиента (M3), sqlite — для библиотеки (M2).
        buildInputs = with pkgs; [ openssl sqlite ];
        nativeBuildInputs = with pkgs; [ pkg-config ];
      in
      {
        packages.default = pkgs.rustPlatform.buildRustPackage {
          pname = "yomi";
          version = "0.1.0";
          src = ./.;

          cargoLock.lockFile = ./Cargo.lock;

          inherit buildInputs nativeBuildInputs;

          meta = with pkgs.lib; {
            description = "Терминальная читалка манги для Linux";
            license = licenses.mit;
            mainProgram = "yomi";
            platforms = platforms.linux;
          };
        };

        devShells.default = pkgs.mkShell {
          inherit buildInputs nativeBuildInputs;

          packages = with pkgs; [
            rustc
            cargo
            rustfmt
            clippy
            rust-analyzer   # языковой сервер для редактора
            cargo-watch     # cargo watch -x test — пересборка при правках
            cargo-edit      # cargo add / cargo rm
          ];

          # Нужно, чтобы rust-analyzer находил исходники стандартной библиотеки.
          RUST_SRC_PATH = "${pkgs.rustPlatform.rustLibSrc}";

          shellHook = ''
            echo "yomi: окружение готово. rustc $(rustc --version | cut -d' ' -f2)"
            echo "Полезное: cargo test | cargo clippy | cargo watch -x test"
          '';
        };
      });
}
