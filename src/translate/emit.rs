use std::collections::BTreeSet;

use super::{diagnostics, Classification, Diagnostic, Translation};
use tree_sitter::Node;

pub fn emit(source: &str, tree: &tree_sitter::Tree) -> Result<Translation, Vec<Diagnostic>> {
    let mut emitter = Emitter {
        source,
        code: String::new(),
        diagnostics: Vec::new(),
        errors: Vec::new(),
        depth: 0,
        in_function: false,
        bindings: Bindings::default(),
        functions: BTreeSet::new(),
    };
    fn collect(node: Node<'_>, source: &str, names: &mut BTreeSet<String>) {
        if node.kind() == "function_definition" {
            if let Some(name) = node.child_by_field_name("name") {
                names.insert(node_text(name, source).to_owned());
            }
        }
        for child in named(node) {
            collect(child, source, names);
        }
    }
    collect(tree.root_node(), source, &mut emitter.functions);
    emitter.statements(&children(tree.root_node()));
    if emitter.errors.is_empty() {
        Ok(Translation {
            code: emitter.code,
            diagnostics: emitter.diagnostics,
        })
    } else {
        Err(emitter.errors)
    }
}

#[derive(Clone, Default)]
struct Bindings {
    scalars: BTreeSet<String>,
    locals: BTreeSet<String>,
    possible_locals: BTreeSet<String>,
    echo_safe: BTreeSet<String>,
}

impl Bindings {
    fn join(arms: &[Self]) -> Self {
        let mut result = arms[0].clone();
        for arm in &arms[1..] {
            result.scalars.retain(|name| arm.scalars.contains(name));
            result.locals.retain(|name| arm.locals.contains(name));
            result
                .possible_locals
                .extend(arm.possible_locals.iter().cloned());
            result.echo_safe.retain(|name| arm.echo_safe.contains(name));
        }
        result
    }
}

struct Emitter<'a> {
    source: &'a str,
    code: String,
    diagnostics: Vec<Diagnostic>,
    errors: Vec<Diagnostic>,
    depth: usize,
    in_function: bool,
    bindings: Bindings,
    functions: BTreeSet<String>,
}

impl Emitter<'_> {
    // Prefix statements, not physical lines: newlines inside literal arguments are data.
    fn line(&mut self, text: &str) {
        self.code.push_str(&"    ".repeat(self.depth));
        self.code.push_str(text);
        self.code.push('\n');
    }

    fn report(&mut self, node: Node<'_>, classification: Classification, message: &str) {
        let d = diagnostics::at(
            self.source,
            node.start_byte(),
            node.end_byte(),
            classification,
            message,
        );
        if matches!(classification, Classification::Unsupported) {
            self.errors.push(d);
        } else {
            self.diagnostics.push(d);
        }
    }

    fn reject(&mut self, node: Node<'_>, message: &str) {
        self.report(node, Classification::Unsupported, message);
    }

    fn approximate(&mut self, node: Node<'_>, message: &str) {
        self.report(node, Classification::Approximate, message);
    }

    fn equivalent(&mut self, node: Node<'_>, message: &str) {
        self.report(node, Classification::Equivalent, message);
    }

    fn tokens(&mut self, node: Node<'_>, allowed: &[&str]) -> bool {
        let mut valid = true;
        for token in children(node).into_iter().filter(|n| !n.is_named()) {
            if !allowed.contains(&token.kind()) {
                self.reject(token, "unsupported Bash operator or token");
                valid = false;
            }
        }
        valid
    }

    fn statements(&mut self, nodes: &[Node<'_>]) {
        for &node in nodes {
            if node.is_named() {
                self.statement(node);
            } else if !matches!(node.kind(), ";" | "\n") {
                self.reject(node, "unsupported Bash statement separator");
            }
        }
    }

    fn header_comments(&mut self, nodes: &[Node<'_>]) {
        for &node in nodes.iter().filter(|n| n.kind() == "comment") {
            self.statement(node);
        }
    }

    fn statement(&mut self, node: Node<'_>) {
        match node.kind() {
            "comment" => {
                let text = node_text(node, self.source);
                if node.start_byte() == 0 && text.starts_with("#!") {
                    if matches!(
                        text.trim_end_matches([' ', '\t']),
                        "#!/bin/bash" | "#!/usr/bin/bash" | "#!/usr/bin/env bash"
                    ) {
                        self.line("#!/usr/bin/env fish");
                        self.equivalent(node, "replaced the leading Bash interpreter with fish");
                    } else {
                        self.reject(
                            node,
                            "unrecognized Bash shebang or unsupported interpreter options",
                        );
                    }
                } else {
                    if self.code.is_empty() && text.starts_with("#!") {
                        // A source comment must not become an interpreter directive after formatting.
                        self.code.push('\n');
                    }
                    self.line(text);
                }
            }
            "variable_assignment" => self.assignment(node, false, false),
            "declaration_command" => {
                let parts = children(node);
                if parts.len() != 2
                    || parts[1].kind() != "variable_assignment"
                    || !matches!(parts[0].kind(), "local" | "export")
                {
                    self.reject(
                        node,
                        "only local NAME=value and export NAME=value are supported",
                    );
                } else {
                    self.assignment(
                        parts[1],
                        parts[0].kind() == "export",
                        parts[0].kind() == "local",
                    );
                }
            }
            "unset_command" => self.unset(node),
            "command" => {
                if let Some(text) = self.command(node, false) {
                    self.line(&text);
                }
            }
            "test_command" => {
                if let Some(test) = self.condition(node) {
                    self.line(&test);
                }
            }
            "if_statement" => self.if_statement(node),
            "for_statement" => self.for_statement(node),
            "while_statement" => self.while_statement(node),
            "function_definition" => self.function(node),
            "pipeline" => self.pipeline(node),
            _ => self.reject(
                node,
                &format!("unsupported Bash construct ({})", node.kind()),
            ),
        }
    }

    fn body(&mut self, node: Node<'_>) {
        let parts = children(node);
        let delimiters = match node.kind() {
            "compound_statement" => ("{", "}"),
            "do_group" => ("do", "done"),
            _ => {
                self.reject(node, "unsupported block body");
                return;
            }
        };
        if parts.first().map(|n| n.kind()) != Some(delimiters.0)
            || parts.last().map(|n| n.kind()) != Some(delimiters.1)
        {
            self.reject(node, "malformed block body");
            return;
        }
        self.statements(&parts[1..parts.len() - 1]);
    }

    fn assignment(&mut self, node: Node<'_>, export: bool, local: bool) {
        if !self.tokens(node, &["="]) {
            return;
        }
        let (Some(name_node), Some(value_node)) = (
            node.child_by_field_name("name"),
            node.child_by_field_name("value"),
        ) else {
            self.reject(node, "assignment requires an ordinary scalar value");
            return;
        };
        let name = node_text(name_node, self.source);
        if name_node.kind() != "variable_name"
            || !ordinary_variable(name)
            || self.source[name_node.end_byte()..value_node.start_byte()].trim() != "="
        {
            self.reject(
                node,
                "only simple ordinary NAME=value assignments are supported",
            );
            return;
        }
        if local && !self.in_function {
            self.reject(node, "local is supported only inside a function");
            return;
        }
        if contains_status(value_node, self.source) {
            self.reject(
                value_node,
                "$? assignment values cannot be status-preserved safely",
            );
            return;
        }
        let Some(value) = self.word(value_node) else {
            return;
        };
        if !local
            && self.bindings.possible_locals.contains(name)
            && !self.bindings.locals.contains(name)
        {
            self.reject(
                node,
                "assignment scope depends on a conditional or loop-local declaration",
            );
            return;
        }
        let own_local = local || self.bindings.locals.contains(name);
        if self.in_function && !own_local {
            self.approximate(node, "function assignment may differ under a Bash caller-local binding; fish writes the global binding");
        }
        if local && !self.bindings.locals.contains(name) {
            self.approximate(node, "local inherits the visible export attribute; a Bash caller-local binding with different export attributes is not visible to fish");
            self.line(&format!("if set -qx {name}"));
            self.depth += 1;
            self.set_assignment("-fx", name, &value);
            self.depth -= 1;
            self.line("else");
            self.depth += 1;
            self.set_assignment("-fu", name, &value);
            self.depth -= 1;
            self.line("end");
        } else {
            let flag = match (own_local, export) {
                (true, true) => "-fx",
                (true, false) => "-f",
                (false, true) => "-gx",
                (false, false) => "-g",
            };
            self.set_assignment(flag, name, &value);
        }
        self.bindings.scalars.insert(name.to_owned());
        self.bindings.echo_safe.remove(name);
        if !has_expansion(value_node)
            && matches!(
                value_node.kind(),
                "word" | "raw_string" | "string" | "number"
            )
            && literal_prefix(value_node, self.source).is_some()
            && !literal_starts_with_dash(value_node, self.source)
        {
            self.bindings.echo_safe.insert(name.to_owned());
        }
        if local {
            self.bindings.locals.insert(name.to_owned());
            self.bindings.possible_locals.insert(name.to_owned());
        }
        self.equivalent(node, "scalar assignment uses set with explicit scope; builtin true before set preserves Bash assignment success status");
    }

    fn set_assignment(&mut self, flag: &str, name: &str, value: &str) {
        self.line("builtin true");
        self.line(&format!("set {flag} {name} {value}"));
    }

    fn unset(&mut self, node: Node<'_>) {
        if !self.tokens(node, &["unset"]) {
            return;
        }
        let args = named(node);
        if args.len() != 1
            || !ordinary_variable(node_text(args[0], self.source))
            || !matches!(args[0].kind(), "word" | "variable_name")
        {
            self.reject(
                node,
                "unset supports exactly one ordinary literal variable name",
            );
            return;
        }
        let name = node_text(args[0], self.source);
        if self.bindings.possible_locals.contains(name) && !self.bindings.locals.contains(name) {
            self.reject(
                node,
                "unset scope depends on a conditional or loop-local declaration",
            );
            return;
        }
        if self.bindings.locals.contains(name) {
            self.line("builtin true");
            self.line(&format!("set -f {name} ''"));
            self.approximate(node, "local unset keeps an empty shadow instead of exposing a global; unset-versus-empty and exported global visibility to child processes or subsequent assignments may differ");
        } else {
            self.approximate(node, "global unset assumes no hidden fish universal binding or Bash caller-local binding; erasing a global may expose a universal variable");
            self.line(&format!("if set -q -g {name}"));
            self.depth += 1;
            self.line(&format!("set -eg {name}"));
            self.depth -= 1;
            self.line("end");
            self.bindings.scalars.remove(name);
            self.bindings.echo_safe.remove(name);
        }
    }

    fn condition(&mut self, node: Node<'_>) -> Option<String> {
        if node.kind() == "command" && matches!(node_text(node, self.source), "true" | "false") {
            return self.command(node, false);
        }
        if node.kind() != "test_command" || !self.tokens(node, &["[", "]", "[[", "]]"]) {
            self.reject(
                node,
                "condition must be a supported test or no-argument true/false",
            );
            return None;
        }
        let parts = named(node);
        if parts.len() != 1 {
            self.reject(node, "unsupported test expression");
            return None;
        }
        let expr = parts[0];
        let Some(op_node) = expr.child_by_field_name("operator") else {
            self.reject(expr, "unsupported test expression");
            return None;
        };
        let op = node_text(op_node, self.source);
        let operands: Vec<_> = named(expr)
            .into_iter()
            .filter(|n| n.id() != op_node.id())
            .collect();
        let rendered = if expr.kind() == "unary_expression"
            && operands.len() == 1
            && matches!(op, "-n" | "-z" | "-f" | "-d" | "-e" | "-r" | "-w" | "-x")
        {
            format!("test {op} {}", self.word(operands[0])?)
        } else if expr.kind() == "binary_expression"
            && operands.len() == 2
            && matches!(
                op,
                "=" | "==" | "!=" | "-eq" | "-ne" | "-lt" | "-le" | "-gt" | "-ge"
            )
        {
            if op.starts_with('-')
                && operands
                    .iter()
                    .any(|n| !canonical_integer(node_text(*n, self.source)))
            {
                self.reject(expr, "numeric tests require canonical small decimal literals, not Bash arithmetic expressions or leading zeros");
                return None;
            }
            let lhs = self.word(operands[0])?;
            let rhs = self.word(operands[1])?;
            format!("test {lhs} {} {rhs}", if op == "==" { "=" } else { op })
        } else {
            self.reject(
                expr,
                "unsupported test operator, pattern, regex, or compound expression",
            );
            return None;
        };
        self.equivalent(
            node,
            "supported Bash test becomes fish test with explicit operands",
        );
        Some(rendered)
    }

    fn one_condition(&mut self, nodes: &[Node<'_>]) -> Option<String> {
        let statements: Vec<_> = nodes
            .iter()
            .copied()
            .filter(|n| n.is_named() && n.kind() != "comment")
            .collect();
        if statements.len() != 1
            || nodes
                .iter()
                .any(|n| !n.is_named() && !matches!(n.kind(), ";" | "\n"))
        {
            if let Some(node) = nodes.first() {
                self.reject(
                    *node,
                    "condition requires exactly one foreground test or true/false command",
                );
            }
            return None;
        }
        self.condition(statements[0])
    }

    fn if_statement(&mut self, node: Node<'_>) {
        if !self.tokens(node, &["if", "then", "fi", ";", "\n"]) {
            return;
        }
        let parts = children(node);
        let conditions: Vec<_> = (0..node.child_count())
            .filter(|i| node.field_name_for_child(*i as u32) == Some("condition"))
            .map(|i| node.child(i).unwrap())
            .collect();
        let Some(test) = self.one_condition(&conditions) else {
            return;
        };
        let Some(then) = parts.iter().position(|n| n.kind() == "then") else {
            self.reject(node, "missing then");
            return;
        };
        self.header_comments(&parts[..then]);
        self.line(&format!("if {test}"));
        let incoming = self.bindings.clone();
        let mut arms = Vec::new();
        let end = parts
            .iter()
            .position(|n| matches!(n.kind(), "elif_clause" | "else_clause" | "fi"))
            .unwrap_or(parts.len());
        self.depth += 1;
        self.statements(&parts[then + 1..end]);
        self.depth -= 1;
        arms.push(self.bindings.clone());
        let mut has_else = false;
        for clause in named(node)
            .into_iter()
            .filter(|n| matches!(n.kind(), "elif_clause" | "else_clause"))
        {
            self.bindings = incoming.clone();
            let tokens = children(clause);
            if !self.tokens(clause, &["elif", "else", "then", ";", "\n"]) {
                continue;
            }
            let body_start = if clause.kind() == "elif_clause" {
                let Some(then) = tokens.iter().position(|n| n.kind() == "then") else {
                    self.reject(clause, "missing elif then");
                    continue;
                };
                let Some(test) = self.one_condition(&tokens[1..then]) else {
                    continue;
                };
                self.header_comments(&tokens[1..then]);
                self.line(&format!("else if {test}"));
                then + 1
            } else {
                has_else = true;
                self.line("else");
                1
            };
            self.depth += 1;
            self.statements(&tokens[body_start..]);
            self.depth -= 1;
            arms.push(self.bindings.clone());
        }
        if !has_else {
            arms.push(incoming);
        }
        self.bindings = Bindings::join(&arms);
        self.line("end");
        self.equivalent(
            node,
            "if/elif/else becomes a nested fish block; binding facts are joined across all paths",
        );
    }

    fn for_statement(&mut self, node: Node<'_>) {
        if !self.tokens(node, &["for", "in", ";", "\n"]) {
            return;
        }
        let parts = children(node);
        if parts.first().map(|n| n.kind()) != Some("for") || !parts.iter().any(|n| n.kind() == "in")
        {
            self.reject(node, "only for with an explicit literal list is supported; select and positional loops are unsupported");
            return;
        }
        let (Some(var), Some(body)) = (
            node.child_by_field_name("variable"),
            node.child_by_field_name("body"),
        ) else {
            self.reject(node, "malformed for loop");
            return;
        };
        let name = node_text(var, self.source);
        if !ordinary_variable(name) {
            self.reject(var, "loop variable must be an ordinary variable name");
            return;
        }
        let values: Vec<_> = (0..node.child_count())
            .filter(|i| node.field_name_for_child(*i as u32) == Some("value"))
            .map(|i| node.child(i).unwrap())
            .collect();
        if named(node).iter().any(|n| {
            n.id() != var.id()
                && n.id() != body.id()
                && n.kind() != "comment"
                && !values.iter().any(|v| v.id() == n.id())
        }) {
            self.reject(node, "unsupported for-loop header child");
            return;
        }
        if values.iter().any(|n| {
            !matches!(n.kind(), "word" | "raw_string" | "string" | "number") || has_expansion(*n)
        }) {
            self.reject(node, "for loop requires an explicit literal word list");
            return;
        }
        let rendered: Option<Vec<_>> = values.into_iter().map(|n| self.word(n)).collect();
        let Some(rendered) = rendered else {
            return;
        };
        self.approximate(node, "Bash loop-variable lifetime and updates to an existing binding may differ from fish block scope");
        self.header_comments(&parts);
        self.line(&format!("for {name} in {}", rendered.join(" ")));
        let incoming = self.bindings.clone();
        self.bindings.scalars.insert(name.to_owned());
        self.depth += 1;
        self.body(body);
        self.depth -= 1;
        self.bindings = Bindings::join(&[incoming, self.bindings.clone()]);
        self.line("end");
    }

    fn while_statement(&mut self, node: Node<'_>) {
        if !self.tokens(node, &["while", ";", "\n"]) {
            return;
        }
        let conditions: Vec<_> = (0..node.child_count())
            .filter(|i| node.field_name_for_child(*i as u32) == Some("condition"))
            .map(|i| node.child(i).unwrap())
            .collect();
        let Some(test) = self.one_condition(&conditions) else {
            return;
        };
        let Some(body) = node.child_by_field_name("body") else {
            self.reject(node, "missing loop body");
            return;
        };
        if named(node).iter().any(|n| {
            n.id() != body.id()
                && n.kind() != "comment"
                && !conditions.iter().any(|c| c.id() == n.id())
        }) {
            self.reject(node, "unsupported while-loop header child");
            return;
        }
        self.header_comments(&children(node));
        self.line(&format!("while {test}"));
        let incoming = self.bindings.clone();
        self.depth += 1;
        self.body(body);
        self.depth -= 1;
        self.bindings = Bindings::join(&[incoming, self.bindings.clone()]);
        self.line("end");
        self.equivalent(
            node,
            "while uses the supported condition; bindings account for zero iterations",
        );
    }

    fn function(&mut self, node: Node<'_>) {
        if self.in_function {
            self.reject(
                node,
                "nested function definitions have unresolved binding lifetime",
            );
            return;
        }
        if !self.tokens(node, &["function", "(", ")"]) {
            return;
        }
        if node.child_by_field_name("redirect").is_some() {
            self.reject(node, "function redirections are unsupported");
            return;
        }
        let (Some(name), Some(body)) = (
            node.child_by_field_name("name"),
            node.child_by_field_name("body"),
        ) else {
            self.reject(node, "malformed function");
            return;
        };
        let text = node_text(name, self.source);
        if !ordinary_variable(text) || shell_builtin(text) {
            self.reject(
                name,
                "function name must be an ordinary non-special identifier",
            );
            return;
        }
        if body.kind() != "compound_statement" {
            self.reject(body, "only brace-delimited function bodies are supported");
            return;
        }
        if named(node)
            .iter()
            .any(|n| n.id() != name.id() && n.id() != body.id() && n.kind() != "comment")
        {
            self.reject(node, "unsupported function child or redirect");
            return;
        }
        self.header_comments(&children(node));
        self.line(&format!("function {text}"));
        let incoming = self.bindings.clone();
        self.bindings = Bindings::default();
        self.in_function = true;
        self.depth += 1;
        self.body(body);
        self.depth -= 1;
        self.in_function = false;
        self.bindings = incoming;
        self.line("end");
        self.equivalent(
            node,
            "function braces become function/end; local declarations use fish function scope",
        );
    }

    fn pipeline(&mut self, node: Node<'_>) {
        if !self.tokens(node, &["|"]) {
            return;
        }
        let mut stages = Vec::new();
        for stage in named(node) {
            if stage.kind() != "command" {
                self.reject(stage, "pipeline stages must be stateless commands");
                return;
            }
            let Some(text) = self.command(stage, true) else {
                return;
            };
            stages.push(text);
        }
        self.line(&stages.join(" | "));
        self.approximate(node, "pipeline uses last-stage status (Bash pipefail must be off); external stages must not resolve to ambient stateful shell functions");
    }

    fn word(&mut self, node: Node<'_>) -> Option<String> {
        match node.kind() {
            "simple_expansion" => {
                if node_text(node, self.source) == "$?" {
                    self.equivalent(
                        node,
                        "Bash $? becomes fish $status without intervening commands",
                    );
                    Some("$status".into())
                } else {
                    self.reject(
                        node,
                        "unquoted variable expansion may split or glob; quote it explicitly",
                    );
                    None
                }
            }
            "word" => {
                let text = node_text(node, self.source);
                if !text.is_empty()
                    && text
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"_./:@%+=,-".contains(&b))
                    && !text.starts_with('~')
                {
                    Some(text.to_owned())
                } else {
                    self.reject(node, "unquoted shell word may expand, split, or glob");
                    None
                }
            }
            "number" => {
                let text = node_text(node, self.source);
                let digits = text
                    .strip_prefix('-')
                    .or_else(|| text.strip_prefix('+'))
                    .unwrap_or(text);
                if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) {
                    Some(text.to_owned())
                } else {
                    self.reject(node, "only decimal integer literals are supported");
                    None
                }
            }
            "raw_string" => {
                let text = node_text(node, self.source);
                Some(single_quote(&text[1..text.len() - 1]))
            }
            "string" => {
                if has_expansion(node) && node_text(node, self.source).contains("\\\n") {
                    self.reject(node, "quoted line continuations adjacent to expansions have unsupported Bash tokenization");
                    return None;
                }
                let mut parts = Vec::new();
                let mut literal_start = node.start_byte() + 1;
                for child in named(node) {
                    match child.kind() {
                        "string_content" => {}
                        "simple_expansion" => {
                            let vars = named(child);
                            if vars.len() != 1 {
                                self.reject(child, "unsupported variable expansion");
                                return None;
                            }
                            let var = node_text(vars[0], self.source);
                            // The Bash grammar can include preceding whitespace in the '$' token.
                            let dollar = vars[0].start_byte() - 1;
                            if self.source.as_bytes()[dollar] != b'$' {
                                self.reject(child, "unsupported variable token layout");
                                return None;
                            }
                            parts.push(escape_double(&self.source[literal_start..dollar]));
                            literal_start = vars[0].end_byte();
                            if var == "?" {
                                parts.push("$status".to_owned());
                                self.equivalent(
                                    child,
                                    "Bash $? becomes fish $status without intervening commands",
                                );
                            } else {
                                if vars[0].kind() != "variable_name" || !ordinary_variable(var) {
                                    self.reject(
                                        child,
                                        "special or positional variable semantics are unsupported",
                                    );
                                    return None;
                                }
                                if !self.bindings.scalars.contains(var) {
                                    self.approximate(child, "quoted Bash scalar expansion assumes an ordinary ambient fish scalar, not a list or universal binding");
                                }
                                if self.in_function && !self.bindings.locals.contains(var) {
                                    self.approximate(child, "Bash may resolve this function variable from a caller-local binding; fish functions do not");
                                }
                                parts.push(format!("${var}"));
                            }
                            if self.source.as_bytes().get(literal_start).is_some_and(|b| {
                                b.is_ascii_alphanumeric() || matches!(b, b'_' | b'[')
                            }) {
                                parts.push("\"\"".to_owned());
                            }
                        }
                        _ => {
                            self.reject(child, "unsupported quoted expansion");
                            return None;
                        }
                    }
                }
                parts.push(escape_double(
                    &self.source[literal_start..node.end_byte() - 1],
                ));
                Some(format!("\"{}\"", parts.join("")))
            }
            _ => {
                self.reject(node, "unsupported word syntax");
                None
            }
        }
    }

    fn command(&mut self, node: Node<'_>, pipeline: bool) -> Option<String> {
        if !self.tokens(node, &[]) {
            return None;
        }
        if node.child_by_field_name("redirect").is_some() {
            self.reject(node, "redirections are not supported");
            return None;
        }
        let Some(name) = node.child_by_field_name("name") else {
            self.reject(node, "missing command name");
            return None;
        };
        let name_parts = named(name);
        if name_parts.len() != 1 || name_parts[0].kind() != "word" {
            self.reject(name, "command name must be a plain literal word");
            return None;
        }
        let cmd = node_text(name_parts[0], self.source);
        if cmd.is_empty()
            || !cmd
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_./-".contains(&b))
            || cmd.starts_with('-')
        {
            self.reject(name, "command name must be a plain literal word");
            return None;
        }
        if shell_builtin(cmd) && !matches!(cmd, "echo" | "printf" | "true" | "false") {
            self.reject(
                node,
                "ambiguous or unsupported shell builtin in command position",
            );
            return None;
        }
        if pipeline && self.functions.contains(cmd) {
            self.reject(node, "function calls in pipelines may mutate shell state");
            return None;
        }
        let mut rendered = vec![cmd.to_owned()];
        let args: Vec<_> = named(node)
            .into_iter()
            .filter(|n| n.id() != name.id())
            .collect();
        if matches!(cmd, "true" | "false") && !args.is_empty() {
            self.reject(node, "true and false are supported only without arguments");
            return None;
        }
        if cmd == "echo"
            && args
                .iter()
                .any(|n| literal_starts_with_dash(*n, self.source))
        {
            self.reject(
                node,
                "echo options and escape-sensitive forms are not supported",
            );
            return None;
        }
        if cmd == "echo" {
            for arg in &args {
                if has_unproven_echo_expansion(*arg, self.source, &self.bindings) {
                    self.approximate(
                        *arg,
                        "echo argument may be interpreted as a fish option when its expanded value starts with a dash",
                    );
                }
            }
        }
        if cmd == "printf"
            && (args.is_empty()
                || !matches!(
                    node_text(args[0], self.source),
                    "'%s'" | "'%s\\n'" | "\"%s\"" | "\"%s\\n\""
                ))
        {
            self.reject(node, "printf supports only tested %s and %s\\n formats");
            return None;
        }
        for arg in args {
            rendered.push(self.word(arg)?);
        }
        Some(rendered.join(" "))
    }
}

fn node_text<'a>(node: Node<'_>, source: &'a str) -> &'a str {
    &source[node.byte_range()]
}
fn children(node: Node<'_>) -> Vec<Node<'_>> {
    (0..node.child_count())
        .map(|i| node.child(i).unwrap())
        .collect()
}
fn named(node: Node<'_>) -> Vec<Node<'_>> {
    children(node)
        .into_iter()
        .filter(|n| n.is_named())
        .collect()
}
fn has_expansion(node: Node<'_>) -> bool {
    node.kind().contains("expansion") || named(node).into_iter().any(has_expansion)
}
fn has_unproven_echo_expansion(node: Node<'_>, source: &str, bindings: &Bindings) -> bool {
    (node.kind() == "simple_expansion"
        && named(node).iter().any(|n| {
            let name = node_text(*n, source);
            name != "?" && !bindings.echo_safe.contains(name)
        }))
        || named(node)
            .into_iter()
            .any(|n| has_unproven_echo_expansion(n, source, bindings))
}
fn contains_status(node: Node<'_>, source: &str) -> bool {
    (node.kind() == "simple_expansion" && named(node).iter().any(|n| node_text(*n, source) == "?"))
        || named(node).into_iter().any(|n| contains_status(n, source))
}
fn ordinary_variable(name: &str) -> bool {
    name.as_bytes()
        .first()
        .is_some_and(|b| b.is_ascii_alphabetic() || *b == b'_')
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
        && !special_variable(name)
}
fn canonical_integer(text: &str) -> bool {
    text.parse::<i32>().is_ok_and(|n| n.to_string() == text)
}
fn single_quote(text: &str) -> String {
    format!("'{}'", text.replace('\\', "\\\\").replace('\'', "\\'"))
}
fn literal_starts_with_dash(node: Node<'_>, source: &str) -> bool {
    literal_prefix(node, source) == Some('-')
}
fn literal_prefix(node: Node<'_>, source: &str) -> Option<char> {
    let text = node_text(node, source);
    match node.kind() {
        // Reuse literal rendering: Bash removes quoted backslash-newline before echo sees it.
        "string" => escape_double(&text[1..text.len() - 1]).chars().next(),
        "raw_string" => text[1..text.len() - 1].chars().next(),
        _ => text.chars().next(),
    }
}
fn escape_double(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\\' && chars.peek() == Some(&'\n') {
            chars.next();
            continue;
        }
        if ch == '\\' && chars.peek() == Some(&'`') {
            chars.next();
            out.push('`');
            continue;
        }
        if ch == '\\'
            && chars
                .peek()
                .is_some_and(|c| matches!(c, '$' | '`' | '\\' | '"'))
        {
            out.push('\\');
            out.push(chars.next().unwrap());
        } else {
            if matches!(ch, '\\' | '"' | '$') {
                out.push('\\');
            }
            out.push(ch);
        }
    }
    out
}

fn special_variable(name: &str) -> bool {
    name.starts_with("BASH")
        || name.starts_with("fish_")
        || name.starts_with("COMP_")
        || name.starts_with("EPOCH")
        || matches!(
            name,
            "PATH"
                | "PWD"
                | "OLDPWD"
                | "IFS"
                | "CDPATH"
                | "ENV"
                | "SHELL"
                | "RANDOM"
                | "SRANDOM"
                | "_"
                | "SECONDS"
                | "LINENO"
                | "UID"
                | "EUID"
                | "PPID"
                | "BASHPID"
                | "BASHOPTS"
                | "SHELLOPTS"
                | "FUNCNAME"
                | "GROUPS"
                | "DIRSTACK"
                | "PIPESTATUS"
                | "MACHTYPE"
                | "HOSTTYPE"
                | "OSTYPE"
                | "OPTARG"
                | "OPTIND"
                | "HISTCMD"
                | "SHLVL"
                | "status"
                | "history"
                | "version"
                | "hostname"
                | "status_generation"
                | "CMD_DURATION"
                | "last_pid"
                | "COLUMNS"
                | "LINES"
                | "argv"
                | "pipestatus"
                | "fish_pid"
                | "fish_version"
                | "fish_user_paths"
        )
}

fn shell_builtin(name: &str) -> bool {
    matches!(
        name,
        "!" | "."
            | ":"
            | "_"
            | "["
            | "abbr"
            | "alias"
            | "argparse"
            | "bg"
            | "bind"
            | "block"
            | "break"
            | "builtin"
            | "caller"
            | "case"
            | "cd"
            | "command"
            | "commandline"
            | "compgen"
            | "complete"
            | "compopt"
            | "continue"
            | "contains"
            | "contains_seq"
            | "count"
            | "declare"
            | "dirh"
            | "dirs"
            | "disown"
            | "echo"
            | "emit"
            | "enable"
            | "eval"
            | "exec"
            | "exit"
            | "export"
            | "false"
            | "fc"
            | "fg"
            | "fish_add_path"
            | "fish_breakpoint"
            | "fish_config"
            | "fish_indent"
            | "fish_key_reader"
            | "fish_opt"
            | "fish_status_to_signal"
            | "fish_update_completions"
            | "functions"
            | "getopts"
            | "hash"
            | "help"
            | "history"
            | "isatty"
            | "jobs"
            | "kill"
            | "let"
            | "local"
            | "logout"
            | "math"
            | "mapfile"
            | "nextd"
            | "not"
            | "open"
            | "or"
            | "path"
            | "popd"
            | "prevd"
            | "printf"
            | "prompt_pwd"
            | "psub"
            | "pushd"
            | "pwd"
            | "random"
            | "read"
            | "readarray"
            | "readonly"
            | "realpath"
            | "return"
            | "set"
            | "set_color"
            | "shift"
            | "shopt"
            | "source"
            | "status"
            | "string"
            | "suspend"
            | "switch"
            | "test"
            | "time"
            | "times"
            | "trap"
            | "true"
            | "type"
            | "typeset"
            | "ulimit"
            | "umask"
            | "unalias"
            | "unset"
            | "wait"
            | "while"
            | "for"
            | "function"
            | "if"
            | "begin"
            | "breakpoint"
            | "end"
            | "else"
            | "and"
    )
}
