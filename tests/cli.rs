use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn binary() -> &'static str {
    env!("CARGO_BIN_EXE_prudence")
}

fn unique_temp_dir(prefix: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("{prefix}-{}-{unique}", std::process::id()));
    fs::create_dir_all(&path).unwrap();
    path
}

fn setup_env(test_name: &str) -> (PathBuf, PathBuf) {
    let root = unique_temp_dir(test_name);
    let xdg_data_home = root.join("xdg-data");
    fs::create_dir_all(&xdg_data_home).unwrap();
    (root, xdg_data_home)
}

#[test]
fn double_dash_allows_leading_dash_paths() {
    let (root, xdg_data_home) = setup_env("prudence-cli-double-dash");
    let workdir = root.join("work");
    fs::create_dir_all(&workdir).unwrap();
    fs::write(workdir.join("-demo"), b"x").unwrap();

    let output = Command::new(binary())
        .current_dir(&workdir)
        .env("XDG_DATA_HOME", &xdg_data_home)
        .args(["--", "-demo"])
        .output()
        .unwrap();

    assert!(output.status.success(), "{output:?}");
    assert!(!workdir.join("-demo").exists());
    assert!(xdg_data_home.join("Trash/files/-demo").exists());

    let _ = fs::remove_dir_all(root);
}

#[test]
fn restore_uses_stable_ids_even_after_new_entries_are_added() {
    let (root, xdg_data_home) = setup_env("prudence-cli-stable-id");
    let workdir = root.join("work");
    fs::create_dir_all(&workdir).unwrap();
    let alpha = workdir.join("alpha.txt");
    let beta = workdir.join("beta.txt");
    fs::write(&alpha, b"a").unwrap();
    fs::write(&beta, b"b").unwrap();

    run_ok(&workdir, &xdg_data_home, &[alpha.to_str().unwrap()]);
    let listed = run_ok(&workdir, &xdg_data_home, &["list"]);
    let alpha_id = first_entry_id(&listed);

    run_ok(&workdir, &xdg_data_home, &[beta.to_str().unwrap()]);
    run_ok(&workdir, &xdg_data_home, &["restore", &alpha_id]);

    assert!(alpha.exists());
    assert!(!xdg_data_home.join("Trash/files/alpha.txt").exists());
    assert!(xdg_data_home.join("Trash/files/beta.txt").exists());

    let _ = fs::remove_dir_all(root);
}

#[test]
fn list_uses_multiline_entry_format() {
    let (root, xdg_data_home) = setup_env("prudence-cli-list-format");
    let workdir = root.join("work");
    fs::create_dir_all(&workdir).unwrap();
    let path = workdir.join("notes.txt");
    fs::write(&path, b"x").unwrap();

    run_ok(&workdir, &xdg_data_home, &[path.to_str().unwrap()]);
    let stdout = run_ok(&workdir, &xdg_data_home, &["list"]);

    assert!(stdout.contains("1 entry in trash"));
    assert!(stdout.contains("[home:notes.txt]"));
    assert!(stdout.contains("  name:    notes.txt"));
    assert!(stdout.contains("  deleted: "));
    assert!(stdout.contains(&format!("  from:    {}", path.display())));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn clear_removes_all_entries_from_trash() {
    let (root, xdg_data_home) = setup_env("prudence-cli-clear");
    let workdir = root.join("work");
    fs::create_dir_all(&workdir).unwrap();
    let alpha = workdir.join("alpha.txt");
    let beta = workdir.join("beta.txt");
    fs::write(&alpha, b"a").unwrap();
    fs::write(&beta, b"b").unwrap();

    run_ok(&workdir, &xdg_data_home, &[alpha.to_str().unwrap()]);
    run_ok(&workdir, &xdg_data_home, &[beta.to_str().unwrap()]);

    let cleared = run_ok(&workdir, &xdg_data_home, &["clear"]);
    assert!(cleared.contains("cleared trash"));
    assert!(!xdg_data_home.join("Trash/files/alpha.txt").exists());
    assert!(!xdg_data_home.join("Trash/files/beta.txt").exists());
    assert!(
        !xdg_data_home
            .join("Trash/info/alpha.txt.trashinfo")
            .exists()
    );
    assert!(!xdg_data_home.join("Trash/info/beta.txt.trashinfo").exists());

    let listed = run_ok(&workdir, &xdg_data_home, &["list"]);
    assert_eq!(listed.trim(), "trash is empty");

    let _ = fs::remove_dir_all(root);
}

#[test]
fn malformed_trashinfo_is_skipped_with_a_warning() {
    let (root, xdg_data_home) = setup_env("prudence-cli-bad-info");
    let trash_files = xdg_data_home.join("Trash/files");
    let trash_info = xdg_data_home.join("Trash/info");
    fs::create_dir_all(&trash_files).unwrap();
    fs::create_dir_all(&trash_info).unwrap();

    fs::write(trash_files.join("broken"), b"x").unwrap();
    fs::write(
        trash_info.join("broken.trashinfo"),
        "[Trash Info]\nDeletionDate=2026-04-13T00:00:00\n",
    )
    .unwrap();
    fs::write(trash_files.join("good"), b"y").unwrap();
    fs::write(
        trash_info.join("good.trashinfo"),
        "[Trash Info]\nPath=/tmp/good\nDeletionDate=2026-04-13T00:00:01\n",
    )
    .unwrap();

    let output = Command::new(binary())
        .env("XDG_DATA_HOME", &xdg_data_home)
        .arg("list")
        .output()
        .unwrap();

    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stdout.contains("good"));
    assert!(!stdout.contains("broken"));
    assert!(stderr.contains("warning"));

    let _ = fs::remove_dir_all(root);
}

fn run_ok(workdir: &Path, xdg_data_home: &Path, args: &[&str]) -> String {
    let output = Command::new(binary())
        .current_dir(workdir)
        .env("XDG_DATA_HOME", xdg_data_home)
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    String::from_utf8(output.stdout).unwrap()
}

fn first_entry_id(list_output: &str) -> String {
    list_output
        .lines()
        .find_map(|line| {
            line.strip_prefix('[')
                .and_then(|line| line.strip_suffix(']'))
        })
        .unwrap()
        .to_string()
}
