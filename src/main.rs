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
            "Usage: fws [-h|--help] [--] 'script'\n       fws --translate [script | --file PATH] [--output|-o PATH] [--force]\n                    [--check] [--explain] [--strict] [--validate]\n\nRun mode (default) executes one Bash script; with no script, read stdin to EOF.\nTranslation never executes input; without script or --file, translate stdin to EOF.\n--file reads a file; --output writes code (existing files need --force).\n--check analyzes without code output; --explain shows transformations and assumptions.\n--strict rejects approximate translations; --validate runs fish -n (syntax only).\nUse -- before a script beginning with a dash. --help describes both modes."
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
