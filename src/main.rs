use std::collections::BTreeSet;
use std::env;
use std::ffi::{OsStr, OsString, c_char, c_int, c_long};
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};
use std::path::{Component, Path, PathBuf};
use std::process;
use std::time::{SystemTime, UNIX_EPOCH};

const AT_FDCWD: c_int = -100;
const AT_SYMLINK_NOFOLLOW: c_int = 0x100;
const ENOSYS: i32 = 38;
const EXDEV: i32 = 18;
const EOPNOTSUPP: i32 = 95;
const EPERM: i32 = 1;
const HELP: &str = "\
prudence 0.0.1

Move files and directories into the freedesktop/XDG trash instead of deleting them permanently.

Usage:
  prudence [--] <path>...
  prudence list
  prudence clear
  prudence restore <entry-id|entry-name>...

Examples:
  prudence notes.txt old-dir
  prudence -- -file-starting-with-dash
  prudence list
  prudence clear
  prudence restore home:notes.txt
  prudence restore notes.txt
";

enum Command {
    NoOp,
    Trash(Vec<PathBuf>),
    List,
    Clear,
    Restore(Vec<OsString>),
}

struct TrashPlan {
    trash_root: PathBuf,
    relative_base: PathBuf,
}

#[derive(Clone)]
struct TrashEntry {
    id: String,
    entry_name: OsString,
    trash_root: PathBuf,
    info_path: PathBuf,
    file_path: PathBuf,
    original_path: PathBuf,
    deletion_date: Option<String>,
}

struct TrashInfo {
    original_path: PathBuf,
    deletion_date: Option<String>,
}

fn main() {
    match run() {
        Ok(code) => process::exit(code),
        Err(err) => {
            eprintln!("prudence: {err}");
            process::exit(1);
        }
    }
}

fn run() -> Result<i32, String> {
    match parse_args(env::args_os().skip(1))? {
        Command::NoOp => Ok(0),
        Command::Trash(paths) => {
            for path in paths {
                trash_path(&path)?;
            }
            Ok(0)
        }
        Command::List => {
            list_entries()?;
            Ok(0)
        }
        Command::Clear => {
            clear_entries()?;
            Ok(0)
        }
        Command::Restore(selectors) => {
            restore_entries(&selectors)?;
            Ok(0)
        }
    }
}

fn parse_args<I>(args: I) -> Result<Command, String>
where
    I: IntoIterator<Item = OsString>,
{
    let mut args = args.into_iter();
    let Some(first) = args.next() else {
        print!("{HELP}");
        return Ok(Command::NoOp);
    };

    if first.as_encoded_bytes() == b"--" {
        let paths: Vec<PathBuf> = args.map(PathBuf::from).collect();
        if paths.is_empty() {
            return Err("expected at least one path after --".to_string());
        }
        return Ok(Command::Trash(paths));
    }

    match first.as_encoded_bytes() {
        b"-h" | b"--help" => {
            print!("{HELP}");
            return Ok(Command::NoOp);
        }
        b"-V" | b"--version" => {
            println!("{}", env!("CARGO_PKG_VERSION"));
            return Ok(Command::NoOp);
        }
        b"list" => {
            if let Some(extra) = args.next() {
                return Err(format!(
                    "unexpected argument '{}' for list",
                    PathBuf::from(extra).display()
                ));
            }
            return Ok(Command::List);
        }
        b"clear" => {
            if let Some(extra) = args.next() {
                return Err(format!(
                    "unexpected argument '{}' for clear",
                    PathBuf::from(extra).display()
                ));
            }
            return Ok(Command::Clear);
        }
        b"restore" => {
            let selectors: Vec<OsString> = args.collect();
            if selectors.is_empty() {
                return Err("restore expects at least one entry id or entry name".to_string());
            }
            return Ok(Command::Restore(selectors));
        }
        _ => {}
    }

    let mut paths = vec![PathBuf::from(first)];
    let mut literal_mode = false;

    for arg in args {
        if !literal_mode {
            match arg.as_encoded_bytes() {
                b"--" => {
                    literal_mode = true;
                    continue;
                }
                bytes if bytes.starts_with(b"-") => {
                    return Err(format!(
                        "unsupported flag '{}'; use -- before paths that start with '-'",
                        PathBuf::from(arg).display()
                    ));
                }
                _ => {}
            }
        }

        paths.push(PathBuf::from(arg));
    }

    Ok(Command::Trash(paths))
}

fn trash_path(input: &Path) -> Result<(), String> {
    let absolute_path = absolute_path(input)?;
    let metadata = fs::symlink_metadata(&absolute_path)
        .map_err(|err| format!("{}: {err}", input.display()))?;

    if absolute_path.parent().is_none() {
        return Err(format!(
            "{}: refusing to trash a filesystem root",
            absolute_path.display()
        ));
    }

    let plan = choose_trash_location(&absolute_path)?;

    if absolute_path.starts_with(&plan.trash_root) {
        return Err(format!(
            "{}: refusing to trash an item that is already inside {}",
            absolute_path.display(),
            plan.trash_root.display()
        ));
    }

    ensure_trash_layout(&plan.trash_root)?;

    let original_name = absolute_path
        .file_name()
        .ok_or_else(|| format!("{}: no final path component", absolute_path.display()))?;
    let trash_info_path = encoded_trashinfo_path(&absolute_path, &plan)?;
    let deletion_date = deletion_timestamp()?;
    let info_contents = format!(
        "[Trash Info]\nPath={}\nDeletionDate={}\n",
        trash_info_path, deletion_date
    );

    for attempt in 0.. {
        let candidate = candidate_name(original_name, attempt);
        let info_path = plan
            .trash_root
            .join("info")
            .join(append_suffix(&candidate, b".trashinfo"));
        let file_path = plan.trash_root.join("files").join(&candidate);

        match reserve_info_file(&info_path, &info_contents) {
            Ok(()) => {}
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(err) => {
                return Err(format!("failed to create {}: {err}", info_path.display()));
            }
        }

        if file_path.exists() {
            let _ = fs::remove_file(&info_path);
            continue;
        }

        if let Err(err) = move_path(&absolute_path, &file_path, &metadata) {
            let _ = fs::remove_file(&info_path);
            let _ = remove_path_if_exists(&file_path);
            return Err(format!("{}: {err}", absolute_path.display()));
        }

        return Ok(());
    }

    Err(format!(
        "{}: could not allocate a unique trash entry",
        absolute_path.display()
    ))
}

fn list_entries() -> Result<(), String> {
    let entries = collect_trash_entries()?;

    if entries.is_empty() {
        println!("trash is empty");
        return Ok(());
    }

    println!(
        "{} {} in trash",
        entries.len(),
        if entries.len() == 1 {
            "entry"
        } else {
            "entries"
        }
    );

    for (index, entry) in entries.iter().enumerate() {
        if index != 0 {
            println!();
        }
        print_list_entry(entry);
    }

    Ok(())
}

fn print_list_entry(entry: &TrashEntry) {
    println!("[{}]", entry.id);
    println!("  name:    {}", entry.entry_name.to_string_lossy());
    println!(
        "  deleted: {}",
        entry.deletion_date.as_deref().unwrap_or("-")
    );
    println!("  from:    {}", entry.original_path.display());
}

fn clear_entries() -> Result<(), String> {
    let mut removed_any = false;

    for trash_root in discover_trash_roots()? {
        removed_any |= clear_directory_contents(&trash_root.join("files"))?;
        removed_any |= clear_directory_contents(&trash_root.join("info"))?;
    }

    if removed_any {
        println!("cleared trash");
    } else {
        println!("trash is already empty");
    }

    Ok(())
}

fn clear_directory_contents(path: &Path) -> Result<bool, String> {
    let read_dir = match fs::read_dir(path) {
        Ok(read_dir) => read_dir,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(err) => return Err(format!("failed to read {}: {err}", path.display())),
    };

    let mut removed_any = false;
    for entry in read_dir {
        let entry = entry.map_err(|err| format!("failed to read {}: {err}", path.display()))?;
        remove_path_if_exists(&entry.path())?;
        removed_any = true;
    }

    Ok(removed_any)
}

fn restore_entries(selectors: &[OsString]) -> Result<(), String> {
    let entries = collect_trash_entries()?;

    if entries.is_empty() {
        return Err("trash is empty".to_string());
    }

    let selected = resolve_entries(&entries, selectors)?;

    for entry in selected {
        restore_entry(&entry)?;
    }

    Ok(())
}

fn resolve_entries(
    entries: &[TrashEntry],
    selectors: &[OsString],
) -> Result<Vec<TrashEntry>, String> {
    let mut selected = Vec::new();
    let mut seen = BTreeSet::new();

    for selector in selectors {
        let key = selector.as_encoded_bytes().to_vec();
        if !seen.insert(key) {
            return Err(format!(
                "duplicate selector '{}'",
                PathBuf::from(selector).display()
            ));
        }

        let selector_bytes = selector.as_encoded_bytes();
        if let Some(entry) = entries
            .iter()
            .find(|entry| entry.id.as_bytes() == selector_bytes)
        {
            selected.push(entry.clone());
            continue;
        }

        let matches: Vec<&TrashEntry> = entries
            .iter()
            .filter(|entry| entry.entry_name.as_encoded_bytes() == selector.as_encoded_bytes())
            .collect();

        match matches.as_slice() {
            [] => {
                return Err(format!(
                    "no trash entry named '{}'",
                    PathBuf::from(selector).display()
                ));
            }
            [entry] => selected.push((*entry).clone()),
            many => {
                let ids = many
                    .iter()
                    .map(|entry| entry.id.to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                return Err(format!(
                    "entry name '{}' is ambiguous; use one of these ids: {ids}",
                    PathBuf::from(selector).display()
                ));
            }
        }
    }

    Ok(selected)
}

fn restore_entry(entry: &TrashEntry) -> Result<(), String> {
    let metadata = fs::symlink_metadata(&entry.file_path).map_err(|err| {
        format!(
            "missing trashed payload for entry {} ({}): {err}",
            entry.id,
            entry.entry_name.to_string_lossy()
        )
    })?;

    if entry.original_path.exists() {
        return Err(format!(
            "refusing to restore {} because {} already exists",
            entry.entry_name.to_string_lossy(),
            entry.original_path.display()
        ));
    }

    let parent = entry.original_path.parent().ok_or_else(|| {
        format!(
            "cannot restore {} without a parent directory",
            entry.original_path.display()
        )
    })?;
    fs::create_dir_all(parent)
        .map_err(|err| format!("failed to create {}: {err}", parent.display()))?;

    move_path(&entry.file_path, &entry.original_path, &metadata)?;
    fs::remove_file(&entry.info_path).map_err(|err| {
        format!(
            "restored {} but could not remove {}: {err}",
            entry.entry_name.to_string_lossy(),
            entry.info_path.display()
        )
    })?;

    Ok(())
}

fn collect_trash_entries() -> Result<Vec<TrashEntry>, String> {
    let mut entries = Vec::new();

    for trash_root in discover_trash_roots()? {
        let info_dir = trash_root.join("info");
        let files_dir = trash_root.join("files");

        let read_dir = match fs::read_dir(&info_dir) {
            Ok(read_dir) => read_dir,
            Err(err) if err.kind() == io::ErrorKind::NotFound => continue,
            Err(err) => return Err(format!("failed to read {}: {err}", info_dir.display())),
        };

        for item in read_dir {
            let item =
                item.map_err(|err| format!("failed to read {}: {err}", info_dir.display()))?;
            let path = item.path();
            if path.extension() != Some(OsStr::new("trashinfo")) {
                continue;
            }

            let entry_name = path
                .file_stem()
                .ok_or_else(|| format!("invalid trash info filename {}", path.display()))?
                .to_os_string();
            let file_path = files_dir.join(&entry_name);
            if !file_path.exists() {
                continue;
            }

            let info = match parse_trashinfo(&path, &trash_root) {
                Ok(info) => info,
                Err(err) => {
                    eprintln!("prudence: warning: {err}");
                    continue;
                }
            };
            entries.push(TrashEntry {
                id: entry_id_for(&trash_root, &entry_name),
                entry_name,
                trash_root: trash_root.clone(),
                info_path: path,
                file_path,
                original_path: info.original_path,
                deletion_date: info.deletion_date,
            });
        }
    }

    entries.sort_by(|left, right| {
        left.deletion_date
            .cmp(&right.deletion_date)
            .reverse()
            .then_with(|| {
                left.entry_name
                    .as_encoded_bytes()
                    .cmp(right.entry_name.as_encoded_bytes())
            })
            .then_with(|| left.trash_root.cmp(&right.trash_root))
    });

    Ok(entries)
}

fn discover_trash_roots() -> Result<Vec<PathBuf>, String> {
    let mut roots = BTreeSet::new();
    let xdg_data_home = xdg_data_home()?;
    roots.insert(xdg_data_home.join("Trash"));

    let uid = uid_string();
    for mountpoint in mounted_topdirs()? {
        if let Some(admin_root) = admin_trash_root_if_present(&mountpoint, &uid) {
            roots.insert(admin_root);
        }

        let fallback = mountpoint.join(format!(".Trash-{uid}"));
        if fallback.exists() {
            roots.insert(fallback);
        }
    }

    Ok(roots.into_iter().collect())
}

fn mounted_topdirs() -> Result<Vec<PathBuf>, String> {
    let contents = fs::read_to_string("/proc/self/mountinfo")
        .map_err(|err| format!("failed to read /proc/self/mountinfo: {err}"))?;
    let mut mountpoints = BTreeSet::new();

    for line in contents.lines() {
        let Some(before_dash) = line.split(" - ").next() else {
            continue;
        };
        let fields: Vec<&str> = before_dash.split_whitespace().collect();
        if fields.len() < 5 {
            continue;
        }

        let mountpoint = decode_mountinfo_path(fields[4].as_bytes())?;
        mountpoints.insert(PathBuf::from(mountpoint));
    }

    Ok(mountpoints.into_iter().collect())
}

fn admin_trash_root_if_present(topdir: &Path, uid: &str) -> Option<PathBuf> {
    let admin_trash = topdir.join(".Trash");
    let metadata = fs::symlink_metadata(&admin_trash).ok()?;
    let is_directory = metadata.is_dir();
    let is_sticky = metadata.permissions().mode() & 0o1000 != 0;
    let is_symlink = metadata.file_type().is_symlink();

    if is_directory && is_sticky && !is_symlink {
        let user_trash = admin_trash.join(uid);
        if user_trash.exists() {
            return Some(user_trash);
        }
    }

    None
}

fn parse_trashinfo(path: &Path, trash_root: &Path) -> Result<TrashInfo, String> {
    let content = fs::read_to_string(path)
        .map_err(|err| format!("failed to read {}: {err}", path.display()))?;
    let mut has_header = false;
    let mut encoded_path = None;
    let mut deletion_date = None;

    for line in content.lines() {
        let line = line.trim_end_matches('\r');
        if line == "[Trash Info]" {
            has_header = true;
            continue;
        }
        if let Some(value) = line.strip_prefix("Path=") {
            encoded_path = Some(value.to_string());
            continue;
        }
        if let Some(value) = line.strip_prefix("DeletionDate=") {
            deletion_date = Some(value.to_string());
        }
    }

    if !has_header {
        return Err(format!("{} is missing [Trash Info]", path.display()));
    }

    let encoded_path =
        encoded_path.ok_or_else(|| format!("{} is missing Path=", path.display()))?;
    let decoded = PathBuf::from(percent_decode(encoded_path.as_bytes())?);
    let original_path = if decoded.is_absolute() {
        decoded
    } else {
        let base = trash_relative_base(trash_root)
            .ok_or_else(|| format!("cannot resolve {}", path.display()))?;
        if decoded
            .components()
            .any(|component| matches!(component, Component::ParentDir))
        {
            return Err(format!(
                "{} contains an unsafe relative path {}",
                path.display(),
                decoded.display()
            ));
        }
        base.join(decoded)
    };

    Ok(TrashInfo {
        original_path,
        deletion_date,
    })
}

fn trash_relative_base(trash_root: &Path) -> Option<PathBuf> {
    let xdg_home_trash = xdg_data_home().ok()?.join("Trash");
    if trash_root == xdg_home_trash {
        return xdg_data_home().ok();
    }

    let parent = trash_root.parent()?;
    if parent.file_name() == Some(OsStr::new(".Trash")) {
        return parent.parent().map(Path::to_path_buf);
    }

    let file_name = trash_root.file_name()?.to_string_lossy();
    if file_name.starts_with(".Trash-") {
        return Some(parent.to_path_buf());
    }

    None
}

fn entry_id_for(trash_root: &Path, entry_name: &OsStr) -> String {
    let scope = xdg_data_home()
        .ok()
        .map(|path| path.join("Trash"))
        .filter(|home_trash| home_trash == trash_root)
        .map(|_| "home".to_string())
        .unwrap_or_else(|| percent_encode(trash_root.as_os_str().as_encoded_bytes()));
    let name = percent_encode(entry_name.as_encoded_bytes());
    format!("{scope}:{name}")
}

fn absolute_path(input: &Path) -> Result<PathBuf, String> {
    let cwd =
        env::current_dir().map_err(|err| format!("failed to read current directory: {err}"))?;
    let joined = if input.is_absolute() {
        input.to_path_buf()
    } else {
        cwd.join(input)
    };

    Ok(normalize_path(&joined))
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();

    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    normalized.push(component.as_os_str());
                }
            }
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::Normal(part) => normalized.push(part),
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
        }
    }

    if normalized.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        normalized
    }
}

fn choose_trash_location(path: &Path) -> Result<TrashPlan, String> {
    let xdg_data_home = xdg_data_home()?;
    let home_mount = mount_root_for_path(&xdg_data_home)?;
    let target_mount = mount_root_for_path(path)?;
    let home_plan = TrashPlan {
        trash_root: xdg_data_home.join("Trash"),
        relative_base: xdg_data_home,
    };

    if target_mount == home_mount {
        return Ok(home_plan);
    }

    let uid = uid_string();
    if let Some(trash_root) = topdir_trash_root(&target_mount, &uid) {
        if ensure_trash_layout(&trash_root).is_ok() {
            return Ok(TrashPlan {
                relative_base: target_mount,
                trash_root,
            });
        }
    }

    Ok(home_plan)
}

fn xdg_data_home() -> Result<PathBuf, String> {
    if let Some(path) = env::var_os("XDG_DATA_HOME") {
        let path = PathBuf::from(path);
        if path.is_absolute() {
            return Ok(path);
        }
    }

    let home = env::var_os("HOME").ok_or_else(|| "HOME is not set".to_string())?;
    Ok(PathBuf::from(home).join(".local/share"))
}

fn mount_root_for_path(path: &Path) -> Result<PathBuf, String> {
    let start = existing_ancestor(path)
        .ok_or_else(|| format!("could not find an existing ancestor for {}", path.display()))?;
    let device = fs::symlink_metadata(&start)
        .map_err(|err| format!("failed to stat {}: {err}", start.display()))?
        .dev();

    let mut current = start;
    loop {
        let Some(parent) = current.parent() else {
            return Ok(current);
        };

        let parent_metadata = fs::symlink_metadata(parent)
            .map_err(|err| format!("failed to stat {}: {err}", parent.display()))?;

        if parent_metadata.dev() != device {
            return Ok(current);
        }

        current = parent.to_path_buf();
    }
}

fn existing_ancestor(path: &Path) -> Option<PathBuf> {
    let mut current = path.to_path_buf();

    loop {
        if current.exists() {
            return Some(current);
        }

        current = current.parent()?.to_path_buf();
    }
}

fn topdir_trash_root(topdir: &Path, uid: &str) -> Option<PathBuf> {
    let admin_trash = topdir.join(".Trash");

    if let Ok(metadata) = fs::symlink_metadata(&admin_trash) {
        let is_directory = metadata.is_dir();
        let is_sticky = metadata.permissions().mode() & 0o1000 != 0;
        let is_symlink = metadata.file_type().is_symlink();

        if is_directory && is_sticky && !is_symlink {
            return Some(admin_trash.join(uid));
        }
    }

    Some(topdir.join(format!(".Trash-{uid}")))
}

fn ensure_trash_layout(trash_root: &Path) -> Result<(), String> {
    fs::create_dir_all(trash_root.join("files")).map_err(|err| {
        format!(
            "failed to create {}: {err}",
            trash_root.join("files").display()
        )
    })?;
    fs::create_dir_all(trash_root.join("info")).map_err(|err| {
        format!(
            "failed to create {}: {err}",
            trash_root.join("info").display()
        )
    })?;
    Ok(())
}

fn encoded_trashinfo_path(path: &Path, plan: &TrashPlan) -> Result<String, String> {
    let stored_path = match path.strip_prefix(&plan.relative_base) {
        Ok(relative) if !relative.as_os_str().is_empty() => relative.to_path_buf(),
        _ => path.to_path_buf(),
    };

    if stored_path.as_os_str().is_empty() {
        return Err(format!("{}: empty stored path", path.display()));
    }

    Ok(percent_encode(stored_path.as_os_str().as_encoded_bytes()))
}

fn percent_encode(bytes: &[u8]) -> String {
    let mut encoded = String::with_capacity(bytes.len());

    for &byte in bytes {
        let safe = byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'-' | b'_' | b'.' | b'~');

        if safe {
            encoded.push(byte as char);
        } else {
            encoded.push('%');
            encoded.push(hex(byte >> 4));
            encoded.push(hex(byte & 0x0f));
        }
    }

    encoded
}

fn percent_decode(bytes: &[u8]) -> Result<OsString, String> {
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;

    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len() {
                return Err("truncated percent-encoding".to_string());
            }
            let high = from_hex(bytes[index + 1])?;
            let low = from_hex(bytes[index + 2])?;
            decoded.push((high << 4) | low);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }

    Ok(OsString::from_vec(decoded))
}

fn decode_mountinfo_path(bytes: &[u8]) -> Result<OsString, String> {
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;

    while index < bytes.len() {
        if bytes[index] == b'\\' {
            if index + 3 >= bytes.len() {
                return Err("truncated mountinfo escape".to_string());
            }
            let d1 = bytes[index + 1];
            let d2 = bytes[index + 2];
            let d3 = bytes[index + 3];
            if !d1.is_ascii_digit() || !d2.is_ascii_digit() || !d3.is_ascii_digit() {
                return Err("invalid mountinfo escape".to_string());
            }
            let value = (d1 - b'0') * 64 + (d2 - b'0') * 8 + (d3 - b'0');
            decoded.push(value);
            index += 4;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }

    Ok(OsString::from_vec(decoded))
}

fn from_hex(byte: u8) -> Result<u8, String> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err(format!("invalid hex digit '{}'", byte as char)),
    }
}

fn hex(value: u8) -> char {
    match value {
        0..=9 => (b'0' + value) as char,
        10..=15 => (b'A' + (value - 10)) as char,
        _ => unreachable!(),
    }
}

fn deletion_timestamp() -> Result<String, String> {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|err| format!("system clock is before the Unix epoch: {err}"))?
        .as_secs() as i64;

    let mut tm = Tm::default();
    let converted = unsafe { localtime_r(&seconds, &mut tm) };

    if converted.is_null() {
        return Err("failed to convert the current time to local time".to_string());
    }

    Ok(format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
        tm.tm_year + 1900,
        tm.tm_mon + 1,
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min,
        tm.tm_sec
    ))
}

fn reserve_info_file(path: &Path, contents: &str) -> io::Result<()> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(contents.as_bytes())?;
    file.flush()?;
    Ok(())
}

fn candidate_name(original_name: &OsStr, attempt: usize) -> OsString {
    if attempt == 0 {
        return original_name.to_os_string();
    }

    let mut bytes = original_name.as_encoded_bytes().to_vec();
    bytes.extend_from_slice(format!(".{attempt}").as_bytes());
    OsString::from_vec(bytes)
}

fn append_suffix(name: &OsStr, suffix: &[u8]) -> OsString {
    let mut bytes = name.as_encoded_bytes().to_vec();
    bytes.extend_from_slice(suffix);
    OsString::from_vec(bytes)
}

fn move_path(source: &Path, destination: &Path, metadata: &fs::Metadata) -> Result<(), String> {
    match fs::rename(source, destination) {
        Ok(()) => Ok(()),
        Err(err) if err.raw_os_error() == Some(EXDEV) => {
            copy_path(source, destination, metadata)?;
            if let Err(remove_err) = remove_original(source, metadata) {
                let _ = remove_path_if_exists(destination);
                return Err(format!(
                    "copied into {} but could not remove the original: {remove_err}",
                    destination.display()
                ));
            }
            Ok(())
        }
        Err(err) => Err(format!(
            "failed to move into {}: {err}",
            destination.display()
        )),
    }
}

fn copy_path(source: &Path, destination: &Path, metadata: &fs::Metadata) -> Result<(), String> {
    let file_type = metadata.file_type();

    if file_type.is_symlink() {
        let target = fs::read_link(source)
            .map_err(|err| format!("failed to read {}: {err}", source.display()))?;
        symlink(&target, destination)
            .map_err(|err| format!("failed to create {}: {err}", destination.display()))?;
        preserve_symlink_metadata(destination, metadata)?;
        return Ok(());
    }

    if metadata.is_file() {
        fs::copy(source, destination)
            .map_err(|err| format!("failed to copy into {}: {err}", destination.display()))?;
        preserve_regular_metadata(destination, metadata)?;
        return Ok(());
    }

    if metadata.is_dir() {
        fs::create_dir(destination)
            .map_err(|err| format!("failed to create {}: {err}", destination.display()))?;

        for entry in fs::read_dir(source)
            .map_err(|err| format!("failed to read {}: {err}", source.display()))?
        {
            let entry =
                entry.map_err(|err| format!("failed to read {}: {err}", source.display()))?;
            let entry_path = entry.path();
            let entry_destination = destination.join(entry.file_name());
            let entry_metadata = fs::symlink_metadata(&entry_path)
                .map_err(|err| format!("failed to stat {}: {err}", entry_path.display()))?;
            copy_path(&entry_path, &entry_destination, &entry_metadata)?;
        }

        preserve_regular_metadata(destination, metadata)?;
        return Ok(());
    }

    Err(format!(
        "unsupported file type at {}; refusing to emulate a move",
        source.display()
    ))
}

fn remove_original(path: &Path, metadata: &fs::Metadata) -> Result<(), String> {
    if metadata.file_type().is_symlink() || metadata.is_file() {
        fs::remove_file(path).map_err(|err| format!("failed to remove {}: {err}", path.display()))
    } else if metadata.is_dir() {
        remove_dir_contents(path)?;
        fs::remove_dir(path).map_err(|err| format!("failed to remove {}: {err}", path.display()))
    } else {
        Err(format!("unsupported file type at {}", path.display()))
    }
}

fn remove_dir_contents(path: &Path) -> Result<(), String> {
    for entry in
        fs::read_dir(path).map_err(|err| format!("failed to read {}: {err}", path.display()))?
    {
        let entry = entry.map_err(|err| format!("failed to read {}: {err}", path.display()))?;
        let entry_path = entry.path();
        let metadata = fs::symlink_metadata(&entry_path)
            .map_err(|err| format!("failed to stat {}: {err}", entry_path.display()))?;
        remove_original(&entry_path, &metadata)?;
    }

    Ok(())
}

fn remove_path_if_exists(path: &Path) -> Result<(), String> {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return Ok(());
    };

    if metadata.file_type().is_symlink() || metadata.is_file() {
        fs::remove_file(path).map_err(|err| format!("failed to remove {}: {err}", path.display()))
    } else if metadata.is_dir() {
        fs::remove_dir_all(path)
            .map_err(|err| format!("failed to remove {}: {err}", path.display()))
    } else {
        Ok(())
    }
}

fn preserve_regular_metadata(path: &Path, metadata: &fs::Metadata) -> Result<(), String> {
    preserve_ownership(path, metadata, true)?;
    fs::set_permissions(path, metadata.permissions())
        .map_err(|err| format!("failed to apply permissions to {}: {err}", path.display()))?;
    preserve_times(path, metadata, true)
}

fn preserve_symlink_metadata(path: &Path, metadata: &fs::Metadata) -> Result<(), String> {
    preserve_ownership(path, metadata, false)?;
    preserve_times(path, metadata, false)
}

fn preserve_ownership(
    path: &Path,
    metadata: &fs::Metadata,
    follow_symlink: bool,
) -> Result<(), String> {
    let c_path = path_to_c_string(path)?;
    let flags = if follow_symlink {
        0
    } else {
        AT_SYMLINK_NOFOLLOW
    };
    let result = unsafe {
        fchownat(
            AT_FDCWD,
            c_path.as_ptr().cast(),
            metadata.uid(),
            metadata.gid(),
            flags,
        )
    };

    if result == 0 {
        return Ok(());
    }

    let err = io::Error::last_os_error();
    match err.raw_os_error() {
        Some(EPERM) => Ok(()),
        _ => Err(format!(
            "failed to preserve ownership on {}: {err}",
            path.display()
        )),
    }
}

fn preserve_times(
    path: &Path,
    metadata: &fs::Metadata,
    follow_symlink: bool,
) -> Result<(), String> {
    let c_path = path_to_c_string(path)?;
    let flags = if follow_symlink {
        0
    } else {
        AT_SYMLINK_NOFOLLOW
    };
    let times = [
        Timespec {
            tv_sec: metadata.atime(),
            tv_nsec: metadata.atime_nsec() as c_long,
        },
        Timespec {
            tv_sec: metadata.mtime(),
            tv_nsec: metadata.mtime_nsec() as c_long,
        },
    ];

    let result = unsafe { utimensat(AT_FDCWD, c_path.as_ptr().cast(), times.as_ptr(), flags) };
    if result == 0 {
        return Ok(());
    }

    let err = io::Error::last_os_error();
    if !follow_symlink
        && matches!(
            err.raw_os_error(),
            Some(EPERM) | Some(ENOSYS) | Some(EOPNOTSUPP)
        )
    {
        Ok(())
    } else {
        Err(format!(
            "failed to preserve timestamps on {}: {err}",
            path.display()
        ))
    }
}

fn path_to_c_string(path: &Path) -> Result<Vec<u8>, String> {
    let bytes = path.as_os_str().as_bytes();
    if bytes.contains(&0) {
        return Err(format!("{} contains an embedded NUL byte", path.display()));
    }

    let mut c_string = Vec::with_capacity(bytes.len() + 1);
    c_string.extend_from_slice(bytes);
    c_string.push(0);
    Ok(c_string)
}

fn uid_string() -> String {
    unsafe { getuid() }.to_string()
}

#[repr(C)]
struct Timespec {
    tv_sec: i64,
    tv_nsec: c_long,
}

#[repr(C)]
#[derive(Default)]
struct Tm {
    tm_sec: i32,
    tm_min: i32,
    tm_hour: i32,
    tm_mday: i32,
    tm_mon: i32,
    tm_year: i32,
    tm_wday: i32,
    tm_yday: i32,
    tm_isdst: i32,
    tm_gmtoff: i64,
    tm_zone: *const c_char,
}

unsafe extern "C" {
    fn fchownat(dirfd: c_int, path: *const c_char, owner: u32, group: u32, flags: c_int) -> c_int;
    fn getuid() -> u32;
    fn localtime_r(timep: *const i64, result: *mut Tm) -> *mut Tm;
    fn utimensat(dirfd: c_int, path: *const c_char, times: *const Timespec, flags: c_int) -> c_int;
}

#[cfg(test)]
mod tests {
    use super::{
        Command, append_suffix, candidate_name, decode_mountinfo_path, entry_id_for, move_path,
        parse_args, percent_decode, percent_encode,
    };
    use std::ffi::{OsStr, OsString};
    use std::fs;
    use std::os::unix::ffi::{OsStrExt, OsStringExt};
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn percent_encodes_spaces_and_utf8() {
        let encoded = percent_encode("a b/中".as_bytes());
        assert_eq!(encoded, "a%20b/%E4%B8%AD");
    }

    #[test]
    fn percent_decodes_spaces_and_utf8() {
        let decoded = percent_decode(b"a%20b/%E4%B8%AD").unwrap();
        assert_eq!(decoded, OsString::from_vec("a b/中".as_bytes().to_vec()));
    }

    #[test]
    fn keeps_first_candidate_unchanged() {
        let name = candidate_name(OsStr::new("demo.txt"), 0);
        assert_eq!(name.as_bytes(), b"demo.txt");
    }

    #[test]
    fn appends_numeric_suffixes() {
        let name = candidate_name(OsStr::new("demo.txt"), 3);
        assert_eq!(name.as_bytes(), b"demo.txt.3");
    }

    #[test]
    fn appends_trashinfo_suffix() {
        let name = append_suffix(OsStr::new("demo.txt"), b".trashinfo");
        assert_eq!(name.as_bytes(), b"demo.txt.trashinfo");
    }

    #[test]
    fn parses_leading_double_dash_for_paths() {
        let command = parse_args(vec![
            OsString::from("--"),
            OsString::from("-demo"),
            OsString::from("x"),
        ])
        .unwrap();
        match command {
            Command::Trash(paths) => {
                assert_eq!(paths, vec![PathBuf::from("-demo"), PathBuf::from("x")]);
            }
            _ => panic!("expected trash command"),
        }
    }

    #[test]
    fn parses_clear_command() {
        let command = parse_args(vec![OsString::from("clear")]).unwrap();
        match command {
            Command::Clear => {}
            _ => panic!("expected clear command"),
        }
    }

    #[test]
    fn decodes_mountinfo_escapes() {
        let decoded = decode_mountinfo_path(br"/tmp/with\040space").unwrap();
        assert_eq!(decoded, OsString::from("/tmp/with space"));
    }

    #[test]
    fn entry_ids_are_stable_for_non_home_trash() {
        let id = entry_id_for(Path::new("/tmp/.Trash-1000"), OsStr::new("demo.txt"));
        assert_eq!(id, "/tmp/.Trash-1000:demo.txt");
    }

    #[test]
    fn move_path_preserves_mode_and_mtime_across_filesystems() {
        let source_root = unique_temp_dir(&std::env::temp_dir(), "prudence-src");
        let dest_root = unique_temp_dir(
            &std::env::current_dir().unwrap().join("target"),
            "prudence-dst",
        );

        let source_root_meta = fs::symlink_metadata(&source_root).unwrap();
        let dest_root_meta = fs::symlink_metadata(&dest_root).unwrap();
        if source_root_meta.dev() == dest_root_meta.dev() {
            let _ = fs::remove_dir_all(&source_root);
            let _ = fs::remove_dir_all(&dest_root);
            return;
        }

        let source = source_root.join("sample.txt");
        let dest = dest_root.join("sample.txt");
        fs::write(&source, b"hello").unwrap();
        fs::set_permissions(&source, fs::Permissions::from_mode(0o640)).unwrap();
        set_test_mtime(&source, 1_700_000_123, 456_000_000);

        let source_metadata = fs::symlink_metadata(&source).unwrap();
        move_path(&source, &dest, &source_metadata).unwrap();

        assert!(!source.exists());
        let dest_metadata = fs::symlink_metadata(&dest).unwrap();
        assert_eq!(dest_metadata.mode() & 0o777, 0o640);
        assert_eq!(dest_metadata.mtime(), source_metadata.mtime());
        assert_eq!(dest_metadata.mtime_nsec(), source_metadata.mtime_nsec());

        let _ = fs::remove_dir_all(&source_root);
        let _ = fs::remove_dir_all(&dest_root);
    }

    fn unique_temp_dir(base: &Path, prefix: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = base.join(format!("{prefix}-{}-{unique}", std::process::id()));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn set_test_mtime(path: &Path, sec: i64, nsec: i64) {
        let c_path = super::path_to_c_string(path).unwrap();
        let times = [
            super::Timespec {
                tv_sec: sec,
                tv_nsec: 0,
            },
            super::Timespec {
                tv_sec: sec,
                tv_nsec: nsec as super::c_long,
            },
        ];
        let result =
            unsafe { super::utimensat(super::AT_FDCWD, c_path.as_ptr().cast(), times.as_ptr(), 0) };
        assert_eq!(result, 0, "failed to set test mtime for {}", path.display());
    }
}
