//! Binary entry point for the `piquelcli` command-line tool.

use piquelcli::cli;

fn main() {
    if let Err(err) = cli::run() {
        eprintln!("{err}");
        std::process::exit(1);
    }
}
