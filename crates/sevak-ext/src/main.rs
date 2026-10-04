//! `sevak-ext`: the command-line tool for Sevak extension authors.

use std::io::Write;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut stdout = std::io::stdout().lock();
    match sevak_ext::run(&args, &mut stdout) {
        Ok(()) => {
            let _ = stdout.flush();
            ExitCode::SUCCESS
        }
        Err(sevak_ext::Failure::Usage(message)) => {
            eprintln!("sevak-ext: {message}\n\n{}", sevak_ext::USAGE);
            ExitCode::from(2)
        }
        Err(sevak_ext::Failure::Message(message)) => {
            let _ = stdout.flush();
            eprintln!("sevak-ext: {message}");
            ExitCode::FAILURE
        }
    }
}
