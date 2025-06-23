# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

This is `penumbra-reindexer`, a Rust utility for reindexing historical Penumbra blockchain events. The tool addresses the problem that occurs during Penumbra chain upgrades where historical blocks are destroyed, making it impossible to regenerate events for pre-upgrade blocks.

The reindexer provides two main functions:
1. **Archive Creation**: Creates compact archive files containing all blocks and genesis files
2. **Event Regeneration**: Re-executes versioned Penumbra logic against archive files to produce state and events

## Architecture

### Core Components

- **Command Structure**: Uses `clap` for CLI with subcommands: `archive`, `regen`, `export`, `bootstrap`, `check`
- **Versioned Dependencies**: Maintains multiple versions of Penumbra dependencies (v0.79, v0.80, v0.81, v1.3, v1.4, v2.x) to handle different upgrade epochs
- **CometBFT Integration**: Includes Go code via build script to read CometBFT block stores
- **Database Backend**: Uses PostgreSQL for event indexing with custom schema

### Key Modules

- `src/command/`: Individual command implementations
- `src/penumbra/`: Version-specific Penumbra logic modules (v0o79.rs, v0o80.rs, etc.)
- `src/history/`: Archive handling and reindexer logic
- `src/indexer/`: PostgreSQL event indexing
- `src/cometbft/`: CometBFT block store integration

### Build Requirements

- **Go Compiler**: Required for CometBFT library integration
- **Rust**: Standard Cargo toolchain
- **Nix**: Optional for containerized builds

## Development Commands

### Build and Check
```bash
# Build project
cargo build

# Run all checks (check, clippy, fmt)
just check

# Format code
just fmt
```

### Testing
```bash
# Run unit tests
just test

# Run expensive tests (requires integration test data)
just expensive-tests

# Run network integration tests (requires significant disk space/bandwidth)
just integration
```

### Nix Commands
```bash
# Build via Nix
just build

# Build container image
just container
```

## Database Schema

The indexer creates PostgreSQL tables for:
- `blocks`: Block metadata with height and chain_id
- `tx_results`: Transaction results with hash and execution data
- `events`: ABCI events with type information
- `attributes`: Event key-value attributes
- `debug.app_hash`: Block app hash storage

## Multi-Version Architecture

The project maintains separate dependency namespaces for different Penumbra versions to handle chain upgrades:
- Each version has its own set of dependencies with version-specific naming
- Migration logic exists between versions to handle state transitions
- The reindexer can step through multiple upgrade epochs in sequence

## Working with Archives

Archive files are binary SQLite databases containing compressed block and genesis data. The tool can:
- Create archives from running CometBFT nodes
- Download pre-built archives for known chains (penumbra-1, penumbra-testnet-phobos-2/3)
- Extract and process archive data for regeneration

Default locations:
- Archive file: `~/.penumbra/network_data/node0/reindexer_archive.bin`
- CometBFT data: `~/.penumbra/network_data/node0/cometbft/`
