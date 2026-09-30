# Install

## Quick Install (via Makefile)

System-wide installation (installs binary, `trash` alias, man page, and Bash/Zsh/Fish completions):

```bash
sudo make install
```

User-only installation (installs into `~/.local`, no `sudo` required):

```bash
make install-user
```

To uninstall:

```bash
sudo make uninstall
```

## Manual Install

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

Install shell completions (optional):

```bash
# Bash
sudo install -Dm644 completions/prudence.bash /usr/share/bash-completion/completions/prudence
# Zsh
sudo install -Dm644 completions/_prudence /usr/share/zsh/site-functions/_prudence
# Fish
sudo install -Dm644 completions/prudence.fish /usr/share/fish/vendor_completions.d/prudence.fish
```

After that, `man prudence` and shell completions will work immediately in new shell sessions.
