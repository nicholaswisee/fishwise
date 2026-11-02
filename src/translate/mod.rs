mod diagnostics;
mod emit;
pub mod io;
mod parser;
pub mod validate;

use std::ffi::OsString;
use std::io::Write;

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
    let input = match io::read_source(&args.source) {
        Ok(input) => input,
        Err(e) => {
            eprintln!("fws: {e}");
            return 1;
        }
    };
    match translate(&input) {
        Ok(result) => {
            for d in &result.diagnostics {
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
            if result
                .diagnostics
                .iter()
                .any(|d| matches!(d.classification, Classification::Unsupported))
            {
                return 1;
            }
            if args.explain {
                let count = result
                    .diagnostics
                    .iter()
                    .filter(|d| matches!(d.classification, Classification::Equivalent))
                    .count();
                if count == 0 {
                    eprintln!(
                        "fws: explanation: translated supported Bash constructs to fish syntax"
                    );
                } else {
                    eprintln!(
                        "fws: explanation: {count} equivalent transformation(s) reported above"
                    );
                }
            }
            if args.strict
                && result
                    .diagnostics
                    .iter()
                    .any(|d| matches!(d.classification, Classification::Approximate))
            {
                eprintln!("fws: strict mode rejects approximate translations");
                return 1;
            }
            if args.validate {
                if let Err(e) = validate::syntax_check(&result.code) {
                    eprintln!("fws: {e}");
                    return 1;
                }
            }
            if args.check {
                return 0;
            }
            if let Some(output) = args.output {
                if let Err(e) = io::reject_same_file(&args.source, &output) {
                    eprintln!("fws: {e}");
                    return 1;
                }
                if let Err(e) = io::publish(&output, result.code.as_bytes(), args.force) {
                    eprintln!("fws: failed to publish {}: {e}", output.display());
                    return 1;
                }
            } else if let Err(e) = std::io::stdout().lock().write_all(result.code.as_bytes()) {
                eprintln!("fws: failed to write generated code: {e}");
                return 1;
            }
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
