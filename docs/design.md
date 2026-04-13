# Design

`prudence` follows the freedesktop/XDG trash layout:

- `$XDG_DATA_HOME/Trash` or `$HOME/.local/share/Trash` for files on the same filesystem as the home data directory
- `$topdir/.Trash/$uid` or `$topdir/.Trash-$uid` for files on other mounted filesystems
- `.trashinfo` metadata files with `Path=` and `DeletionDate=` entries

Behavior is intentionally conservative:

- it refuses to trash a filesystem root
- it refuses to trash items already inside the selected trash root
- when cross-filesystem rename is not possible, it copies into trash first and only removes the original after the copy succeeds
- the cross-filesystem copy path preserves mode bits, ownership when permitted, and timestamps
- `list` reads entries from the home trash and any mounted topdir trash roots that already exist
- `list` prints a stable entry ID derived from the trash record, grouped in a multi-line per-entry view so `restore` does not depend on a changing row number
- `clear` empties the discovered trash roots only when explicitly requested
- `restore` refuses to overwrite an existing destination and requires an exact match when restoring by name
