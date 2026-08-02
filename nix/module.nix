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
  basePiquel = pkgs.callPackage ./pkg.nix { };

  wrappedPiquel =
    pkgs.symlinkJoin {
      name = "piquelcli-wrapped";
      paths = [ basePiquel ];
      nativeBuildInputs = [ pkgs.makeWrapper ];
      postBuild = ''
        wrapProgram $out/bin/piquel \
            --set PIQUEL_CONFIG ${configFile} \
            --prefix PATH : ${
              lib.makeBinPath [
                pkgs.fzf
                pkgs.git
                pkgs.openssh
                pkgs.tmux
              ]
            }
      '';
    };

  piquelcli = wrappedPiquel;
in
{
  options.programs.piquelcli = {
    enable = lib.mkEnableOption "piquelcli";

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
                name = mkOption {
                  type = types.nullOr types.str;
                  default = null;
                  description = "Optional tmux window name.";
                };
                commands = mkOption {
                  type = types.listOf types.str;
                  default = [ ];
                  description = "List of commands to run in the window";
                };
              };
            };

            sessionConfigType = types.submodule {
              options = {
                windows = mkOption {
                  type = types.listOf windowConfigType;
                  default = [ ];
                  description = "Windows in this session template.";
                };
              };
            };

            projectConfigType = types.submodule {
              options = {
                repository = mkOption {
                  type = types.str;
                  description = "Git repository URL for the project.";
                };
                name = mkOption {
                  type = types.nullOr types.str;
                  default = null;
                  description = "Optional configured project name.";
                };
                path = mkOption {
                  type = types.nullOr types.str;
                  default = null;
                  description = "Optional local project path.";
                };
                default_session = mkOption {
                  type = types.nullOr (
                    types.oneOf [
                      types.str
                      sessionConfigType
                    ]
                  );
                  default = null;
                  description = "Optional default session template name or inline session config for this project.";
                };
              };
            };

            machineConfigType = types.submodule {
              options = {
                name = mkOption {
                  type = types.str;
                  description = "Machine name used by piquel's -m/--machine flag.";
                };
                address = mkOption {
                  type = types.str;
                  description = "SSH server address for the machine.";
                };
                username = mkOption {
                  type = types.str;
                  description = "SSH username for the machine.";
                };
              };
            };
          in
          {
            projects_dir = mkOption {
              type = types.str;
              default = "~/Projects";
              description = "Default directory for local projects.";
            };

            worktrees_dir = mkOption {
              type = types.str;
              default = "~/.piquel/worktrees";
              description = "Default directory for managed project worktrees.";
            };

            default_session = mkOption {
              type = types.str;
              default = "default";
              description = "Global default session template name.";
            };

            sessions = mkOption {
              type = types.attrsOf sessionConfigType;
              default = { };
              description = "Named reusable session templates.";
            };

            projects = mkOption {
              type = types.listOf projectConfigType;
              default = [ ];
              description = "Configured projects.";
            };

            machines = mkOption {
              type = types.listOf machineConfigType;
              default = [ ];
              description = "Configured SSH machines.";
            };
          };
      };
    };
  };

  config = lib.mkIf cfg.enable {
    environment.systemPackages = [ cfg.package ];
  };
}
