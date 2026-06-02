//! Binary entry point for the `piquelcli` command-line tool.

use std::error::Error;

use piquelcli::cli;

fn main() -> Result<(), Box<dyn Error>> {
    cli::run()
}
