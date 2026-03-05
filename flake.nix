{
    description = "Piquel CLI";
    
    inputs = {
        nixpkgs.url = "github:nixos/nixpkgs?ref=nixos-25.11";
        flake-utils.url = "github:numtide/flake-utils";
    };
    
    outputs = { self, nixpkgs, flake-utils }: 
    let
        inherit (self) outputs;
    in
    flake-utils.lib.eachDefaultSystem (system:
        let
            pkgs = import nixpkgs {inherit system;};
            manifest = (pkgs.lib.importTOML ./Cargo.toml).package;
            piquelcli = pkgs.rustPlatform.buildRustPackage {
                pname = manifest.name;
                version = manifest.version;
                src = pkgs.lib.cleanSource ./.;
                cargoLock.lockFile = ./Cargo.lock;
            };
        in
        {
            packages = {
                piquel = piquelcli;
                piquelcli = piquelcli;
                default = piquelcli;
            };
            devShells.default = import ./nix/shell.nix { inherit outputs system pkgs; };
        }
    );
}
