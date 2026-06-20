//! `magicjarlib` is the engine behind the `magicjar` binary.
//!
//! It resolves an input down to a single concrete `.jar` on disk, then prepends
//! a shell preamble and marks the result executable, producing one portable
//! self-executing file. The input may be:
//!
//! - a `.jar` file (by suffix, case-insensitive, or by ZIP magic);
//! - a symlink to a `.jar`;
//! - a conda/bioconda wrapper script (the `jar_wrapper` shim) that points at a
//!   jar elsewhere in the install;
//! - a shell alias whose definition launches a jar.

#![warn(missing_docs)]

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};

/// The shell preamble prepended to the jar.
///
/// A JAR is a ZIP archive whose central directory is read from the *end* of the
/// file, so these leading bytes are ignored by `java -jar` while `./file` runs
/// this script (which re-execs the JVM on itself).
pub const PREAMBLE: &str = include_str!("../preamble.sh");

/// The result of a successful [`run`].
#[derive(Debug, Clone)]
pub struct Outcome {
    /// The executable file that was written.
    pub output: PathBuf,
    /// The concrete source `.jar` the input resolved to.
    pub source: PathBuf,
}

/// Resolve `input`, prepend [`PREAMBLE`] to its jar, and write an executable
/// `output`.
///
/// When `output` is `None` it defaults to the input's basename with a trailing
/// (case-insensitive) `.jar` removed. An existing output is not overwritten
/// unless `force` is set.
pub fn run(input: &str, output: Option<&str>, force: bool) -> Result<Outcome> {
    let source = resolve_source(input)?;
    validate_source(&source)?;

    let output = match output {
        Some(name) => PathBuf::from(name),
        None => PathBuf::from(default_output_name(input)),
    };

    write_executable(PREAMBLE, &source, &output, force)?;
    Ok(Outcome { output, source })
}

/// Resolve an input string to the concrete `.jar` it stands for.
///
/// If the input is an existing path it is canonicalized (following symlinks); a
/// resolved jar is returned directly, otherwise a text file is treated as a
/// wrapper script. A non-existent path is treated as a shell alias name.
pub fn resolve_source(input: &str) -> Result<PathBuf> {
    let path = Path::new(input);
    if path.exists() {
        let real =
            std::fs::canonicalize(path).with_context(|| format!("cannot resolve path: {input}"))?;
        if looks_like_jar(&real) {
            return Ok(real);
        }
        match read_text_head(&real, 1 << 20) {
            Some(text) => resolve_wrapper_jar(&real, &text),
            None => bail!(
                "{} is neither a .jar nor a text wrapper script that references one",
                real.display()
            ),
        }
    } else {
        resolve_alias(input)
    }
}

/// Compute the default output name: the input basename minus a `.jar` suffix.
pub fn default_output_name(input: &str) -> String {
    let base = Path::new(input)
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| input.to_string());
    strip_jar_suffix(&base).to_string()
}

/// Whether a string ends with `.jar` (case-insensitive).
fn has_jar_suffix(name: &str) -> bool {
    name.len() >= 4 && name[name.len() - 4..].eq_ignore_ascii_case(".jar")
}

/// Strip a trailing `.jar` (case-insensitive) if present.
fn strip_jar_suffix(name: &str) -> &str {
    if has_jar_suffix(name) {
        &name[..name.len() - 4]
    } else {
        name
    }
}

/// Whether a path looks like a JAR: a `.jar` suffix, or ZIP magic bytes.
fn looks_like_jar(path: &Path) -> bool {
    if has_jar_suffix(&path.to_string_lossy()) {
        return true;
    }
    if let Ok(mut file) = std::fs::File::open(path) {
        let mut magic = [0u8; 4];
        if file.read_exact(&mut magic).is_ok() {
            return matches!(&magic, b"PK\x03\x04" | b"PK\x05\x06" | b"PK\x07\x08");
        }
    }
    false
}

/// Validate that a resolved source is a real, jar-like file.
fn validate_source(path: &Path) -> Result<()> {
    let meta = std::fs::metadata(path)
        .with_context(|| format!("source jar does not exist: {}", path.display()))?;
    if !meta.is_file() {
        bail!("source is not a regular file: {}", path.display());
    }
    if !looks_like_jar(path) {
        bail!(
            "source does not look like a .jar (no .jar suffix or ZIP magic): {}",
            path.display()
        );
    }
    Ok(())
}

/// Read up to `max` bytes and return them as UTF-8 text, or `None` if the file
/// contains a NUL byte (i.e. looks binary) or is not valid UTF-8.
fn read_text_head(path: &Path, max: usize) -> Option<String> {
    let mut file = std::fs::File::open(path).ok()?;
    let mut buf = vec![0u8; max];
    let read = file.read(&mut buf).ok()?;
    buf.truncate(read);
    if buf.contains(&0) {
        return None;
    }
    String::from_utf8(buf).ok()
}

/// Write `preamble` followed by the bytes of `source` to `output`, then mark it
/// executable. Refuses to clobber an existing file (unless `force`) or to write
/// onto the source itself.
fn write_executable(preamble: &str, source: &Path, output: &Path, force: bool) -> Result<()> {
    if output.is_dir() {
        bail!("output path is a directory: {}", output.display());
    }
    if output.exists() {
        if same_file(source, output) {
            bail!("input and output are the same file: {}", output.display());
        }
        if !force {
            bail!(
                "{} already exists (use --force to overwrite)",
                output.display()
            );
        }
    }

    let mut src = std::fs::File::open(source)
        .with_context(|| format!("cannot open source jar: {}", source.display()))?;
    let mut out = std::fs::File::create(output)
        .with_context(|| format!("cannot create output: {}", output.display()))?;
    out.write_all(preamble.as_bytes())
        .with_context(|| format!("failed writing preamble to {}", output.display()))?;
    std::io::copy(&mut src, &mut out)
        .with_context(|| format!("failed appending jar to {}", output.display()))?;
    out.flush()?;
    drop(out);

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(output)?.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(output, perms)
            .with_context(|| format!("failed to chmod {}", output.display()))?;
    }
    Ok(())
}

/// Whether two paths refer to the same existing file (by canonical path).
fn same_file(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => false,
    }
}

/// Resolve a shell alias name to the jar it launches by asking the user's shell
/// (`$SHELL -ic 'alias <name>'`) and parsing the definition.
fn resolve_alias(name: &str) -> Result<PathBuf> {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
    let output = std::process::Command::new(&shell)
        .arg("-ic")
        .arg(format!("alias {name}"))
        .output()
        .with_context(|| {
            format!("no file '{name}', and failed to run {shell} to look up an alias")
        })?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let line = stdout
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && line.contains(name))
        .ok_or_else(|| anyhow!("no file or shell alias named '{name}'"))?;

    let value = alias_value(line, name);
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    resolve_jar_from_text(&value, &cwd, None)
        .with_context(|| format!("shell alias '{name}' does not resolve to a .jar: {line}"))
}

/// Extract the value of an `alias` line, e.g. `foo='java -jar x.jar'` -> the
/// inner command `java -jar x.jar`.
fn alias_value(line: &str, name: &str) -> String {
    let line = line.strip_prefix("alias ").unwrap_or(line).trim();
    let prefix = format!("{name}=");
    let value = line.strip_prefix(&prefix).unwrap_or(line);
    strip_outer_quotes(value).to_string()
}

/// Strip one layer of matching surrounding single or double quotes.
fn strip_outer_quotes(s: &str) -> &str {
    let s = s.trim();
    let bytes = s.as_bytes();
    if bytes.len() >= 2
        && (bytes[0] == b'\'' || bytes[0] == b'"')
        && bytes[bytes.len() - 1] == bytes[0]
    {
        &s[1..s.len() - 1]
    } else {
        s
    }
}

/// Resolve the jar referenced by a wrapper script located at `script`.
fn resolve_wrapper_jar(script: &Path, text: &str) -> Result<PathBuf> {
    let base_dir = script.parent().unwrap_or_else(|| Path::new("."));
    // When the wrapper sits at `<prefix>/bin/<tool>`, its grandparent is the
    // conda/install prefix that holds the jar under `share/`.
    let prefix = base_dir.parent();
    resolve_jar_from_text(text, base_dir, prefix).with_context(|| {
        format!(
            "could not find the .jar referenced by wrapper script {}",
            script.display()
        )
    })
}

/// Find a concrete jar referenced by some shell/python text.
///
/// `base_dir` is the directory used to resolve script-relative references (the
/// wrapper's own directory, or the cwd for aliases). `prefix_hint`, when set, is
/// the install prefix searched as a last resort for a jar by basename.
fn resolve_jar_from_text(
    text: &str,
    base_dir: &Path,
    prefix_hint: Option<&Path>,
) -> Result<PathBuf> {
    // 1) An explicit `-jar <path>` on some line. Tokenize with shell-words to
    //    honor quoting and spaces. Covers shell wrappers and aliases.
    for line in text.lines() {
        if !line.contains("-jar") {
            continue;
        }
        let Ok(tokens) = shell_words::split(line.trim()) else {
            continue;
        };
        if let Some(pos) = tokens.iter().position(|token| token == "-jar")
            && let Some(candidate) = tokens.get(pos + 1)
            && has_jar_suffix(candidate)
            && let Some(found) = canonicalize_existing(&expand(candidate, base_dir, prefix_hint))
        {
            return Ok(found);
        }
    }

    // 2) The bioconda `jar_wrapper` template: `PKG_NAME` + `JAR_NAME`, with the
    //    jar installed at `<prefix>/share/<PKG_NAME>/<JAR_NAME>`.
    if let Some(prefix) = prefix_hint
        && let Some(jar_name) = assignment_value(text, "JAR_NAME")
        && has_jar_suffix(&jar_name)
    {
        if let Some(pkg) = assignment_value(text, "PKG_NAME") {
            let candidate = prefix.join("share").join(&pkg).join(&jar_name);
            if let Some(found) = canonicalize_existing(&candidate.to_string_lossy()) {
                return Ok(found);
            }
        }
        if let Some(found) = search_prefix_for_jar(prefix, base_dir, &jar_name) {
            return Ok(found);
        }
    }

    // 3) Any `.jar` token in the text: expand and resolve directly, else search
    //    the prefix by basename.
    let candidates = collect_jar_candidates(text);
    for candidate in &candidates {
        if let Some(found) = canonicalize_existing(&expand(candidate, base_dir, prefix_hint)) {
            return Ok(found);
        }
    }
    if let Some(prefix) = prefix_hint {
        for candidate in &candidates {
            if let Some(base) = Path::new(candidate).file_name() {
                let base = base.to_string_lossy();
                if let Some(found) = search_prefix_for_jar(prefix, base_dir, &base) {
                    return Ok(found);
                }
            }
        }
    }

    bail!("no resolvable .jar reference found")
}

/// Canonicalize a path string if it exists, else `None`.
fn canonicalize_existing(path: &str) -> Option<PathBuf> {
    let path = Path::new(path);
    if path.exists() {
        std::fs::canonicalize(path).ok()
    } else {
        None
    }
}

/// Patterns commonly used in wrapper scripts to refer to the script's own
/// directory; each is replaced with `base_dir` during expansion.
const SCRIPT_DIR_PATTERNS: &[&str] = &[
    "$(cd \"$(dirname \"$0\")\" && pwd)",
    "$(dirname \"$0\")",
    "$(dirname $0)",
    "${0%/*}",
    "$SCRIPTPATH",
    "${SCRIPTPATH}",
    "$SCRIPT_DIR",
    "${SCRIPT_DIR}",
    "$DIR",
    "${DIR}",
    "$HERE",
    "${HERE}",
    "$here",
    "${here}",
];

/// Expand script-relative constructs, `~`, conda prefix variables, and any other
/// `$VAR`/`${VAR}` references in a candidate path.
fn expand(token: &str, base_dir: &Path, prefix_hint: Option<&Path>) -> String {
    let mut s = token.to_string();

    let base = base_dir.to_string_lossy();
    for pattern in SCRIPT_DIR_PATTERNS {
        if s.contains(pattern) {
            s = s.replace(pattern, &base);
        }
    }

    // `$PREFIX` / `$CONDA_PREFIX` fall back to the inferred install prefix when
    // they are not set in the environment.
    if let Some(prefix) = prefix_hint {
        let prefix = prefix.to_string_lossy();
        for var in ["PREFIX", "CONDA_PREFIX"] {
            if std::env::var(var).is_err() {
                s = s.replace(&format!("${{{var}}}"), &prefix);
                s = s.replace(&format!("${var}"), &prefix);
            }
        }
    }

    if let Some(rest) = s.strip_prefix("~/")
        && let Ok(home) = std::env::var("HOME")
    {
        s = format!("{home}/{rest}");
    }

    expand_env(&s)
}

/// Expand `$VAR` and `${VAR}` from the environment; unknown variables expand to
/// the empty string.
fn expand_env(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(pos) = rest.find('$') {
        out.push_str(&rest[..pos]);
        let after = &rest[pos + 1..];
        if let Some(braced) = after.strip_prefix('{') {
            if let Some(close) = braced.find('}') {
                let name = &braced[..close];
                if is_var_name(name) {
                    if let Ok(value) = std::env::var(name) {
                        out.push_str(&value);
                    }
                    rest = &braced[close + 1..];
                    continue;
                }
            }
            out.push('$');
            rest = after;
        } else {
            let len = after
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .unwrap_or(after.len());
            if len > 0 {
                let name = &after[..len];
                if let Ok(value) = std::env::var(name) {
                    out.push_str(&value);
                }
                rest = &after[len..];
            } else {
                out.push('$');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Whether `name` is a valid shell variable name.
fn is_var_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Find the value of a `KEY = '...'` / `KEY="..."` / `KEY=...` assignment.
fn assignment_value(text: &str, key: &str) -> Option<String> {
    for line in text.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix(key) else {
            continue;
        };
        if !(rest.starts_with(char::is_whitespace) || rest.starts_with('=')) {
            continue;
        }
        let rest = rest.trim_start();
        let Some(rest) = rest.strip_prefix('=') else {
            continue;
        };
        let value = strip_outer_quotes(rest.trim());
        let value = value.split_whitespace().next().unwrap_or(value);
        if !value.is_empty() {
            return Some(value.to_string());
        }
    }
    None
}

/// Collect every `.jar` token appearing in arbitrary text.
fn collect_jar_candidates(text: &str) -> Vec<String> {
    let lower = text.to_ascii_lowercase();
    let bytes = text.as_bytes();
    let mut out: Vec<String> = Vec::new();
    let mut from = 0;
    while let Some(rel) = lower[from..].find(".jar") {
        let dot = from + rel;
        let end = dot + 4;
        from = end;
        // The character after `.jar` must be a delimiter (not e.g. `.jarx`).
        if let Some(&next) = bytes.get(end)
            && !is_delim(next)
            && next != b'.'
        {
            continue;
        }
        let mut start = dot;
        while start > 0 && is_token_char(bytes[start - 1]) {
            start -= 1;
        }
        if end > start {
            let token = &text[start..end];
            if !token.is_empty() && !out.iter().any(|t| t == token) {
                out.push(token.to_string());
            }
        }
    }
    out
}

/// Whether a byte ends a path-like token.
fn is_delim(c: u8) -> bool {
    matches!(
        c,
        b' ' | b'\t'
            | b'\n'
            | b'\r'
            | b'\''
            | b'"'
            | b'='
            | b'('
            | b')'
            | b'['
            | b']'
            | b'{'
            | b'}'
            | b','
            | b';'
            | b'`'
            | b'<'
            | b'>'
            | b'|'
            | b'&'
            | b':'
    )
}

/// Whether a byte may be part of a path-like token (printable ASCII, not a
/// delimiter). Restricting to ASCII keeps slice boundaries valid.
fn is_token_char(c: u8) -> bool {
    (0x21..0x80).contains(&c) && !is_delim(c)
}

/// Search common jar locations under an install prefix for a file with the given
/// basename, returning the first (deterministic) match.
fn search_prefix_for_jar(prefix: &Path, base_dir: &Path, basename: &str) -> Option<PathBuf> {
    let roots = [
        prefix.join("share"),
        prefix.join("opt"),
        prefix.join("libexec"),
        prefix.join("lib"),
        prefix.join("jars"),
        base_dir.to_path_buf(),
        prefix.to_path_buf(),
    ];
    for root in roots {
        if let Some(hit) = walk_find(&root, basename, 7) {
            return Some(std::fs::canonicalize(&hit).unwrap_or(hit));
        }
    }
    None
}

/// Depth-bounded search for a file named `name` (case-insensitive) under `dir`.
/// Files in a directory are checked before recursing; subdirectories are visited
/// in sorted order for deterministic results.
fn walk_find(dir: &Path, name: &str, depth: usize) -> Option<PathBuf> {
    if depth == 0 {
        return None;
    }
    let mut subdirs: Vec<PathBuf> = Vec::new();
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_file() {
            if entry
                .file_name()
                .to_string_lossy()
                .eq_ignore_ascii_case(name)
            {
                return Some(entry.path());
            }
        } else if file_type.is_dir() {
            subdirs.push(entry.path());
        }
    }
    subdirs.sort();
    for subdir in subdirs {
        if let Some(hit) = walk_find(&subdir, name, depth - 1) {
            return Some(hit);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use rstest::rstest;

    #[rstest]
    #[case("fgbio.jar", true)]
    #[case("FGBIO.JAR", true)]
    #[case("Tool.Jar", true)]
    #[case("noext", false)]
    #[case("jar", false)]
    #[case("x.jarx", false)]
    fn jar_suffix(#[case] name: &str, #[case] expected: bool) {
        assert_eq!(has_jar_suffix(name), expected);
    }

    #[rstest]
    #[case("fgbio.jar", "fgbio")]
    #[case("path/to/fgbio.jar", "fgbio")]
    #[case("/opt/conda/bin/fgbio", "fgbio")]
    #[case("Tool.JAR", "Tool")]
    #[case("fgbio", "fgbio")]
    fn default_output(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(default_output_name(input), expected);
    }

    #[rstest]
    #[case("foo='java -jar x.jar'", "foo", "java -jar x.jar")]
    #[case("alias foo='java -jar x.jar'", "foo", "java -jar x.jar")]
    #[case("foo=\"java -jar x.jar\"", "foo", "java -jar x.jar")]
    fn alias_values(#[case] line: &str, #[case] name: &str, #[case] expected: &str) {
        assert_eq!(alias_value(line, name), expected);
    }

    #[test]
    fn collect_candidates_finds_paths_and_names() {
        let text = "exec java -jar \"/opt/share/app.jar\" \"$@\"";
        assert_eq!(collect_jar_candidates(text), vec!["/opt/share/app.jar"]);

        let py = "JAR_NAME = 'fgbio.jar'\nPKG_NAME = 'fgbio'\n";
        assert_eq!(collect_jar_candidates(py), vec!["fgbio.jar"]);
    }

    #[test]
    fn assignment_values_parse() {
        let py = "JAR_NAME = 'fgbio.jar'\nPKG_NAME = \"fgbio\"\n";
        assert_eq!(
            assignment_value(py, "JAR_NAME").as_deref(),
            Some("fgbio.jar")
        );
        assert_eq!(assignment_value(py, "PKG_NAME").as_deref(), Some("fgbio"));
        assert_eq!(assignment_value(py, "MISSING"), None);
    }

    #[test]
    fn expand_env_substitutes_and_drops() {
        unsafe {
            std::env::set_var("MAGICJAR_TEST_VAR", "/abc");
        }
        assert_eq!(expand_env("$MAGICJAR_TEST_VAR/x.jar"), "/abc/x.jar");
        assert_eq!(expand_env("${MAGICJAR_TEST_VAR}/x.jar"), "/abc/x.jar");
        assert_eq!(expand_env("$MAGICJAR_UNSET_VAR/x.jar"), "/x.jar");
        assert_eq!(expand_env("no vars here"), "no vars here");
    }

    #[test]
    fn expand_substitutes_script_dir() {
        let base = Path::new("/envs/x/bin");
        assert_eq!(
            expand("$(dirname \"$0\")/../share/a.jar", base, None),
            "/envs/x/bin/../share/a.jar"
        );
        assert_eq!(expand("${0%/*}/a.jar", base, None), "/envs/x/bin/a.jar");
    }

    #[test]
    fn resolve_jar_via_explicit_jar_flag() {
        let dir = tempfile::tempdir().unwrap();
        let jar = dir.path().join("app.jar");
        std::fs::write(&jar, b"PK\x03\x04 fake").unwrap();
        let text = format!("exec java -jar \"{}\" \"$@\"", jar.display());
        let resolved = resolve_jar_from_text(&text, dir.path(), None).unwrap();
        assert_eq!(resolved, std::fs::canonicalize(&jar).unwrap());
    }

    #[test]
    fn resolve_jar_via_bioconda_template() {
        // Reconstruct `<prefix>/bin/<tool>` + `<prefix>/share/<pkg>/<jar>`.
        let prefix = tempfile::tempdir().unwrap();
        let bin = prefix.path().join("bin");
        let share = prefix.path().join("share").join("fgbio");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::create_dir_all(&share).unwrap();
        let jar = share.join("fgbio.jar");
        std::fs::write(&jar, b"PK\x03\x04 fake").unwrap();

        let text = "JAR_NAME = 'fgbio.jar'\nPKG_NAME = 'fgbio'\n";
        let base_dir = bin.as_path();
        let resolved = resolve_jar_from_text(text, base_dir, Some(prefix.path())).unwrap();
        assert_eq!(resolved, std::fs::canonicalize(&jar).unwrap());
    }

    #[test]
    fn resolve_jar_via_prefix_search_fallback() {
        // Jar basename is known but its directory is not named after the package.
        let prefix = tempfile::tempdir().unwrap();
        let share = prefix.path().join("share").join("fgbio-2.0.0-0");
        std::fs::create_dir_all(&share).unwrap();
        let jar = share.join("fgbio.jar");
        std::fs::write(&jar, b"PK\x03\x04 fake").unwrap();

        let text = "JAR_NAME = 'fgbio.jar'\n"; // no PKG_NAME match for the dir
        let bin = prefix.path().join("bin");
        let resolved = resolve_jar_from_text(text, &bin, Some(prefix.path())).unwrap();
        assert_eq!(resolved, std::fs::canonicalize(&jar).unwrap());
    }

    #[test]
    fn looks_like_jar_by_magic() {
        let dir = tempfile::tempdir().unwrap();
        let zip = dir.path().join("noext");
        std::fs::write(&zip, b"PK\x03\x04rest").unwrap();
        assert!(looks_like_jar(&zip));

        let txt = dir.path().join("plain");
        std::fs::write(&txt, b"not a zip").unwrap();
        assert!(!looks_like_jar(&txt));
    }
}
