//! End-to-end tests for the `magicjar` binary.
//!
//! These exercise the real CLI with `assert_cmd`, a real runnable hello-world
//! jar (`tests/fixtures/hello.jar`), the actual bioconda fgbio wrapper shim
//! (`tests/fixtures/fgbio_shim.py`), and a mocked `$SHELL` for alias resolution.
//! Tests that execute a JVM are skipped when `java` is not on the `PATH`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use assert_cmd::Command as AssertCommand;
use pretty_assertions::assert_eq;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn hello_jar() -> PathBuf {
    fixtures().join("hello.jar")
}

fn magicjar() -> AssertCommand {
    AssertCommand::cargo_bin("magicjar").unwrap()
}

fn have_java() -> bool {
    Command::new("java")
        .arg("-version")
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false)
}

/// Assert that `output` is exactly `PREAMBLE` followed by the bytes of `jar`.
fn assert_magicjar_layout(output: &Path, jar: &Path) {
    let out = fs::read(output).unwrap();
    let jar = fs::read(jar).unwrap();
    let preamble = magicjarlib::PREAMBLE.as_bytes();
    assert!(
        out.starts_with(preamble),
        "output should begin with the shell preamble"
    );
    assert_eq!(
        out.len(),
        preamble.len() + jar.len(),
        "output length should equal preamble + jar"
    );
    assert_eq!(
        &out[preamble.len()..],
        &jar[..],
        "the appended bytes should be the original jar, untouched"
    );
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(path).unwrap().permissions().mode() & 0o111 != 0
}

/// Write an executable mock `$SHELL` that echoes `line` for any arguments.
fn write_mock_shell(dir: &Path, line: &str) -> PathBuf {
    let path = dir.join("mock_shell.sh");
    fs::write(
        &path,
        format!(
            "#!/usr/bin/env bash\nprintf '%s\\n' {}\n",
            shell_quote(line)
        ),
    )
    .unwrap();
    make_executable(&path);
    path
}

fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

#[cfg(unix)]
fn make_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = fs::metadata(path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(path, perms).unwrap();
}

#[test]
fn prepends_preamble_and_marks_executable() {
    let dir = tempfile::tempdir().unwrap();
    let jar = dir.path().join("app.jar");
    fs::copy(hello_jar(), &jar).unwrap();
    let out = dir.path().join("app");

    magicjar().arg(&jar).arg(&out).assert().success();

    assert_magicjar_layout(&out, &jar);
    #[cfg(unix)]
    assert!(is_executable(&out), "output should be executable");
}

#[test]
fn default_output_name_strips_jar_suffix() {
    let dir = tempfile::tempdir().unwrap();
    fs::copy(hello_jar(), dir.path().join("app.jar")).unwrap();

    // No output argument: defaults to "app" in the working directory.
    magicjar()
        .current_dir(dir.path())
        .arg("app.jar")
        .assert()
        .success();

    let out = dir.path().join("app");
    assert!(out.exists(), "default output 'app' should exist");
    assert_magicjar_layout(&out, &dir.path().join("app.jar"));
}

#[test]
fn refuses_to_clobber_then_force_overwrites() {
    let dir = tempfile::tempdir().unwrap();
    let jar = dir.path().join("app.jar");
    fs::copy(hello_jar(), &jar).unwrap();
    let out = dir.path().join("out");
    fs::write(&out, b"preexisting").unwrap();

    magicjar()
        .arg(&jar)
        .arg(&out)
        .assert()
        .failure()
        .stderr(predicates::str::contains("already exists"));

    magicjar()
        .arg(&jar)
        .arg(&out)
        .arg("--force")
        .assert()
        .success();
    assert_magicjar_layout(&out, &jar);
}

#[test]
fn rejects_identical_input_and_output() {
    let dir = tempfile::tempdir().unwrap();
    let jar = dir.path().join("app.jar");
    fs::copy(hello_jar(), &jar).unwrap();

    magicjar()
        .arg(&jar)
        .arg(&jar)
        .arg("--force")
        .assert()
        .failure()
        .stderr(predicates::str::contains("same file"));
}

#[test]
fn resolves_symlink_to_jar() {
    let dir = tempfile::tempdir().unwrap();
    let jar = dir.path().join("app.jar");
    fs::copy(hello_jar(), &jar).unwrap();
    let link = dir.path().join("link.jar");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&jar, &link).unwrap();
    let out = dir.path().join("out");

    magicjar().arg(&link).arg(&out).assert().success();
    assert_magicjar_layout(&out, &jar);
}

#[test]
fn rejects_non_jar_binary() {
    let dir = tempfile::tempdir().unwrap();
    let blob = dir.path().join("blob");
    fs::write(&blob, [0u8, 1, 2, 3, 0, 9]).unwrap();
    let out = dir.path().join("out");

    magicjar()
        .arg(&blob)
        .arg(&out)
        .assert()
        .failure()
        .stderr(predicates::str::contains("neither a .jar"));
}

#[test]
fn resolves_real_bioconda_fgbio_shim() {
    // Reconstruct the conda layout the fgbio shim expects:
    //   <prefix>/bin/fgbio           (the Python wrapper)
    //   <prefix>/share/fgbio/fgbio.jar
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join("bin");
    let share = dir.path().join("share").join("fgbio");
    fs::create_dir_all(&bin).unwrap();
    fs::create_dir_all(&share).unwrap();

    let wrapper = bin.join("fgbio");
    fs::copy(fixtures().join("fgbio_shim.py"), &wrapper).unwrap();
    #[cfg(unix)]
    make_executable(&wrapper);

    let jar = share.join("fgbio.jar");
    fs::copy(hello_jar(), &jar).unwrap();

    let out = dir.path().join("fgbio");
    magicjar().arg(&wrapper).arg(&out).assert().success();
    assert_magicjar_layout(&out, &jar);

    if have_java() {
        let run = Command::new(&out).output().unwrap();
        assert!(run.status.success(), "re-wrapped fgbio shim should run");
        assert_eq!(String::from_utf8_lossy(&run.stdout).trim(), "Hello, World!");
    }
}

#[test]
fn magicked_jar_runs_and_is_still_a_jar() {
    if !have_java() {
        eprintln!("skipping: no `java` on PATH");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let jar = dir.path().join("hello.jar");
    fs::copy(hello_jar(), &jar).unwrap();
    let out = dir.path().join("hello");

    magicjar().arg(&jar).arg(&out).assert().success();

    // The output runs directly as a program...
    let direct = Command::new(&out).output().unwrap();
    assert!(direct.status.success());
    assert_eq!(
        String::from_utf8_lossy(&direct.stdout).trim(),
        "Hello, World!"
    );

    // ...and still works as a jar (the prepended bytes do not break `java -jar`).
    let as_jar = Command::new("java").arg("-jar").arg(&out).output().unwrap();
    assert!(as_jar.status.success());
    assert_eq!(
        String::from_utf8_lossy(&as_jar.stdout).trim(),
        "Hello, World!"
    );
}

#[test]
fn resolves_mocked_shell_alias() {
    let dir = tempfile::tempdir().unwrap();
    let jar = dir.path().join("app.jar");
    fs::copy(hello_jar(), &jar).unwrap();

    // A mock $SHELL that prints an alias definition pointing at our jar.
    let alias_line = format!("myalias='java -jar {}'", jar.display());
    let shell = write_mock_shell(dir.path(), &alias_line);

    let out = dir.path().join("aliased");
    magicjar()
        .current_dir(dir.path())
        .env("SHELL", &shell)
        .arg("myalias")
        .arg(&out)
        .assert()
        .success();
    assert_magicjar_layout(&out, &jar);
}

#[test]
fn errors_on_unknown_alias() {
    let dir = tempfile::tempdir().unwrap();
    // A mock $SHELL that knows no aliases (prints nothing).
    let shell = write_mock_shell(dir.path(), "");

    magicjar()
        .current_dir(dir.path())
        .env("SHELL", &shell)
        .arg("definitely-not-a-real-thing")
        .arg(dir.path().join("out"))
        .assert()
        .failure()
        .stderr(predicates::str::contains("no file or shell alias"));
}
