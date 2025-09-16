{
  description = "Reth Proofs";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs";
    crane.url = "github:ipetkov/crane";
    flake-parts.url = "github:hercules-ci/flake-parts";
    treefmt-nix.url = "github:numtide/treefmt-nix";
    treefmt-nix.inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs =
    inputs@{
      flake-parts,
      treefmt-nix,
      crane,
      ...
    }:
    flake-parts.lib.mkFlake { inherit inputs; } {
      systems = [ "x86_64-linux" ];

      imports = [
        treefmt-nix.flakeModule
      ];

      perSystem =
        {
          self',
          pkgs,
          lib,
          system,
          ...
        }:
        let
          craneLib = crane.mkLib pkgs;

          rethProofsAttrs = {
            pname = "reth-proofs";
            src = craneLib.cleanCargoSource ./.;
            strictDeps = true;
            nativeBuildInputs = [ pkgs.pkg-config ];
            buildInputs = with pkgs; [
              openssl.dev
            ];
            # NOTE: We cannot just build the whole workspace due to zkVMs. Add more `-p pname` when needed.
            cargoExtraArgs = "-p reth-proofs-core"; 
          };
        in
        {
          packages = {
            reth-proofs-deps = craneLib.buildDepsOnly rethProofsAttrs;

            reth-proofs-core = craneLib.buildPackage (rethProofsAttrs // {
              pname = "reth-proofs-core";
              cargoArtifacts = self'.packages.reth-proofs-deps;
              cargoExtraArgs = "-p reth-proofs-core";
            });
          };

          devShells.default = pkgs.mkShell {
            shellHook = ''
              echo "Shell is ready!"
            '';
          };

          # Formatter for `nix fmt`.
          treefmt = {
            projectRootFile = "flake.nix";
            programs.nixfmt.enable = true;
            programs.rustfmt.enable = true;
          };
        };
    };
}
