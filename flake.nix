{
    description = "Piquel CLI";
    
    inputs = {
        nixpkgs.url = "github:nixos/nixpkgs?ref=nixos-25.11";
        flake-utils.url = "github:numtide/flake-utils";
    };
    
    outputs = { self, nixpkgs, flake-utils }: 
    {
        nixosModules.default = import ./nix/module.nix;
    } //
    flake-utils.lib.eachDefaultSystem (system:
        let
            inherit (self) outputs;
            pkgs = import nixpkgs {inherit system;};
            piquelcli = pkgs.callPackage ./nix/pkg.nix { };
        in
        {
            packages.default = piquelcli;
            checks =
                {
                    package = piquelcli;

                    rust-workflow = pkgs.rustPlatform.buildRustPackage {
                        pname = "piquelcli-rust-workflow";
                        version = piquelcli.version;
                        src = pkgs.lib.cleanSource ./.;
                        cargoLock.lockFile = ./Cargo.lock;
                        nativeBuildInputs = with pkgs; [
                            rustfmt
                            clippy
                            fzf
                            git
                            tmux
                        ];
                        buildPhase = ''
                            runHook preBuild
                            cargo fmt --check
                            cargo clippy --all-targets --all-features -- -D warnings
                            runHook postBuild
                        '';
                        checkPhase = ''
                            runHook preCheck
                            cargo test --all-targets --all-features
                            runHook postCheck
                        '';
                        installPhase = ''
                            runHook preInstall
                            touch "$out"
                            runHook postInstall
                        '';
                    };
                } // pkgs.lib.optionalAttrs pkgs.stdenv.isLinux {
                    nixos-module =
                        (nixpkgs.lib.nixosSystem {
                            inherit system;
                            modules = [
                                self.nixosModules.default
                                {
                                    programs.piquelcli = {
                                        enable = true;
                                        settings = {
                                            default_session = "default";
                                            sessions.default.windows = [
                                                { commands = [ "echo ready" ]; }
                                            ];
                                            projects = [
                                                {
                                                    repository = "https://github.com/PiquelChips/piquel-cli.git";
                                                    name = "piquel-cli";
                                                    default_session = "default";
                                                }
                                                {
                                                    repository = "git@github.com:owner/inline.git";
                                                    name = "inline";
                                                    default_session.windows = [
                                                        { commands = [ "cargo check" ]; }
                                                    ];
                                                }
                                            ];
                                        };
                                    };
                                }
                            ];
                        }).config.programs.piquelcli.package;
                };
            devShells.default = import ./nix/shell.nix { inherit outputs system pkgs; };
        }
    );
}
