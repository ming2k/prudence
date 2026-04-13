# Install

Build the release binary:

```bash
cargo build --release
```

Install it system-wide and expose it as `trash`:

```bash
sudo install -m 755 target/release/prudence /usr/local/bin/prudence
sudo ln -sfn /usr/local/bin/prudence /usr/local/bin/trash
sudo install -d /usr/local/share/man/man1
sudo install -m 644 man/man1/prudence.1 /usr/local/share/man/man1/prudence.1
```

After that, `man prudence` should work.
