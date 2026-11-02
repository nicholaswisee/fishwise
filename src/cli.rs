use std::ffi::{OsStr, OsString};

pub enum Invocation {
    Help,
    Run(Option<OsString>),
    Translate(crate::translate::TranslateArgs),
}

pub fn parse(args: Vec<OsString>) -> Result<Invocation, String> {
    if args.len() == 1 && (args[0] == OsStr::new("-h") || args[0] == OsStr::new("--help")) {
        return Ok(Invocation::Help);
    }

    if args
        .first()
        .is_some_and(|arg| arg == OsStr::new("--translate") || arg == OsStr::new("-t"))
    {
        let rest = &args[1..];
        if rest.first().is_some_and(|arg| arg == OsStr::new("--")) {
            if rest.len() != 2 {
                return Err("translate mode accepts one script after --".into());
            }
            return Ok(Invocation::Translate(crate::translate::TranslateArgs {
                source: crate::translate::Source::Script(rest[1].clone()),
                output: None,
                force: false,
                check: false,
                explain: false,
                strict: false,
                validate: false,
            }));
        }
        if rest.len() > 1
            || rest
                .first()
                .is_some_and(|arg| arg.to_string_lossy().starts_with('-'))
        {
            return Err("translate mode accepts at most one script argument; additional options are not implemented yet".into());
        }
        return Ok(Invocation::Translate(crate::translate::TranslateArgs {
            source: rest.first().cloned().map_or(
                crate::translate::Source::Stdin,
                crate::translate::Source::Script,
            ),
            output: None,
            force: false,
            check: false,
            explain: false,
            strict: false,
            validate: false,
        }));
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
