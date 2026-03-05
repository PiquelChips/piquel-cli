{
    description = "Piquel CLI";
    
    inputs = {
        nixpkgs.url = "github:nixos/nixpkgs?ref=nixos-25.11";
        flake-utils.url = "github:numtide/flake-utils";
    };
    
    outputs = { self, nixpkgs, flake-utils }: 
    flake-utils.lib.eachDefaultSystem (system:
        let
            pkgs = import nixpkgs {inherit system;};
            manifest = (pkgs.lib.importTOML ./Cargo.toml).package;
        in
        {
            packages = rec {
                piquel = pkgs.rustPlatform.buildRustPackage {
                    pname = manifest.name;
                    version = manifest.version;
                    src = pkgs.lib.cleanSource ./.;
                    cargoLock.lockFile = ./Cargo.lock;
                    postInstall = ''
                        mv $out/bin/piquel-cli $out/bin/piquel
                    '';
                };
                default = piquel;
            };
        }
    );
}
