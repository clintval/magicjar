use std::process::ExitCode;

use clap::Parser;
use clap::builder::styling::{AnsiColor, Effects, Styles};

/// Cargo-style help colors (green headers, cyan literals), matching the other
/// tools in this account.
const STYLES: Styles = Styles::styled()
    .header(AnsiColor::Green.on_default().effects(Effects::BOLD))
    .usage(AnsiColor::Green.on_default().effects(Effects::BOLD))
    .literal(AnsiColor::Cyan.on_default().effects(Effects::BOLD))
    .placeholder(AnsiColor::Cyan.on_default());

/// Make a Java JAR self-executing by prepending a shell preamble.
///
/// The input may be a `.jar`, a symlink to one, a conda/bioconda wrapper script,
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
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match magicjarlib::run(&cli.input, cli.output.as_deref(), cli.force) {
        Ok(outcome) => {
            eprintln!("source: {}", outcome.source.display());
            eprintln!("wrote {} (executable)", outcome.output.display());
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("magicjar: error: {err:#}");
            ExitCode::FAILURE
        }
    }
}
