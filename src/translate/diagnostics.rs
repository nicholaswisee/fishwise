use super::{Classification, Diagnostic, Span};

pub fn at(
    source: &str,
    start: usize,
    end: usize,
    classification: Classification,
    message: impl Into<String>,
) -> Diagnostic {
    let before = &source[..start.min(source.len())];
    let line = before.bytes().filter(|b| *b == b'\n').count() + 1;
    let column = before.rsplit('\n').next().unwrap_or("").len() + 1;
    Diagnostic {
        classification,
        span: Span {
            start,
            end,
            line,
            column,
        },
        message: message.into(),
    }
}
