// The release build has no console window on Windows; what it prints to stderr goes to its log file.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use std::process::ExitCode;

use game37::{app, cli};

#[cfg(windows)]
mod log_file;

fn main() -> ExitCode {
    let options = match cli::parse(std::env::args().skip(1)) {
        Ok(options) => options,
        Err(e) => {
            eprintln!("game37: {e}\n{}", cli::USAGE);
            return ExitCode::from(2);
        }
    };
    // Screenshot mode keeps the stderr it was given, so the scripts that run it see what it prints.
    #[cfg(windows)]
    if options.screenshot.is_none() {
        let data_dir = options.data_dir.clone().unwrap_or_else(game37::config::data_dir);
        if let Err(e) = log_file::redirect_stderr(&data_dir.join("logs")) {
            eprintln!("game37: can't write the log file: {e}");
        }
    }
    match app::run(options) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("game37: {e}");
            ExitCode::FAILURE
        }
    }
}
