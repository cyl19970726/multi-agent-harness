use std::env;
use std::fs;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = env::args_os().skip(1);
    let Some(command) = args.next() else {
        eprintln!("usage: star-harness-install-fs hard-link-no-replace <source> <destination>");
        return ExitCode::from(2);
    };
    let Some(source) = args.next() else {
        eprintln!("missing hard-link source");
        return ExitCode::from(2);
    };
    let Some(destination) = args.next() else {
        eprintln!("missing hard-link destination");
        return ExitCode::from(2);
    };
    if args.next().is_some() || command != "hard-link-no-replace" {
        eprintln!("usage: star-harness-install-fs hard-link-no-replace <source> <destination>");
        return ExitCode::from(2);
    }

    match fs::hard_link(source, destination) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("hard-link-no-replace failed: {error}");
            ExitCode::FAILURE
        }
    }
}
