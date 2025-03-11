#!/bin/bash
# utility script to run `penumbra-reindexer regen` for the purpose of integration testing.
# eventually this logic should be ported to the rust integration tests
set -euo pipefail




db_url="postgresql://penumbra:penumbra@127.0.0.1:5432/penumbra?sslmode=disable"
working_dir="${HOME:?}/regen-testnet-1"


# mainnet logic
archive_file="${PWD:?}/test_data/ephemeral-storage/network/penumbra-1/node0/reindexer_archive.bin"
cargo run --release -- regen --database-url "$db_url" --archive-file "$archive_file" --working-dir "$working_dir" --stop-height 501974
cargo run --release -- regen --database-url "$db_url" --archive-file "$archive_file" --working-dir "$working_dir" --stop-height 2611799
cargo run --release -- regen --database-url "$db_url" --archive-file "$archive_file" --working-dir "$working_dir"

# exit early before running conflicting network logic
exit 0

# testnet logic
archive_file="${PWD:?}/test_data/ephemeral-storage/network/penumbra-testnet-phobos-2/node0/reindexer_archive.bin"
cargo run --release -- regen --database-url "$db_url" --archive-file "$archive_file" --working-dir "$working_dir" --stop-height 1459799
# cargo run --release -- regen --database-url "$db_url" --archive-file "$archive_file" --working-dir "$working_dir" --stop-height 14597990
cargo run --release -- regen --database-url "$db_url" --archive-file "$archive_file" --working-dir "$working_dir"
