{
  description = "A nix development shell and build environment for penumbra-reindexer";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
      inputs.rust-analyzer-src.follows = "";
    };
    crane = {
      url = "github:ipetkov/crane";
      inputs = { nixpkgs.follows = "nixpkgs"; };
    };
  };

  outputs = { self, nixpkgs, flake-utils, crane, ... }:
    flake-utils.lib.eachDefaultSystem
      (system:
        let
          pkgs = import nixpkgs { inherit system; };
          # Permit version declarations, but default to unset,
          # meaning the local working copy will be used.
          penumbraReindexerRelease = null;

          # Set up for Rust builds.
          craneLib = crane.mkLib pkgs;

          # Important environment variables so that the build can find the necessary libraries
          LIBCLANG_PATH="${pkgs.libclang.lib}/lib";
          ROCKSDB_LIB_DIR="${pkgs.rocksdb.out}/lib";
        in with pkgs; with pkgs.lib; let
          # All the Penumbra binaries
          penumbraReindexer = (craneLib.buildPackage {
            pname = "penumbra-reindexer";
            # what
            src = cleanSourceWith {
              src = if penumbraReindexerRelease == null then craneLib.path ./. else fetchFromGitHub {
                owner = "penumbra-zone";
                repo = "reindexer";
                rev = "v${penumbraReindexerRelease.version}";
                sha256 = "${penumbraReindexerRelease.sha256}";
              };
              filter = path: type:
                # Retain non-rust files as build inputs:
                # * sql: database schema files for indexing
                # * go, mod, sum: golang files for linking in cometbft
                (builtins.match ".*\.(sql|go|mod|sum)$" path != null) ||
                # ... as well as all the normal cargo source files:
                (craneLib.filterCargoSources path type);
            };
            nativeBuildInputs = [ pkg-config ];
            buildInputs = if stdenv.hostPlatform.isDarwin then 
              with pkgs.darwin.apple_sdk.frameworks; [clang openssl rocksdb SystemConfiguration CoreServices go]
            else
              [clang openssl rocksdb go];

            inherit system LIBCLANG_PATH ROCKSDB_LIB_DIR;
            cargoExtraArgs = "-p penumbra-reindexer";
            meta = {
              description = "A reindexing tool for Penumbra ABCI event data";
              homepage = "https://penumbra.zone";
              license = [ licenses.mit licenses.asl20 ];
            };
          }).overrideAttrs (_: { doCheck = false; }); # Disable tests to improve build times

        in rec {
          packages = { inherit penumbraReindexer ; };
          apps = {
            penumbra-reindexer.type = "app";
            penumbra-reindexer.program = "${penumbraReindexer}/bin/penumbra-reindexer";
          };
          defaultPackage = symlinkJoin {
            name = "penumbra-reindexer";
            paths = [ penumbraReindexer ];
          };
          devShells.default = craneLib.devShell {
            inherit LIBCLANG_PATH ROCKSDB_LIB_DIR;
            inputsFrom = [ penumbraReindexer ];
            packages = [
              cargo-nextest
              cargo-watch
              go
              just
              nix-prefetch-scripts
              sqlfluff
            ];
            shellHook = ''
              export LIBCLANG_PATH=${LIBCLANG_PATH}
              export ROCKSDB_LIB_DIR=${ROCKSDB_LIB_DIR}
            '';
          };
        }
      );
}
