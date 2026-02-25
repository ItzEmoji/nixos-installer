{
  description = "A TUI-based NixOS installer for nixos-dots";

  nixConfig = {
    substituters = [
      "https://cache.itzemoji.com/nix"
      "https://cache.nixos.org"
    ];

    trusted-public-keys = [
      "nix:U22mA6l/Br6W9STnaHWO2LPvUCNVuh1yTEIlTCtjtkg="
      "cache.nixos.org-1:6NCHdD59X431o0gWypbMrAURkbJ16ZPMQFGspcDShjY="
    ];
  };
  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs?ref=nixos-unstable";
    flake-parts.url = "github:hercules-ci/flake-parts";
    nci.url = "github:90-008/nix-cargo-integration";
    nci.inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs =
    inputs:
    inputs.flake-parts.lib.mkFlake { inherit inputs; } {
      imports = [
        inputs.nci.flakeModule
      ];
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      flake =
        { self, ... }:
        {
          nixosModules.nixos-installer =
            { pkgs, ... }:
            {
              environment.systemPackages = [
                self.packages.${pkgs.system}.default
              ];
            };
          homeManagerModules.nixos-installer =
            { pkgs, ... }:
            {
              home.packages = [
                self.packages.${pkgs.system}.default
              ];
            };
          flakeModuels.nixos-installer = {
            perSystem =
              { pkgs, system, ... }:
              {
                app.default = {
                  program = "${self.packages.${system}.nvim}/bin/nvim";
                };
              };

          };
        };
      perSystem =
        { config, ... }:
        let
          crateOutputs = config.nci.outputs."nixos-installer";
        in
        {
          nci.projects."nixos-installer".path = ./.;
          nci.crates."nixos-installer" = { };

          packages.default = crateOutputs.packages.release;
          devShells.default = crateOutputs.devShell;
        };
    };
}
