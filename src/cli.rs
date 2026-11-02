use std::ffi::{OsStr, OsString};

pub enum Invocation {
    Help,
    Run(Option<OsString>),
}

pub fn parse(args: Vec<OsString>) -> Result<Invocation, String> {
    if args.len() == 1 && (args[0] == OsStr::new("-h") || args[0] == OsStr::new("--help")) {
        return Ok(Invocation::Help);
    }

    match args.as_slice() {
        [] => Ok(Invocation::Run(None)),
        [separator, script] if separator == OsStr::new("--") => {
            Ok(Invocation::Run(Some(script.clone())))
        }
        [script] if script != OsStr::new("--") => Ok(Invocation::Run(Some(script.clone()))),
        _ => Err(
            "expected one complete script argument; quote it as one argument or paste it on stdin"
                .into(),
        ),
    }
}
