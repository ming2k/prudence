# Usage

Build:

```bash
cargo build --release
```

Run:

```bash
prudence file.txt old-dir
prudence -- -starts-with-dash
```

Or run the built binary directly:

```bash
./target/release/prudence file.txt old-dir
```

List current trash entries:

```bash
prudence list
```

Clear the current trash contents permanently:

```bash
prudence clear
```

Restore by exact trashed name, or by the stable entry ID printed by `prudence list` when a name is ambiguous:

```bash
prudence restore notes.txt
prudence restore 'home:notes.txt'
```
