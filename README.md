# penumbra-reindexer

A utility around reindexing historical Penumbra events.

## Building

This requires a Go compiler to be installed in order to build.
This is because we depend on the [cometbft libraries](https://pkg.go.dev/github.com/cometbft/cometbft)
to read the block store.

Aside from that, this crate employs a build script to link in the necessary Go code
into the resulting binary, so standard Cargo commands will work as normal.

## Why This Exists

When an upgrade happens on Penumbra, the initial chain gets destroyed, and a new chain is started,
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

Let's say you have a full archive database, up to say, block `600123`, post upgrade,
and would like to recreate an events database.
(Maybe you ran a node up to this height without configuring it to index into Postgres, woops).

Assuming that you have the archive file in the default location, as per the last command,
and you want to index into a database named `penumbra_raw` on a local Postgres instance,
and you want to store the state in `/tmp/regen`, you'd do:

```bash
penumbra-reindexer regen --database-url postgresql://localhost:5432/penumbra_raw?sslmode=disable --working-dir /tmp/regen --stop-height 501974
penumbra-reindexer regen --database-url postgresql://localhost:5432/penumbra_raw?sslmode=disable --working-dir /tmp/regen --stop-height 2611799
penumbra-reindexer regen --database-url postgresql://localhost:5432/penumbra_raw?sslmode=disable --working-dir /tmp/regen --stop-height 4378761
penumbra-reindexer regen --database-url postgresql://localhost:5432/penumbra_raw?sslmode=disable --working-dir /tmp/regen
```

Unfortunately, we have to run the logic twice, because the Penumbra crate will kill our process after it halts pre-upgrade,
after block `501974`.
Post-upgrade, we will run until we process the last block in our archive `600123`.
After running these commands, the raw event database should have all events up to and including height `600123`.

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
      --home <HOME>
          A starting point for reading and writing penumbra data.
          
          The equivalent of pd's --network-dir.
          
          Read usage can be overriden with --cometbft-data-dir.
          
          Write usage can be overriden with --archive-file.
          
          In this directory we expect there to be: - ./cometbft/config/config.toml, for reading cometbft configuration, - (maybe) ./reindexer_archive.bin, for existing archive data to append to.
          
          If unset, defaults to ~/.penumbra/network_data/node0.

      --cometbft-dir <COMETBFT_DIR>
          If set, use this directory for cometbft, instead of HOME/cometbft/

      --archive-file <ARCHIVE_FILE>
          If set, use this file for archive data, instead of HOME/reindexer_archive.bin

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
          A home directory to read penumbra data from.
          
          The equivalent of pd's --network-dir.
          
          This will be overriden by --archive-file.
          
          We expect there to be a ./reindexer_archive.bin file in this directory otherwise.

      --archive-file <ARCHIVE_FILE>
          If set, use this file to read the archive file from directory, ignoring other options

      --start-height <START_HEIGHT>
          If set, index events starting from this height

      --stop-height <STOP_HEIGHT>
          If set, index events up to and including this height.
          
          For example, if this is set to 2, only events in blocks 1, 2 will be indexed.

      --working-dir <WORKING_DIR>
          If set, use a given directory to store the working reindexing state.
          
          This allow resumption of reindexing, by reusing the directory.

  -h, --help
          Print help (see a summary with '-h')
```
