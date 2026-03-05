# nixos/module.nix
{
    config,
    lib,
    pkgs,
    ...
}:
let
    cfg = config.programs.piquelcli;
    configFile = pkgs.writeText "config.json" (builtins.toJSON cfg.settings);
    wrappedPiquel =
        pkgs.runCommand "piquelcli-wrapped"
            {
                nativeBuildInputs = [ pkgs.makeWrapper ];
            }
            ''
                makeWrapper ${pkgs.piquel}/bin/piquelcli $out/bin/piquel \
                    --add-flags "--config ${configFile}"
            '';
in
{
    options.programs.piquel = {
        enable = lib.mkEnableOption "Enable piquelcli";
        settings = lib.mkOption {
            type = lib.types.attrs;
            default = { };
        };
    };
    config = lib.mkIf cfg.enable {
        environment.systemPackages = [ wrappedPiquel ];
    };
}
