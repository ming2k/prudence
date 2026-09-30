# Changelog

All notable changes to this project will be documented in this file.

## 0.0.2 - 2026-09-30

### Added

- Shell completions for Bash, Zsh, and Fish under `completions/`
- Makefile with build, test, check, install, user-install, and uninstall targets
- `trash` alias symlink creation during make installation
- `clean` and `empty` aliases for the `clear` command
- Progress message when restoring entries
- `PRUDENCE_HOME_ONLY` environment variable support for test and single-root environments

### Fixed

- Physical path resolution when trashing items through symlinked parent directories
- Filesystem root and mount point detection improvements

## 0.0.1 - 2026-04-13

Initial public release of `prudence`.

### Added

- Rust CLI for moving files and directories into the freedesktop/XDG trash
- `list` command for enumerating trash entries across home and mounted trash roots
- `restore` command for restoring entries by stable entry ID or exact trashed name
- `clear` command for permanently emptying the current trash roots on demand
- `prudence(1)` manual page under `man/man1/prudence.1`
- project docs under `docs/`
- CLI integration tests for dash-prefixed paths, stable restore IDs, multiline list output, explicit clear, and malformed `.trashinfo` handling

### Interface

- `list` prints readable multi-line entry blocks with stable IDs instead of unstable row numbers
- README, usage docs, and the manual page document the current `trash`, `list`, `restore`, and `clear` workflow

### Safety

- refuses to trash a filesystem root
- refuses to restore over an existing destination
- skips malformed `.trashinfo` entries with a warning instead of breaking `list` or `restore`
- uses stable restore IDs derived from trash records rather than list row order

### Compatibility

- follows the freedesktop/XDG trash directory layout
- preserves mode bits, timestamps, and ownership when permitted during cross-filesystem fallback copies
