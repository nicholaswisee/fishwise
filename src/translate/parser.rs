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
        Ok(tree)
    } else {
        Err(errors)
    }
}
