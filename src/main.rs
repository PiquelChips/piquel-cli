use std::error::Error;

use piquelcli::cli;

fn main() -> Result<(), Box<dyn Error>> {
    cli::run()
}
