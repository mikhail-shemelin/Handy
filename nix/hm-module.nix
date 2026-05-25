# Home-manager module for Handy Hybrid speech-to-text
#
# Provides a systemd user service for autostart.
# Usage: imports = [ handy-hybrid.homeManagerModules.default ];
#        services.handyHybrid.enable = true;
{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.services.handyHybrid;
in
{
  options.services.handyHybrid = {
    enable = lib.mkEnableOption "Handy Hybrid speech-to-text user service";

    package = lib.mkOption {
      type = lib.types.package;
      defaultText = lib.literalExpression "handy-hybrid.packages.\${system}.handy";
      description = "The Handy Hybrid package to use.";
    };
  };

  config = lib.mkIf cfg.enable {
    systemd.user.services.handy-hybrid = {
      Unit = {
        Description = "Handy Hybrid speech-to-text";
        After = [ "graphical-session.target" ];
        PartOf = [ "graphical-session.target" ];
      };
      Service = {
        ExecStart = "${cfg.package}/bin/handy-hybrid";
        Restart = "on-failure";
        RestartSec = 5;
      };
      Install.WantedBy = [ "graphical-session.target" ];
    };
  };
}
