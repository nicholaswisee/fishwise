#[cfg(unix)]
mod cli;
#[cfg(unix)]
mod runner;

#[cfg(unix)]
fn main() {
    match cli::parse(std::env::args_os().skip(1).collect()) {
        Ok(cli::Invocation::Help) => println!(
            "Usage: fws [-h|--help] [--] 'script'\nRun a Bash script, or read the complete script from stdin when no script is given."
        ),
        Ok(cli::Invocation::Run(script)) => runner::run(script),
        Err(error) => {
            eprintln!("fws: {error}");
            std::process::exit(2);
        }
    }
}

#[cfg(not(unix))]
fn main() {
    eprintln!("fws: this program currently supports Unix systems only");
    std::process::exit(1);
}
