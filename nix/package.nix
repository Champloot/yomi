{ lib
, rustPlatform
, fetchFromGitHub
, installShellFiles
, makeWrapper
, unar
, withCbrSupport ? true
}:

rustPlatform.buildRustPackage rec {
  pname = "yomi";
  version = "0.1.0";

  src = fetchFromGitHub {
    owner = "Champloot";
    repo = "yomi";
    rev = "v${version}";
    hash = "sha256-VsQpqZq73djKKwxBLQZ1Rq2KsYmMOVwvuQlseTjQ/Ko="; # получить настоящий: nix build .#yomi, взять хэш из ошибки
  };

  # Реальный Cargo.lock проекта вендорится рядом (nix/Cargo.lock) — так
  # каждая крейт-зависимость фетчится напрямую с crates.io по чек-сумме
  # из самого лока, отдельный "vendor hash" считать не нужно.
  cargoLock = {
    lockFile = ./Cargo.lock;
  };

  nativeBuildInputs = [ installShellFiles ]
    ++ lib.optionals withCbrSupport [ makeWrapper ];

  # SQLite подключён через rusqlite с фичей `bundled` — компилируется из
  # исходников внутрь бинарника, системная либа не нужна. Компилятор C
  # для сборки вложенного sqlite уже есть в стандартном окружении
  # buildRustPackage, отдельно ничего добавлять не требуется.

  postInstall = ''
    installShellCompletion --cmd yomi \
      --bash <($out/bin/yomi generate completions bash) \
      --zsh <($out/bin/yomi generate completions zsh) \
      --fish <($out/bin/yomi generate completions fish)

    $out/bin/yomi generate manpage > yomi.1
    installManPage yomi.1
  '' + lib.optionalString withCbrSupport ''
    # unar нужен только для CBR/RAR — необязательная фича, поэтому
    # переключается через withCbrSupport, а не жёстко зашита в buildInputs.
    wrapProgram $out/bin/yomi --prefix PATH : ${lib.makeBinPath [ unar ]}
  '';

  meta = {
    description = "Терминальная читалка манги и комиксов (CBZ, PDF, CBR через unar, каталоги с картинками)";
    homepage = "https://github.com/Champloot/yomi";
    license = lib.licenses.mit;
    mainProgram = "yomi";
    platforms = lib.platforms.linux;
  };
}
