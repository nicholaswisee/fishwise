use std::ffi::OsString;

#[cfg(unix)]
pub fn run(script: Option<OsString>) -> ! {
    use std::io::{self, IsTerminal, Read, Write};
    use std::os::unix::ffi::OsStringExt;
    use std::os::unix::process::CommandExt;
    use std::process::Command;

    let script = match script {
        Some(script) => script,
        None => {
            let stdin = io::stdin();
            if stdin.is_terminal() {
                eprint!("Paste Bash script, then press Ctrl-D to run: ");
                let _ = io::stderr().flush();
            }
            let mut script = Vec::new();
            if let Err(error) = stdin.lock().read_to_end(&mut script) {
                eprintln!("fws: failed to read script from stdin: {error}");
                std::process::exit(1);
            }
            OsString::from_vec(script)
        }
    };

    let error = Command::new("bash")
        .arg("-c")
        .arg("--")
        .arg(script)
        .arg("fws")
        .exec();
    eprintln!("fws: could not execute bash: {error}");
    std::process::exit(if error.kind() == io::ErrorKind::NotFound {
        127
    } else {
        126
    });
}

#[cfg(not(unix))]
pub fn run(_script: Option<OsString>) -> ! {
    eprintln!("fws: this program currently supports Unix systems only");
    std::process::exit(1);
}
