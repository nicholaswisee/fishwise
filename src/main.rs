#[cfg(unix)]
mod cli;
#[cfg(unix)]
mod runner;
#[cfg(unix)]
mod translate;

#[cfg(unix)]
fn main() {
    match cli::parse(std::env::args_os().skip(1).collect()) {
        Ok(cli::Invocation::Help) => println!(
            "Usage: fws [-h|--help] [--] 'script'\n       fws --translate [script]\nRun a Bash script, or translate supported Bash into fish."
        ),
        Ok(cli::Invocation::Run(script)) => runner::run(script),
        Ok(cli::Invocation::Translate(args)) => std::process::exit(translate::run(args)),
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
