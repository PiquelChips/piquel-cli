# nixos/module.nix
{
    config,
    lib,
    pkgs,
    ...
}:
let
    cfg = config.programs.piquelcli;

    settingsFormat = pkgs.formats.json { };
    configFile = settingsFormat.generate "piquel-cli.json" cfg.settings;

    wrappedPiquel =
        pkgs.runCommand "piquelcli-wrapped"
            {
                nativeBuildInputs = [ pkgs.makeWrapper ];
            }
            ''
                makeWrapper ${pkgs.piquel}/bin/piquelcli $out/bin/piquel \
                    --add-flags "--config ${configFile}"
            '';

    piquelcli = wrappedPiquel;
in
{
    options.programs.piquelcli = {
        enable = lib.mkEnableOption "Enable piquelcli";

        package = lib.mkOption {
            type = lib.types.package;
            default = piquelcli;
            defaultText = lib.literalExpression "pkgs.piquel-cli";
            example = lib.literalExpression "pkgs.piquel-cli";
        };

        settings = lib.mkOption {
            description = "The configuration being passed to the CLI";
            type = lib.types.submodule {
                freeformType = settingsFormat.type;
                options =
                let
                    inherit (lib) mkOption types;

                    windowConfigType = types.submodule {
                        options = {
                            commnads = mkOption {
                                type = types.listOf types.str;
                                default = [];
                                description = "List of commands to run in the window";
                            };
                        };
                    };

                    sessionConfigType = types.submodule {
                        options = {
                            root = mkOption {
                                type = types.str;
                                description = "Root directory for the session.";
                            };
                            windows = mkOption {
                                type = types.listOf windowConfigType;
                                default = [];
                                description = "Windows in this session.";
                            };
                        };
                    };
                in
                {
                    sessions = mkOption {
                        type = types.attrsOf sessionConfigType;
                        default  = {};
                        description = "Named sessions, each with a root path and windows.";
                    };

                    validateSessionRoot = mkOption {
                        type = types.bool;
                        default = false;
                        description = "Whetther to validate that the session root is an actual path";
                    };

                    defaultSession = mkOption {
                        type = types.listOf windowConfigType;
                        default = [];
                        description = "Default session to create with any root";
                    };
                };
            };
        };
    };

    config = lib.mkIf cfg.enable {
        environment.systemPackages = [ piquelcli ];
    };
}
