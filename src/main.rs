use std::process::ExitCode;

use clap::Parser;
use clap::builder::styling::{AnsiColor, Effects, Styles};

/// Cargo-style help colors (green headers, cyan literals).
const STYLES: Styles = Styles::styled()
    .header(AnsiColor::Green.on_default().effects(Effects::BOLD))
    .usage(AnsiColor::Green.on_default().effects(Effects::BOLD))
    .literal(AnsiColor::Cyan.on_default().effects(Effects::BOLD))
    .placeholder(AnsiColor::Cyan.on_default());

/// Make a Java JAR self-executing by prepending a shell preamble.
///
/// The input may be a JAR, a symlink to one, a conda/bioconda wrapper script,
/// or a shell alias; magicjar resolves any of these to the underlying archive,
/// prepends a shell preamble, and marks the result executable. The output then
/// runs directly (`./fgbio`) and still works as a jar (`java -jar fgbio`).
#[derive(Debug, Parser)]
#[command(name = "magicjar", version, about, long_about = None, styles = STYLES, term_width = 80)]
struct Cli {
    /// The .jar, symlink, wrapper script, or shell alias to make executable.
    input: String,

    /// Output file name [default: the input basename without its .jar suffix].
    output: Option<String>,

    /// Overwrite the output file if it already exists.
    #[arg(short, long)]
    force: bool,

    /// Do not set MALLOC_ARENA_MAX in the generated preamble (the glibc
    /// arena-limiting hack is included by default).
    #[arg(long)]
    no_malloc_arena_max: bool,

    /// JVM options applied only when the caller sets no heap preference of their
    /// own. Pass "" to apply none.
    #[arg(
        long,
        default_value = "-Xms512m -XX:+AggressiveHeap",
        value_name = "OPTS",
        allow_hyphen_values = true
    )]
    default_jvm_opts: String,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let options = magicjarlib::PreambleOptions {
        malloc_arena_max: !cli.no_malloc_arena_max,
        default_jvm_opts: cli.default_jvm_opts,
    };
    match magicjarlib::run(&cli.input, cli.output.as_deref(), cli.force, &options) {
        Ok(_) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("magicjar: error: {err:#}");
            ExitCode::FAILURE
        }
    }
}
