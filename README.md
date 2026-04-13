# prudence

`prudence` is a small Rust CLI that moves files and directories into the freedesktop/XDG trash instead of deleting them permanently.

It follows the freedesktop/XDG trash layout and ships a conservative command set:

- `prudence <path>...` moves files or directories into trash
- `prudence list` shows stable entry IDs, names, deletion times, and original paths
- `prudence restore <entry-id|entry-name>...` restores exact matches from trash
- `prudence clear` permanently empties the discovered trash roots on demand

Project docs live in [docs/README.md](/home/ming/projects/prudence/docs/README.md).
The man page source lives at [man/man1/prudence.1](/home/ming/projects/prudence/man/man1/prudence.1).

Quick start:

```bash
cargo build --release
./target/release/prudence file.txt old-dir
./target/release/prudence list
./target/release/prudence restore home:file.txt
./target/release/prudence clear
```

See [docs/usage.md](/home/ming/projects/prudence/docs/usage.md) for command details and [docs/install.md](/home/ming/projects/prudence/docs/install.md) for system installation.
