# Home Manager module for task — Rust workflow CLI.
#
# Usage from a consuming flake:
#
#   inputs.task.url = "github:tsauvajon/task";
#   ...
#   imports = [ inputs.task.homeManagerModules.default ];
#   programs.task = {
#     enable = true;
#     reposDir = "~/dev/repos";
#     wtDir = "~/dev/wt";
#     detachedDir = "~/dev/detached";
#     editor = "helix";
#     opencodeCommand = "opencode-shared";
#     extraConfig = { vscodium.trusted_roots = [ "/path/one" ]; };
#   };
#
# The module installs the task binary and writes
# `~/.config/task/config.toml`. `extraConfig` is deep-merged onto the
# generated TOML so private machine-local options can flow through
# without losing structure.
self:
{
  config,
  lib,
  pkgs,
  ...
}:

let
  cfg = config.programs.task;
  # Keep in sync with DEFAULT_OPENCODE_COMMAND in src/runtime/config.rs.
  defaultOpencodeCommand = "opencode";
  validOpencodeCommand = command:
    let
      normalized = lib.strings.trim command;
      hasSeparator = lib.hasInfix "/" normalized;
      isAbsolute = lib.hasPrefix "/" normalized;
    in
    normalized != "" && (!hasSeparator || isAbsolute);
  opencodeCommandType = (types.addCheck types.nonEmptyStr validOpencodeCommand) // {
    description = "non-whitespace PATH executable name or absolute Unix path";
  };
  normalizedOpencodeCommand = lib.strings.trim cfg.opencodeCommand;
  inherit (lib)
    mkEnableOption
    mkOption
    mkIf
    types
    ;

  baseConfig = {
    repos_dir = cfg.reposDir;
    wt_dir = cfg.wtDir;
    detached_dir = cfg.detachedDir;
    editor = cfg.editor;
  }
  // lib.optionalAttrs (normalizedOpencodeCommand != defaultOpencodeCommand) {
    opencode.command = normalizedOpencodeCommand;
  };

  mergedConfig = lib.recursiveUpdate baseConfig cfg.extraConfig;

  tomlFormat = pkgs.formats.toml { };
in
{
  options.programs.task = {
    enable = mkEnableOption "task — workflow CLI for repos and worktrees";

    package = mkOption {
      type = types.package;
      default = self.packages.${pkgs.stdenv.hostPlatform.system}.default;
      defaultText = lib.literalExpression "task.packages.<system>.default";
      description = "The task package to install.";
    };

    reposDir = mkOption {
      type = types.str;
      default = "~/dev/repos";
      description = "Where bare clones live.";
    };

    wtDir = mkOption {
      type = types.str;
      default = "~/dev/wt";
      description = "Where feature-branch worktrees live.";
    };

    detachedDir = mkOption {
      type = types.str;
      default = "~/dev/detached";
      description = "Where detached default-branch worktrees live.";
    };

    editor = mkOption {
      type = types.str;
      default = "helix";
      description = "Default editor task should open files in.";
    };

    opencodeCommand = mkOption {
      type = opencodeCommandType;
      default = defaultOpencodeCommand;
      description = ''
        PATH-resolvable executable name or absolute Unix path used to launch OpenCode.
        Relative paths and whitespace-only values are rejected.
      '';
    };

    extraConfig = mkOption {
      type = types.attrsOf types.anything;
      default = { };
      example = {
        vscodium.trusted_roots = [ "/path/one" ];
      };
      description = ''
        Extra configuration deep-merged onto the generated config.toml.
        Use this for sections task does not yet expose as typed options
        (e.g. `[vscodium]`, machine-local extensions).
      '';
    };

  };

  config = mkIf cfg.enable {
    home.packages = [ cfg.package ];

    xdg.configFile."task/config.toml".source =
      tomlFormat.generate "task-config.toml" mergedConfig;
  };
}
