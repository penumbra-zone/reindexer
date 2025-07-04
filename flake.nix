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
    # Pull in penumbra repo so we have access to `pd` on cli for integration tests.
    penumbra-repo = {
      # The CLI remains backward compatible as of v2.0.1,
      # so it's fine to use only one version of `pd` to initial network dirs.
      url = "github:penumbra-zone/penumbra/v2.0.1";
    };
  };

  outputs = { self, nixpkgs, flake-utils, crane, penumbra-repo, ... }:
    let
      # Read the application version from the local `Cargo.toml` file.
      cargoToml = builtins.fromTOML (builtins.readFile ./Cargo.toml);
      version = cargoToml.package.version;
    in
    {
      # Export the version so it's accessible outside the build context
      inherit version;
    } // (flake-utils.lib.eachDefaultSystem
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


          # Container image for shipping the reindexer.
          containerImage = pkgs.dockerTools.buildLayeredImage {
            name = "penumbra-reindexer";
            tag = version;

            contents = pkgs.buildEnv {
              name = "penumbra-reindexer-container-packages";
              paths = [
                # The `penumbra-reindexer` binary
                penumbraReindexer

                # Basic system utilities, to provide a barebones environment
                pkgs.cacert
                pkgs.bash
                pkgs.coreutils
                pkgs.findutils
                pkgs.tzdata
                pkgs.dockerTools.shadowSetup
                pkgs.shadow

                # Tools useful for wrangling archives
                pkgs.curl
                pkgs.gnutar
                pkgs.gzip
                pkgs.lz4
                pkgs.pigz
                pkgs.sqlite
                pkgs.xz


              ];
              pathsToLink = ["/bin" "/etc" "share"];
            };

            # Create non-root user dirs. The `extraCommands` step runs
            # after nix deps are added, before image is final.
            fakeRootCommands = ''
              ${dockerTools.shadowSetup}
              groupadd --gid 1000 penumbra
              useradd -m -d /home/penumbra -g 1000 -u 1000 penumbra
            '';
            enableFakechroot = true;

            config = {
              Cmd = [ "${penumbraReindexer}/bin/penumbra-reindexer"];
              User = "1000";
              WorkingDir = "/home/penumbra";
              Env = [
                "HOME=/home/penumbra"
                # Set SSL cert vars so that `curl` can fetch over HTTPS.
                "SSL_CERT_FILE=/etc/ssl/certs/ca-bundle.crt"
                "NIX_SSL_CERT_FILE=/etc/ssl/certs/ca-bundle.crt"
              ];
            };
          };

        in {
          packages = {
            inherit penumbraReindexer ;
            default = penumbraReindexer;
            container = containerImage;
          };
          apps = {
            penumbra-reindexer.type = "app";
            penumbra-reindexer.program = "${penumbraReindexer}/bin/penumbra-reindexer";
          };
          devShells.default = craneLib.devShell {
            inherit LIBCLANG_PATH ROCKSDB_LIB_DIR;
            inputsFrom = [ penumbraReindexer ];
            packages = [

              # Wrap the `pd` command in a script to avoid a `cannot execute binary` error.
              (pkgs.writeShellScriptBin "pd" ''
                exec ${penumbra-repo.apps.${system}.pd.program} "$@"
              '')

              cargo-nextest
              cargo-release
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
      )
    );
}
