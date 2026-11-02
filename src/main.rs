#[cfg(unix)]
fn main() {
    use std::ffi::{OsStr, OsString};
    use std::io::{self, IsTerminal, Read, Write};
    use std::os::unix::ffi::OsStringExt;
    use std::os::unix::process::CommandExt;
    use std::process::Command;

    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    if args.len() == 1 && (args[0] == OsStr::new("-h") || args[0] == OsStr::new("--help")) {
        println!("Usage: fws [-h|--help] [--] 'script'\nRun a Bash script, or read the complete script from stdin when no script is given.");
        return;
    }

    let script = match args.as_slice() {
        [] => {
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
        [separator, script] if separator == OsStr::new("--") => script.clone(),
        [script] if script != OsStr::new("--") => script.clone(),
        _ => {
            eprintln!("fws: expected one complete script argument; quote it as one argument or paste it on stdin");
            std::process::exit(2);
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
fn main() {
    eprintln!("fws: this program currently supports Unix systems only");
    std::process::exit(1);
}
