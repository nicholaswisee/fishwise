use super::{diagnostics, Classification, Diagnostic};

pub fn parse(source: &str) -> Result<tree_sitter::Tree, Vec<Diagnostic>> {
    if source.as_bytes().contains(&0) {
        return Err(vec![diagnostics::at(
            source,
            source.find('\0').unwrap(),
            source.find('\0').unwrap() + 1,
            Classification::Unsupported,
            "NUL byte is not valid translation input",
        )]);
    }
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_bash::LANGUAGE.into())
        .expect("compatible tree-sitter-bash language");
    let tree = parser.parse(source, None).expect("parser returns tree");
    let mut errors = Vec::new();
    fn visit(node: tree_sitter::Node<'_>, source: &str, errors: &mut Vec<Diagnostic>) {
        if node.is_error() || node.is_missing() {
            let start = if node.kind() == "ERROR"
                && node.start_byte() == 0
                && node.end_byte() == source.len()
            {
                source.len()
            } else {
                node.start_byte()
            };
            errors.push(diagnostics::at(
                source,
                start,
                node.end_byte(),
                Classification::Unsupported,
                format!("malformed Bash syntax ({})", node.kind()),
            ));
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            visit(child, source, errors);
        }
    }
    visit(tree.root_node(), source, &mut errors);
    if errors.is_empty() {
        let root = tree.root_node();
        validate_gap(source, 0, root.start_byte(), &mut errors);
        validate_owned_bytes(root, source, &mut errors);
        validate_gap(source, root.end_byte(), source.len(), &mut errors);
    }
    if errors.is_empty() {
        Ok(tree)
    } else {
        Err(errors)
    }
}

fn validate_gap(source: &str, start: usize, end: usize, errors: &mut Vec<Diagnostic>) {
    if let Some((offset, ch)) = source[start..end]
        .char_indices()
        .find(|(_, ch)| !matches!(ch, ' ' | '\t' | '\n'))
    {
        let start = start + offset;
        errors.push(diagnostics::at(
            source,
            start,
            start + ch.len_utf8(),
            Classification::Unsupported,
            "unsupported ignored Bash gap: only space, tab, and LF may separate syntax; quote data and continuations",
        ));
    }
}

fn validate_owned_bytes(node: tree_sitter::Node<'_>, source: &str, errors: &mut Vec<Diagnostic>) {
    // Data spans own their bytes; structural spans do not own trivia dropped by the lexer.
    if matches!(
        node.kind(),
        "word" | "number" | "raw_string" | "string" | "comment"
    ) {
        return;
    }
    if node.child_count() == 0 {
        if node.kind() == "program" {
            validate_gap(source, node.start_byte(), node.end_byte(), errors);
        }
        return;
    }
    let mut end = node.start_byte();
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        validate_gap(source, end, child.start_byte(), errors);
        validate_owned_bytes(child, source, errors);
        end = child.end_byte();
    }
    validate_gap(source, end, node.end_byte(), errors);
}

#[cfg(test)]
mod tests {
    #[test]
    fn unicode_gap_diagnostic_reports_one_full_character() {
        let source = " \t\u{2003}\n";
        let mut errors = Vec::new();
        super::validate_gap(source, 2, 5, &mut errors);
        assert_eq!(errors.len(), 1);
        let error = &errors[0];
        assert!(matches!(
            error.classification,
            super::Classification::Unsupported
        ));
        assert_eq!((error.span.start, error.span.end), (2, 5));
        assert_eq!((error.span.line, error.span.column), (1, 3));
        assert!(error.message.contains("ignored Bash gap"));
    }

    #[test]
    fn empty_program_and_root_exterior_trivia_are_checked() {
        for source in ["\r", "\u{b}", "\u{c}", " \t\r\n", "\recho hi", "echo hi\r"] {
            let errors = super::parse(source).unwrap_err();
            assert!(
                errors
                    .iter()
                    .any(|error| error.message.contains("ignored Bash gap")),
                "{source:?}"
            );
        }
        assert!(super::parse("echo \"unfinished")
            .unwrap_err()
            .iter()
            .any(|error| error.message.contains("malformed Bash syntax")));
    }
}
