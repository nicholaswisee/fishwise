mod diagnostics;
mod emit;
mod parser;

use std::ffi::OsString;
use std::io::Read;

#[derive(Clone, Debug)]
#[allow(dead_code)]
pub enum Source {
    Stdin,
    Script(OsString),
    File(std::path::PathBuf),
}
#[derive(Clone, Debug)]
pub struct TranslateArgs {
    pub source: Source,
    pub output: Option<std::path::PathBuf>,
    pub force: bool,
    pub check: bool,
    pub explain: bool,
    pub strict: bool,
    pub validate: bool,
}
#[derive(Clone, Debug)]
pub struct Translation {
    pub code: String,
    pub diagnostics: Vec<Diagnostic>,
}
#[derive(Clone, Debug)]
pub struct Diagnostic {
    pub classification: Classification,
    pub span: Span,
    pub message: String,
}
#[derive(Clone, Copy, Debug)]
#[allow(dead_code)]
pub enum Classification {
    Equivalent,
    Approximate,
    Unsupported,
}
#[derive(Clone, Copy, Debug)]
#[allow(dead_code)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub line: usize,
    pub column: usize,
}

pub fn translate(source: &str) -> Result<Translation, Vec<Diagnostic>> {
    let tree = parser::parse(source)?;
    emit::emit(source, &tree)
}

pub fn run(args: TranslateArgs) -> i32 {
    if args.output.is_some()
        || args.force
        || args.check
        || args.explain
        || args.strict
        || args.validate
    {
        eprintln!("fws: output/check/explain/strict/validate options are not implemented yet");
        return 2;
    }
    let input = match args.source {
        Source::Script(script) => match script.into_string() {
            Ok(s) => s,
            Err(_) => {
                eprintln!("fws: translation input is not valid UTF-8");
                return 1;
            }
        },
        Source::Stdin => {
            let mut bytes = Vec::new();
            if let Err(e) = std::io::stdin().read_to_end(&mut bytes) {
                eprintln!("fws: {e}");
                return 1;
            }
            match String::from_utf8(bytes) {
                Ok(s) => s,
                Err(_) => {
                    eprintln!("fws: translation input is not valid UTF-8");
                    return 1;
                }
            }
        }
        Source::File(_) => {
            eprintln!("fws: file input is not implemented yet");
            return 2;
        }
    };
    match translate(&input) {
        Ok(result) => {
            for d in result.diagnostics {
                let label = match d.classification {
                    Classification::Approximate => "warning",
                    Classification::Equivalent => "info",
                    Classification::Unsupported => "error",
                };
                eprintln!(
                    "fws:{}:{}: {label}: {}",
                    d.span.line, d.span.column, d.message,
                );
            }
            print!("{}", result.code);
            0
        }
        Err(errors) => {
            for d in errors {
                eprintln!("fws:{}:{}: {}", d.span.line, d.span.column, d.message);
            }
            1
        }
    }
}
