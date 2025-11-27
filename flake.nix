{
  description = "Rust development environment with fenix and crane";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";

    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    crane = {
      url = "github:ipetkov/crane";
    };
  };

  outputs = {
    self,
    nixpkgs,
    flake-utils,
    fenix,
    crane,
  }:
    flake-utils.lib.eachDefaultSystem (system: let
      overlays = [
        fenix.overlays.default
      ];

      pkgs = import nixpkgs {
        inherit system overlays;
      };

      toolchain = pkgs.fenix.fromToolchainFile {
        file = ./rust-toolchain.toml;
        sha256 = "sha256-+9FmLhAOezBZCOziO0Qct1NOrfpjNsXxc/8I0c7BdKE=";
      };
      craneLib = (crane.mkLib pkgs).overrideToolchain toolchain;

      src = craneLib.cleanCargoSource ./.;

      commonArgs = {
        inherit src;
        buildInputs = with pkgs;
          [
            # Add any system dependencies here
          ]
          ++ pkgs.lib.optionals pkgs.stdenv.isDarwin [
            # macOS specific dependencies
            pkgs.libiconv
          ];
      };

      cargoArtifacts = craneLib.buildDepsOnly commonArgs;

      package = craneLib.buildPackage (commonArgs
        // {
          inherit cargoArtifacts;
          meta = {
            description = "Rust application";
            mainProgram = "hyprhist";
          };
        });
    in {
      checks = {
        inherit package;

        clippy = craneLib.cargoClippy (commonArgs
          // {
            inherit cargoArtifacts;
            cargoClippyExtraArgs = "--all-targets -- -Wclippy::pedantic -Dclippy::pedantic --deny warnings";
          });

        fmt = craneLib.cargoFmt {
          inherit src;
        };

        tests = craneLib.cargoNextest (commonArgs
          // {
            inherit cargoArtifacts;
          });
      };

      packages.default = package;

      apps.default = flake-utils.lib.mkApp {
        drv = package;
      };

      devShells.default = craneLib.devShell {
        checks = self.checks.${system};

        packages = with pkgs;
          [
            # Development tools
            cargo-watch
            cargo-audit
            cargo-deny
            cargo-edit
            cargo-outdated

            # Additional tools
            git
          ]
          ++ [ toolchain ]
          ++ commonArgs.buildInputs;

        shellHook = ''
          echo "Rust development environment ready!"
          echo "Rust version: $(rustc --version)"
          echo "Cargo version: $(cargo --version)"
        '';
      };
    });
}
