{
  # This Nix flake represents a two-pass build:
  #
  #   1. Build a C archive from CometBFT golang code;
  #   2. Statically link that C archive into Rust binary via build.rs.
  #
  # Therefore there are two packages defined to declare the dependency.
  # The project's `build.rs` is smart enough to build the golang code on the fly,
  # but nix doesn't permit network access in the buildPhase, so we need to be explicit
  # about the `libcometbft.a` file being an ouput of one derivation and the input to another.
  description = "A nix development shell and build environment for penumbra-reindexer";

  inputs = {
    # nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-24.11";
    flake-utils.url = "github:numtide/flake-utils";
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
      inputs.rust-analyzer-src.follows = "";
    };
    crane = {
      url = "github:ipetkov/crane";
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
          # Build a C-archive via the cometbft golang module,
          # for linking into the `penumbra-reindexer` binary at build time.
          cometbftArchive = pkgs.buildGoModule {
            pname = "libcometbft-archive";
            # Extract the verison string from the cometbft golang dep, and use that as the version
            # for the C archive we're building.
            version = builtins.head (
              builtins.match ".*github.com/cometbft/cometbft (v[0-9]+\\.[0-9]+\\.[0-9]+).*" (
                builtins.readFile ./go/go.mod
              )
            );

            # The checksum represents all golang dependencies, and will change whenever deps are bumped.
            # To bump a golang dep:
            #
            #   1. edit the `go/go.mod` file with the new versions
            #   2. run `go mod tidy` within the `go` directory
            #   3. run `nix build`, view mismatched hash, update `vendorHash` value below.
            #
            vendorHash = "sha256-fxvcw9oqRsANg0P+QVc1idAxkiDSbtcVntU6eoLjox0=";

            # Ensure Go doesn't treat the golang source directory as GOPATH;
            # this is only necessary because we've named the subdir `go`.
            src = ./go;
            preBuild = ''
              export GOPATH="$TMPDIR/go-path"
              export GOCACHE="$TMPDIR/go-cache"
              export GO111MODULE=on
            '';

            # Override the buildPhase to provide instructions for C archive.
            buildPhase = ''
              runHook preBuild
              go build -buildmode=c-archive -o libcometbft.a ./cometbft.go
              runHook postBuild
            '';

            # Override the installPhase to copy the built artifacts, and that's all.
            # Technically we only need it to land in `$lib`, but we'll copy it to `$out/`
            # as well, so it's easier to debug, or reference directly from a non-nix
            # `cargo build`.
            outputs = [ "out" "lib" ];
            installPhase = ''
              runHook preInstall
              mkdir -p $lib/lib $out
              cp libcometbft.a $lib/lib/
              cp libcometbft.a $out/
              runHook postInstall
            '';

            # Don't run checks; will fail on libs, rather than binaries.
            doCheck = false;

          };
          # Build the `penumbra-reindexer` binary
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
            buildInputs = [
              clang openssl rocksdb go cometbftArchive
              ] ++ lib.optionals pkgs.stdenv.isDarwin [
                # mac-only deps
                pkgs.darwin.apple_sdk.frameworks.SystemConfiguration
                pkgs.darwin.apple_sdk.frameworks.CoreServices
            ];

            inherit system LIBCLANG_PATH ROCKSDB_LIB_DIR;

            # Declare the custom-built C archive via env var, so `build.rs` picks it up.
            # The library path is accessible to the rust build due to `cometbftArchive`
            # being included as a `buildInput`.
            preBuild = ''
              export PENUMBRA_REINDEXER_STATIC_LIB="${cometbftArchive.lib}/lib/libcometbft.a";
            '';
            cargoExtraArgs = "-p penumbra-reindexer";
            meta = {
              description = "A reindexing tool for Penumbra ABCI event data";
              homepage = "https://penumbra.zone";
              license = [ licenses.mit licenses.asl20 ];
            };
          }).overrideAttrs (_: { doCheck = false; }); # Disable tests to improve build times

        in {
          packages = {
            inherit penumbraReindexer ;
            default = penumbraReindexer;
          };
          apps = {
            penumbra-reindexer.type = "app";
            penumbra-reindexer.program = "${penumbraReindexer}/bin/penumbra-reindexer";
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
