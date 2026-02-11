{
  description = "LLZero Benchmark Environment";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-25.11";
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, fenix, ... }:
    let
      supportedSystems =
        [ "aarch64-linux" "x86_64-linux" "aarch64-darwin" "x86_64-darwin" ];
      forEachSupportedSystem = f:
        nixpkgs.lib.genAttrs supportedSystems (system:
          f {
            pkgs = import nixpkgs {
              inherit system;
              overlays = [ self.overlays.default ];
            };
          });
    in {
      # Overlay for the Rust toolchain
      overlays.default = final: prev: {
        rustToolchain =
          fenix.packages.${prev.stdenv.hostPlatform.system}.fromToolchainFile {
            file = ./rust-toolchain.toml;
            sha256 = "sha256-vra6TkHITpwRyA5oBKAHSX0Mi6CBDNQD+ryPSpxFsfg=";
          };
      };

      # Inputs for shell environments
      devShells = forEachSupportedSystem ({ pkgs }: {
        default = pkgs.mkShell.override { stdenv = pkgs.clangStdenv; } {
          buildInputs = with pkgs;
            [
              rustToolchain
              cargo-deny
              cargo-edit
              cargo-watch
              lldb
              openssl
              wasm-pack
              bun
            ];
          env = {
            # Rust-analyzer
            RUST_SRC_PATH =
              "${pkgs.rustToolchain}/lib/rustlib/src/rust/library";
            RUST_LOG = "info";
            EXPIRY_DAYS = "1";
          };
        };
      });
    };
}
