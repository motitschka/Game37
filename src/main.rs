use std::process::ExitCode;

use game37::{app, cli};

fn main() -> ExitCode {
    let options = match cli::parse(std::env::args().skip(1)) {
        Ok(options) => options,
        Err(e) => {
            eprintln!("game37: {e}\n{}", cli::USAGE);
            return ExitCode::from(2);
        }
    };
    match app::run(options) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("game37: {e}");
            ExitCode::FAILURE
        }
    }
}
