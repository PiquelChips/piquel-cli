{ pkgs, ... }:
pkgs.mkShell {
    inputsFrom = [ (pkgs.callPackage ./pkg.nix { }) ];
    packages = with pkgs; [
        cargo
        rustc
        rustfmt
        clippy
        rust-analyzer
        fzf
        git
        tmux
    ];
}
