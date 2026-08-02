{ pkgs }:
let
  manifest = (pkgs.lib.importTOML ../Cargo.toml).package;
in
pkgs.rustPlatform.buildRustPackage {
  pname = manifest.name;
  version = manifest.version;
  src = pkgs.lib.cleanSource ../.;
  cargoLock.lockFile = ../Cargo.lock;
  nativeBuildInputs = [ pkgs.installShellFiles ];

  postInstall = ''
    installShellCompletion --cmd piquel \
      --bash <($out/bin/piquel completions bash) \
      --fish <($out/bin/piquel completions fish) \
      --zsh <($out/bin/piquel completions zsh)
  '';
}
