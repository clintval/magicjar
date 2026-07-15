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

fn echo_jar() -> PathBuf {
    fixtures().join("echo.jar")
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

fn default_preamble() -> String {
    magicjarlib::build_preamble(&magicjarlib::PreambleOptions::default()).unwrap()
}

/// Assert that `output` is exactly `preamble` followed by the bytes of `jar`.
fn assert_layout(output: &Path, jar: &Path, preamble: &str) {
    let out = fs::read(output).unwrap();
    let jar = fs::read(jar).unwrap();
    let preamble = preamble.as_bytes();
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

/// Assert the output is the default preamble followed by the jar.
fn assert_magicjar_layout(output: &Path, jar: &Path) {
    assert_layout(output, jar, &default_preamble());
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
        assert!(
            run.status.success(),
            "re-wrapped fgbio shim should run; stderr: {}",
            String::from_utf8_lossy(&run.stderr)
        );
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
    assert!(
        direct.status.success(),
        "the magicked file should run directly; stderr: {}",
        String::from_utf8_lossy(&direct.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&direct.stdout).trim(),
        "Hello, World!"
    );

    // ...and still works as a jar (the prepended bytes do not break `java -jar`).
    let as_jar = Command::new("java").arg("-jar").arg(&out).output().unwrap();
    assert!(
        as_jar.status.success(),
        "the magicked file should still be a valid jar; stderr: {}",
        String::from_utf8_lossy(&as_jar.stderr)
    );
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

#[test]
fn routes_jvm_flags_and_passes_program_args() {
    if !have_java() {
        eprintln!("skipping: no `java` on PATH");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let jar = dir.path().join("echo.jar");
    fs::copy(echo_jar(), &jar).unwrap();
    let out = dir.path().join("echo");

    magicjar().arg(&jar).arg(&out).assert().success();

    // echo.jar prints "prop=<-Dmagicjar.test>" and "args=<program args>".
    // The preamble must route -D to the JVM (so the property is set) and -Xss/
    // -Xint to the JVM (so they do NOT leak into the program's argv).
    let run = Command::new(&out)
        .args(["-Dmagicjar.test=hi", "-Xss4m", "-Xint", "alpha", "beta"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert!(
        run.status.success(),
        "magicked echo should run; stderr: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert!(
        stdout.contains("prop=hi"),
        "-D must reach the JVM; got: {stdout}"
    );
    assert!(
        stdout.contains("args=alpha,beta"),
        "-Xss/-Xint must go to the JVM, not the program; got: {stdout}"
    );
}

#[test]
fn bare_named_file_hands_the_jvm_a_dot_jar_path() {
    // A reflections-based tool (e.g. GATK) only scans classpath entries whose
    // name ends in .jar. Prove that an extensionless magicked file still hands
    // the JVM a .jar-suffixed path. A fake `java` records its argv, so this needs
    // neither a real JVM nor a classpath-scanning jar.
    let dir = tempfile::tempdir().unwrap();
    let jar = dir.path().join("app.jar");
    fs::copy(hello_jar(), &jar).unwrap();
    let out = dir.path().join("mytool"); // no .jar suffix
    magicjar().arg(&jar).arg(&out).assert().success();

    let bindir = dir.path().join("fakebin");
    fs::create_dir_all(&bindir).unwrap();
    let args_file = dir.path().join("java-args.txt");
    let fake_java = bindir.join("java");
    fs::write(
        &fake_java,
        format!(
            "#!/usr/bin/env bash\nprintf '%s\\n' \"$@\" > {}\n",
            shell_quote(args_file.to_str().unwrap())
        ),
    )
    .unwrap();
    make_executable(&fake_java);

    let path = format!(
        "{}:{}",
        bindir.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let status = Command::new(&out).env("PATH", &path).status().unwrap();
    assert!(
        status.success(),
        "the extensionless magicked file should run"
    );

    let recorded = fs::read_to_string(&args_file).unwrap();
    let args: Vec<&str> = recorded.lines().collect();
    let jar_pos = args
        .iter()
        .position(|a| *a == "-jar")
        .expect("the preamble must invoke `java -jar`");
    let jar_arg = args.get(jar_pos + 1).expect("`-jar` needs a path");
    assert!(
        jar_arg.ends_with(".jar"),
        "the classpath entry handed to the JVM must end in .jar; got {jar_arg}"
    );
    assert_eq!(
        Path::new(jar_arg).file_name().and_then(|n| n.to_str()),
        Some("mytool.jar"),
        "the .jar handle should derive from the tool basename; got {jar_arg}"
    );
}

#[test]
#[cfg(unix)]
fn symlink_staged_file_hands_the_jvm_a_real_jar_not_a_symlink() {
    // A workflow engine (e.g. nextflow) stages an input as a symlink in the task
    // dir. The preamble must hand the JVM a *real* .jar file, not a hardlink to
    // the symlink that canonicalizes back to the bare-named target (which would
    // defeat reflections-based tools like GATK). The fake `java` reports whether
    // its .jar argument is a symlink.
    let dir = tempfile::tempdir().unwrap();
    let jar = dir.path().join("app.jar");
    fs::copy(hello_jar(), &jar).unwrap();
    let real = dir.path().join("mytool"); // extensionless real magicked file
    magicjar().arg(&jar).arg(&real).assert().success();

    let taskdir = dir.path().join("task");
    fs::create_dir_all(&taskdir).unwrap();
    let staged = taskdir.join("mytool"); // bare-named symlink, like nextflow
    std::os::unix::fs::symlink(&real, &staged).unwrap();

    let bindir = dir.path().join("fakebin");
    fs::create_dir_all(&bindir).unwrap();
    let kind_file = dir.path().join("jar-kind.txt");
    let fake_java = bindir.join("java");
    fs::write(
        &fake_java,
        format!(
            "#!/usr/bin/env bash\nfor a in \"$@\"; do case \"$a\" in *.jar) if [ -L \"$a\" ]; then echo symlink > {k}; else echo real > {k}; fi;; esac; done\n",
            k = shell_quote(kind_file.to_str().unwrap())
        ),
    )
    .unwrap();
    make_executable(&fake_java);

    let path = format!(
        "{}:{}",
        bindir.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let status = Command::new(&staged).env("PATH", &path).status().unwrap();
    assert!(status.success(), "the symlink-staged file should run");
    assert_eq!(
        fs::read_to_string(&kind_file).unwrap().trim(),
        "real",
        "the JVM must get a real .jar, not a symlink that resolves back to a bare name"
    );
}

#[test]
fn resolves_shell_wrapper_using_prefix_variable() {
    // A non-Python (shell) wrapper that references the jar via $PREFIX, the way
    // many conda shell shims do. Layout: <prefix>/bin/toolwrap + <prefix>/lib/tool.jar.
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join("bin");
    let lib = dir.path().join("lib");
    fs::create_dir_all(&bin).unwrap();
    fs::create_dir_all(&lib).unwrap();

    let jar = lib.join("tool.jar");
    fs::copy(hello_jar(), &jar).unwrap();

    let wrapper = bin.join("toolwrap");
    fs::write(
        &wrapper,
        "#!/usr/bin/env bash\nexec java -jar \"$PREFIX/lib/tool.jar\" \"$@\"\n",
    )
    .unwrap();
    #[cfg(unix)]
    make_executable(&wrapper);

    let out = dir.path().join("toolwrap.exe");
    magicjar()
        // Force the inferred-prefix path (don't let an ambient $PREFIX interfere).
        .env_remove("PREFIX")
        .env_remove("CONDA_PREFIX")
        .arg(&wrapper)
        .arg(&out)
        .assert()
        .success();
    assert_magicjar_layout(&out, &jar);
}

#[test]
fn no_malloc_arena_max_flag_omits_the_block() {
    let dir = tempfile::tempdir().unwrap();
    let jar = dir.path().join("app.jar");
    fs::copy(hello_jar(), &jar).unwrap();
    let out = dir.path().join("app");

    magicjar()
        .arg(&jar)
        .arg(&out)
        .arg("--no-malloc-arena-max")
        .assert()
        .success();

    let preamble = magicjarlib::build_preamble(&magicjarlib::PreambleOptions {
        malloc_arena_max: false,
        ..Default::default()
    })
    .unwrap();
    assert!(
        !preamble.contains("MALLOC_ARENA_MAX"),
        "malloc block should be omitted"
    );
    assert_layout(&out, &jar, &preamble);
}

#[test]
fn default_jvm_opts_empty_omits_heap_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let jar = dir.path().join("app.jar");
    fs::copy(hello_jar(), &jar).unwrap();
    let out = dir.path().join("app");

    magicjar()
        .arg(&jar)
        .arg(&out)
        .arg("--default-jvm-opts")
        .arg("")
        .assert()
        .success();

    let preamble = magicjarlib::build_preamble(&magicjarlib::PreambleOptions {
        default_jvm_opts: String::new(),
        ..Default::default()
    })
    .unwrap();
    assert!(
        !preamble.contains("-Xms512m"),
        "default heap opts should be omitted"
    );
    assert!(
        preamble.contains("DEFAULT_MEM_OPTS=()"),
        "expected an empty default mem opts array"
    );
    assert_layout(&out, &jar, &preamble);
}

#[test]
fn errors_on_double_wrap() {
    let dir = tempfile::tempdir().unwrap();
    let jar = dir.path().join("app.jar");
    fs::copy(hello_jar(), &jar).unwrap();
    let once = dir.path().join("once");
    magicjar().arg(&jar).arg(&once).assert().success();

    // Running magicjar on an already-wrapped file must error, not double-wrap.
    let twice = dir.path().join("twice");
    magicjar()
        .arg(&once)
        .arg(&twice)
        .assert()
        .failure()
        .stderr(predicates::str::contains("already a magicjar"));
    assert!(
        !twice.exists(),
        "no output should be written on a double-wrap attempt"
    );
}

#[test]
fn custom_default_jvm_opts_with_hyphen_value() {
    let dir = tempfile::tempdir().unwrap();
    let jar = dir.path().join("app.jar");
    fs::copy(hello_jar(), &jar).unwrap();
    let out = dir.path().join("app");

    // The value begins with '-'; clap must accept it (allow_hyphen_values).
    // Use portable heap flags so the produced file runs on any JVM >= 8.
    magicjar()
        .arg(&jar)
        .arg(&out)
        .arg("--default-jvm-opts")
        .arg("-Xms64m -Xmx128m")
        .assert()
        .success();

    let preamble = magicjarlib::build_preamble(&magicjarlib::PreambleOptions {
        default_jvm_opts: "-Xms64m -Xmx128m".to_string(),
        ..Default::default()
    })
    .unwrap();
    assert!(preamble.contains("-Xms64m") && preamble.contains("-Xmx128m"));
    assert_layout(&out, &jar, &preamble);

    if have_java() {
        let run = Command::new(&out).output().unwrap();
        assert!(
            run.status.success(),
            "custom-heap file should run; stderr: {}",
            String::from_utf8_lossy(&run.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&run.stdout).trim(), "Hello, World!");
    }
}

/// Write a fake `java` at `path` that records `tag` into `marker` when invoked.
/// Used to observe which JVM the preamble actually launches, without a real JVM.
#[cfg(unix)]
fn write_recording_java(path: &Path, tag: &str, marker: &Path) {
    fs::write(
        path,
        format!(
            "#!/usr/bin/env bash\nprintf '%s' {} > {}\n",
            shell_quote(tag),
            shell_quote(marker.to_str().unwrap())
        ),
    )
    .unwrap();
    make_executable(path);
}

#[test]
#[cfg(unix)]
fn magicjar_java_env_overrides_path_java() {
    // $MAGICJAR_JAVA selects the JVM the preamble launches, taking precedence
    // over `java` on PATH. This covers the bare-name launch path, the way a
    // workflow engine stages and runs e.g. ./gatk3-3.8-1.
    let dir = tempfile::tempdir().unwrap();
    let jar = dir.path().join("app.jar");
    fs::copy(hello_jar(), &jar).unwrap();
    let out = dir.path().join("mytool"); // bare name
    magicjar().arg(&jar).arg(&out).assert().success();

    let which = dir.path().join("which-java.txt");
    let pathbin = dir.path().join("pathbin");
    fs::create_dir_all(&pathbin).unwrap();
    write_recording_java(&pathbin.join("java"), "path", &which);
    let override_dir = dir.path().join("jvm8");
    fs::create_dir_all(&override_dir).unwrap();
    let override_java = override_dir.join("java");
    write_recording_java(&override_java, "override", &which);

    let path = format!(
        "{}:{}",
        pathbin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let status = Command::new(&out)
        .env("PATH", &path)
        .env("MAGICJAR_JAVA", &override_java)
        .status()
        .unwrap();
    assert!(status.success(), "the magicked file should run");
    assert_eq!(
        fs::read_to_string(&which).unwrap().trim(),
        "override",
        "the JVM named by $MAGICJAR_JAVA must run, not `java` on PATH"
    );
}

#[test]
#[cfg(unix)]
fn magicjar_java_env_honored_on_dot_jar_fast_path() {
    // A magicked file whose own name ends in .jar takes the fast path
    // (exec ... -jar "$0"); it must honor $MAGICJAR_JAVA too.
    let dir = tempfile::tempdir().unwrap();
    let jar = dir.path().join("app.jar");
    fs::copy(hello_jar(), &jar).unwrap();
    let out = dir.path().join("mytool.jar"); // .jar name -> fast path
    magicjar().arg(&jar).arg(&out).assert().success();

    let which = dir.path().join("which-java.txt");
    let pathbin = dir.path().join("pathbin");
    fs::create_dir_all(&pathbin).unwrap();
    write_recording_java(&pathbin.join("java"), "path", &which);
    let override_dir = dir.path().join("jvm8");
    fs::create_dir_all(&override_dir).unwrap();
    let override_java = override_dir.join("java");
    write_recording_java(&override_java, "override", &which);

    let path = format!(
        "{}:{}",
        pathbin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let status = Command::new(&out)
        .env("PATH", &path)
        .env("MAGICJAR_JAVA", &override_java)
        .status()
        .unwrap();
    assert!(status.success(), "the .jar-named magicked file should run");
    assert_eq!(
        fs::read_to_string(&which).unwrap().trim(),
        "override",
        "the .jar fast-path must honor $MAGICJAR_JAVA"
    );
}
