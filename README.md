# penumbra-reindexer

A utility around reindexing historical Penumbra events.

## Building

This requires a Go compiler to be installed in order to build.
This is because we depend on the [cometbft libraries](https://pkg.go.dev/github.com/cometbft/cometbft)
to read the block store.

Aside from that, this crate employs a build script to link in the necessary Go code
into the resulting binary, so standard Cargo commands will work as normal.
