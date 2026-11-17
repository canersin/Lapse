{
  description = "Lapse: save the last moments of your game and trim clips";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs =
    { self, nixpkgs }:
    let
      systems = [ "x86_64-linux" ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
    in
    {
      packages = forAllSystems (pkgs: rec {
        lapse = pkgs.callPackage ./nix/package.nix { };
        default = lapse;
      });

      # `nix run github:canersin/Lapse` starts the service and the window;
      # with arguments it passes them on (`nix run ... -- clip`).
      apps = forAllSystems (
        pkgs:
        let
          lapse = self.packages.${pkgs.stdenv.hostPlatform.system}.default;
          run = pkgs.writeShellScript "lapse-run" ''
            if [ $# -eq 0 ]; then exec ${lapse}/bin/lapse start; fi
            exec ${lapse}/bin/lapse "$@"
          '';
        in
        {
          default = {
            type = "app";
            program = "${run}";
          };
        }
      );

      overlays.default = final: _prev: { lapse = final.callPackage ./nix/package.nix { }; };
    };
}
