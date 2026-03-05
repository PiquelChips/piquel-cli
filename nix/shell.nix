{ outputs, pkgs, system, ... }:
pkgs.mkShell {
    inputsFrom = [ outputs.packages.${system}.default ];
    packages = with pkgs; [
        cargo
        rustc
        rustfmt
        clippy
        rust-analyzer
    ];
}
