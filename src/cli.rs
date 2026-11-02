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
        let mut source = None;
        let mut output = None;
        let (mut force, mut check, mut explain, mut strict, mut validate) =
            (false, false, false, false, false);
        let mut rest = args[1..].iter();
        while let Some(arg) = rest.next() {
            if arg == OsStr::new("--") {
                if source.is_some() {
                    return Err("translation accepts only one input source".into());
                }
                let Some(script) = rest.next() else {
                    return Err("expected a script after --".into());
                };
                if rest.next().is_some() {
                    return Err("translation accepts one script argument".into());
                }
                source = Some(crate::translate::Source::Script(script.clone()));
                break;
            }
            let flag = match arg.to_str() {
                Some(flag) => flag,
                None => {
                    use std::os::unix::ffi::OsStrExt;
                    if arg.as_bytes().first() == Some(&b'-') {
                        return Err("invalid translation option".into());
                    }
                    if source.is_some() {
                        return Err("translation accepts only one input source".into());
                    }
                    source = Some(crate::translate::Source::Script(arg.clone()));
                    continue;
                }
            };
            match flag {
                "--file" => {
                    if source.is_some() {
                        return Err("translation accepts only one input source".into());
                    }
                    let path = rest.next().ok_or("--file requires a path")?;
                    source = Some(crate::translate::Source::File(path.into()));
                }
                "--output" | "-o" => {
                    if output.is_some() {
                        return Err("--output may be specified only once".into());
                    }
                    output = Some(rest.next().ok_or("--output requires a path")?.into());
                }
                "--force" => {
                    if std::mem::replace(&mut force, true) {
                        return Err("--force may be specified only once".into());
                    }
                }
                "--check" => {
                    if std::mem::replace(&mut check, true) {
                        return Err("--check may be specified only once".into());
                    }
                }
                "--explain" => {
                    if std::mem::replace(&mut explain, true) {
                        return Err("--explain may be specified only once".into());
                    }
                }
                "--strict" => {
                    if std::mem::replace(&mut strict, true) {
                        return Err("--strict may be specified only once".into());
                    }
                }
                "--validate" => {
                    if std::mem::replace(&mut validate, true) {
                        return Err("--validate may be specified only once".into());
                    }
                }
                _ if flag.starts_with('-') => {
                    return Err(format!("unknown translation option: {flag}"))
                }
                _ => {
                    if source.is_some() {
                        return Err("translation accepts only one input source".into());
                    }
                    source = Some(crate::translate::Source::Script(arg.clone()));
                }
            }
        }
        if force && output.is_none() {
            return Err("--force requires --output".into());
        }
        if check && output.is_some() {
            return Err("--check cannot be combined with --output".into());
        }
        return Ok(Invocation::Translate(crate::translate::TranslateArgs {
            source: source.unwrap_or(crate::translate::Source::Stdin),
            output,
            force,
            check,
            explain,
            strict,
            validate,
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
