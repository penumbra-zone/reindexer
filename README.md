# penumbra-reindexer

A utility for reindexing historical [Penumbra] ABCI events.

## Building

This requires a Go compiler to be installed in order to build.
This is because we depend on the [cometbft libraries](https://pkg.go.dev/github.com/cometbft/cometbft)
to read the block store.

Aside from that, this crate employs a build script to link in the necessary Go code
into the resulting binary, so standard Cargo commands will work as normal.

For an integrated build environment, there's a [nix] flake in the repo,
which provides a devshell that includes all necessary dependencies. To use it,
install [nix], then run:

```
nix develop
cargo build --release
```

The tool will take a long time to compile: it needs to build every historical version of Penumbra,
up to the present.

## Why This Exists

When an upgrade happens on [Penumbra], the initial chain gets destroyed, and a new chain is started,
using the state of Penumbra after the last block of the first chain (with migration logic potentially
having been applied), but with the rest of the cometbft data destroyed.

This has two annoying effects:
- the state of the penumbra chain is destroyed,
- the pre-upgrade blocks are forgotten.

This means that:
- syncing requires a snapshot of the pre-upgrade Penumbra state,
- it's no longer possible to generate events for pre-upgrade blocks.

The first issue is not a massive deal, but it can be nice to have a way to re-check
the Penumbra logic against the history, shifting trust from a snapshot of the state to
a snapshot of the blocks.

The second is not an issue if an event database was being maintained pre-upgrade.
However, If one wants to add events to previous blocks, one can no longer
do this pre-upgrade.

On the current chain, you can always re-sync a node with new logic to generate more events.
But, since the old blocks are all gone, this isn't possible.

## What this Does

This tool does two things:
1. it allows creating a compact archive file containing all blocks and genesis files,
2. it allows re-executing versioned Penumbra logic against this archive file, producing state and events.

Point 1. is necessary for point 2.
To re-generate all of the events that have happened on Penumbra, we need to execute the first
genesis file, step through all of the blocks, run the first upgrade migration, execute the second genesis,
step through all the blocks, and so on.
To this end, having a single archive file is useful, since this provides an artifact which can be maintained
and shared without needing to scrub private data, unlike a full node directory.

## Use-Cases

Here are some examples of using the command for particular uses cases.

### Maintaining an Archive File

Let's say you're running a node in the default directory `~/.penumbra/network_data/node0/`.

Then, you reach an upgrade point.
Before upgrading, you want to create an archive file, to save the pre-upgrade genesis and blocks.

You run:
```bash
penumbra-reindexer archive
```
and an archive file will get placed in that directory.

Then you run the migration as usual.

Before the next upgrade, you'll run this command again, etc. etc.

Currently, it's not possible to run this command while the node is executing,
unfortunately, because cometbft will take a lock on its blocks database.
At any point in time you can stop the node, run the archive command,
and then resume the node, if you'd like an in-situ archive.

### Regenerating with new Events

Let's say you have a full archive database, up to say, block `5500123`, post-upgrade,
and would like to recreate an events database.
(Maybe you ran a node up to this height without configuring it to index into Postgres, woops).

Assuming that you have the archive file in the default location, as per the last command,
and you want to index into a database named `penumbra_raw` on a local Postgres instance,

```bash
penumbra-reindexer regen --database-url postgresql://localhost:5432/penumbra_raw?sslmode=disable
```

The reindexer will then read block data from the sqlite3 database (configurable via `--archive-file`),
use the appropriate version of Penumbra dependencies for each block height, and store the resulting
generated ABCI events in the target postgres database.
After running these commands, the raw event database should have all events up to and including height `5500123`.

### Starting a node without a Snapshot

The first command we ran in the previous section:

```bash
penumbra-reindexer regen --database-url postgresql://localhost:5432/penumbra_raw?sslmode=disable --working-dir /tmp/regen --stop-height 501974
```

has the side effect of putting the Penumbra state pre-migration into `/tmp/regen`.
If we then do the process of creating a node, but then replace its rocksdb folder with this folder,
replacing its state, we can then migrate and sync our node, as if we had started from a pre-migration state snapshot.

## Full Usage Information

```
This is a utility around re-indexing historical Penumbra events

Usage: penumbra-reindexer <COMMAND>

Commands:
  archive  Create or add to our full historical archive
  regen    Regenerate an index of events, given a historical archive
  help     Print this message or the help of the given subcommand(s)

Options:
  -h, --help     Print help
  -V, --version  Print version
```

### Archiving

```
Create or add to our full historical archive

Usage: penumbra-reindexer archive [OPTIONS]

Options:
      --node-home <NODE_HOME>
          The directory containing pd and cometbft data for a full node.

          In this directory we expect there to be:

          - ./cometbft/config/config.toml, for reading cometbft configuration - ./cometbft/data/, for reading historical blocks

          Defaults to `~/.penumbra/network_data/node0`, the same default used for `pd start`.

          The node state will be read from this directory, and saved inside an sqlite3 database at ~/.local/share/penumbra-reindexer/<CHAIN_ID>/reindexer-archive.sqlite.

          Read usage can be overridden with --cometbft-dir. Write usage can be overridden with --archive-file.

      --home <HOME>
          The home directory for the penumbra-reindexer.

          Downloaded large files will be stored within this directory.

          Defaults to `~/.local/share/penumbra-reindexer`. Can be overridden with --archive-file.

      --cometbft-dir <COMETBFT_DIR>
          Override the path where CometBFT configuration is stored. Defaults to <HOME>/cometbft/

      --archive-file <ARCHIVE_FILE>
          Override the filepath for the sqlite3 database. Defaults to <HOME>/reindexer_archive.bin

      --remote-rpc <REMOTE_RPC>
          Use a remote CometBFT RPC URL to fetch block and genesis data.

          Setting this option will remove the need for on-disk cometbft data for the reindexer to read from. The reindexer must still write to a local sqlite3 database to store the results.

      --chain-id <CHAIN_ID>
          Set a specific chain id

  -h, --help
          Print help (see a summary with '-h')
```

### Regeneration

```
Regenerate an index of events, given a historical archive

Usage: penumbra-reindexer regen [OPTIONS] --database-url <DATABASE_URL>

Options:
      --database-url <DATABASE_URL>
          The URL for the database where we should store the produced events

      --home <HOME>
          The home directory for the penumbra-reindexer.

          Downloaded large files will be stored within this directory.

          Defaults to `~/.local/share/penumbra-reindexer`. Can be overridden with --archive-file.

      --archive-file <ARCHIVE_FILE>
          Override the location of the sqlite3 database from which event data will be read. Defaults to `<HOME>/reindexer_archive.bin`

      --working-dir <WORKING_DIR>
          If set, use a given directory to store the working reindexing state.

          This allows resumption of reindexing, by reusing the directory.

      --allow-existing-data
          If set, allows the indexing database to have data.

          This will make the indexer add any data that's not there (e.g. blocks that are missing, etc.). The indexer will not overwrite existing data, and simply skip indexing anything that
would do so.

      --chain-id <CHAIN_ID>
          Specify a network for which events should be regenerated.

          The sqlite3 database must already have events in it from this chain. If the chain id in the sqlite3 database doesn't match this value, the program will exit with an error.

      --clean
          If set, remove the working directory before starting regeneration.

          This ensures a clean state for regeneration but will remove any existing regeneration progress.

  -h, --help
          Print help (see a summary with '-h')
```

[Penumbra]: https://github.com/penumbra-zone/penumbra
[nix]: https://nixos.org/download/
