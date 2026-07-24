use std::process::ExitCode;

use nitidus::{config, logging};

fn main() -> ExitCode {
    let guard = match logging::init() {
        Ok(guard) => guard,
        Err(error) => return fail(&error),
    };
    let loaded = match config::load() {
        Ok(loaded) => loaded,
        Err(error) => return fail(&error),
    };
    let _guard = guard;
    match nitidus::run(loaded) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => fail(&error),
    }
}

fn fail(error: &anyhow::Error) -> ExitCode {
    eprintln!("nitidus: {error:#}");
    ExitCode::FAILURE
}
