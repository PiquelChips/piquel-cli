{
    description = "Piquel CLI";
    
    inputs = {
        nixpkgs.url = "github:nixos/nixpkgs?ref=nixos-24.11";
        flake-utils.url = "github:numtide/flake-utils";
    };
    
    outputs = { self, nixpkgs, flake-utils }: 
    flake-utils.lib.eachDefaultSystem (system:
        let
            pkgs = import nixpkgs {inherit system;};
        in
        {
            packages = rec {
                piquel = pkgs.buildGoModule {
                    pname = "piquel";
                    version = "0.1.0";
                    src = ./.;
                    vendorHash = "sha256-sZUEzBxbButVYi8eFxyrqCQI51a8rUDXpvO1JUxSmjU=";
                    postInstall = ''
                        mv $out/bin/piquel-cli $out/bin/piquel
                    '';
                };
                default = piquel;
            };
        }
    );
}
