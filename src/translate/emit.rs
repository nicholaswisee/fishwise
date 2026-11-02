use super::{diagnostics, Classification, Diagnostic, Translation};

pub fn emit(source: &str, tree: &tree_sitter::Tree) -> Result<Translation, Vec<Diagnostic>> {
    let root = tree.root_node();
    let mut code = String::new();
    let mut warnings = Vec::new();
    let mut errors = Vec::new();
    for child_index in 0..root.child_count() {
        let node = root.child(child_index).unwrap();
        if !node.is_named() {
            if node.kind() != ";" {
                errors.push(unsupported(
                    source,
                    node,
                    "unsupported Bash operator or token",
                ));
            }
            continue;
        }
        match node.kind() {
            "comment" => {
                let text = node_text(node, source);
                if text.starts_with("#!") {
                    if text.contains("bash") || matches!(text, "#!/bin/sh" | "#!/usr/bin/sh") {
                        code.push_str("#!/usr/bin/env fish\n");
                    } else {
                        errors.push(unsupported(source, node, "unrecognized shebang"));
                    }
                } else {
                    code.push_str(text);
                    code.push('\n');
                }
            }
            "variable_assignment" => {
                assignment(node, source, false, &mut code, &mut warnings, &mut errors)
            }
            "declaration_command" => {
                let mut c = node.walk();
                let children: Vec<_> = node.named_children(&mut c).collect();
                let anonymous: Vec<_> = (0..node.child_count())
                    .map(|i| node.child(i).unwrap())
                    .filter(|child| !child.is_named())
                    .collect();
                if anonymous.len() != 1
                    || anonymous[0].kind() != "export"
                    || !node_text(node, source).starts_with("export ")
                    || children.len() != 1
                    || children[0].kind() != "variable_assignment"
                {
                    errors.push(unsupported(
                        source,
                        node,
                        "only simple export NAME=value is supported",
                    ));
                } else {
                    assignment(
                        children[0],
                        source,
                        true,
                        &mut code,
                        &mut warnings,
                        &mut errors,
                    );
                }
            }
            "command" => command(node, source, &mut code, &mut warnings, &mut errors),
            _ => errors.push(unsupported(
                source,
                node,
                "unsupported top-level Bash construct",
            )),
        }
    }
    if errors.is_empty() {
        Ok(Translation {
            code,
            diagnostics: warnings,
        })
    } else {
        Err(errors)
    }
}

fn node_text<'a>(n: tree_sitter::Node<'_>, s: &'a str) -> &'a str {
    &s[n.start_byte()..n.end_byte()]
}
fn unsupported(s: &str, n: tree_sitter::Node<'_>, msg: &str) -> Diagnostic {
    diagnostics::at(
        s,
        n.start_byte(),
        n.end_byte(),
        Classification::Unsupported,
        msg,
    )
}
fn warn(s: &str, n: tree_sitter::Node<'_>, msg: &str) -> Diagnostic {
    diagnostics::at(
        s,
        n.start_byte(),
        n.end_byte(),
        Classification::Approximate,
        msg,
    )
}
fn assignment(
    n: tree_sitter::Node<'_>,
    s: &str,
    export: bool,
    out: &mut String,
    warns: &mut Vec<Diagnostic>,
    errors: &mut Vec<Diagnostic>,
) {
    let tokens: Vec<_> = (0..n.child_count())
        .map(|i| n.child(i).unwrap())
        .filter(|child| !child.is_named())
        .collect();
    if tokens.len() != 1 || tokens[0].kind() != "=" {
        errors.push(unsupported(
            s,
            n,
            "unsupported assignment operator or token",
        ));
        return;
    }
    let Some(name_node) = n.child_by_field_name("name") else {
        errors.push(unsupported(s, n, "malformed assignment"));
        return;
    };
    let Some(value) = n.child_by_field_name("value") else {
        errors.push(unsupported(
            s,
            n,
            "assignment without a scalar value is unsupported",
        ));
        return;
    };
    let name = node_text(name_node, s);
    if !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
        || name
            .as_bytes()
            .first()
            .is_none_or(|b| !b.is_ascii_alphabetic() && *b != b'_')
    {
        errors.push(unsupported(s, n, "invalid or special variable name"));
        return;
    }
    if s[name_node.end_byte()..value.start_byte()].trim() != "=" {
        errors.push(unsupported(
            s,
            n,
            "only simple NAME=value assignments are supported",
        ));
        return;
    }
    if special_variable(name) {
        errors.push(unsupported(
            s,
            n,
            "assignment to a shell-special variable is unsupported",
        ));
        return;
    }
    let Some(value) = word(value, s, warns, errors) else {
        return;
    };
    warns.push(warn(
        s,
        n,
        "fish variables are lists; this assignment assumes an ordinary scalar value",
    ));
    out.push_str("builtin true\n");
    out.push_str(if export { "set -gx " } else { "set -g " });
    out.push_str(name);
    out.push(' ');
    out.push_str(&value);
    out.push('\n');
}
fn command(
    n: tree_sitter::Node<'_>,
    s: &str,
    out: &mut String,
    warns: &mut Vec<Diagnostic>,
    errors: &mut Vec<Diagnostic>,
) {
    if (0..n.child_count()).any(|i| !n.child(i).unwrap().is_named()) {
        errors.push(unsupported(s, n, "unsupported command operator or token"));
        return;
    }
    if n.child_by_field_name("redirect").is_some() {
        errors.push(unsupported(s, n, "redirections are not supported"));
        return;
    }
    let mut c = n.walk();
    let children: Vec<_> = n.named_children(&mut c).collect();
    if children.iter().any(|x| x.kind() == "variable_assignment") {
        errors.push(unsupported(
            s,
            n,
            "temporary command assignments are not supported",
        ));
        return;
    }
    let Some(name) = n.child_by_field_name("name") else {
        errors.push(unsupported(s, n, "missing command name"));
        return;
    };
    let mut name_cursor = name.walk();
    let name_parts: Vec<_> = name.named_children(&mut name_cursor).collect();
    if name_parts.len() != 1 || name_parts[0].kind() != "word" {
        errors.push(unsupported(
            s,
            name,
            "command name must be a plain literal word",
        ));
        return;
    }
    let cmd = node_text(name_parts[0], s);
    if !cmd
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b"_./-".contains(&b))
        || cmd.starts_with('-')
    {
        errors.push(unsupported(
            s,
            name,
            "command name must be a plain literal word",
        ));
        return;
    }
    if shell_builtin(cmd) && !matches!(cmd, "echo" | "printf" | "true" | "false") {
        errors.push(unsupported(
            s,
            n,
            "ambiguous or unsupported shell builtin in command position",
        ));
        return;
    }
    let mut rendered = vec![cmd.to_owned()];
    let mut args = Vec::new();
    for arg in children.into_iter().filter(|x| x.id() != name.id()) {
        if arg.kind() == "command_name" {
            continue;
        }
        let Some(v) = word(arg, s, warns, errors) else {
            return;
        };
        args.push((arg, v));
    }
    if cmd == "true" || cmd == "false" {
        if !args.is_empty() {
            errors.push(unsupported(
                s,
                n,
                "true and false are supported only without arguments",
            ));
            return;
        }
    } else if cmd == "echo" {
        if args
            .iter()
            .any(|(arg, _)| literal_starts_with_dash(*arg, s))
        {
            errors.push(unsupported(
                s,
                n,
                "echo options and escape-sensitive forms are not supported",
            ));
            return;
        }
    } else if cmd == "printf"
        && (args.is_empty()
            || !matches!(
                node_text(args[0].0, s),
                "'%s'" | "'%s\\n'" | "\"%s\"" | "\"%s\\n\""
            ))
    {
        errors.push(unsupported(
            s,
            n,
            "printf supports only tested %s and %s\\n formats",
        ));
        return;
    }
    rendered.extend(args.into_iter().map(|(_, v)| v));
    out.push_str(&rendered.join(" "));
    out.push('\n');
}
fn word(
    n: tree_sitter::Node<'_>,
    s: &str,
    warns: &mut Vec<Diagnostic>,
    errors: &mut Vec<Diagnostic>,
) -> Option<String> {
    match n.kind() {
        "word" => {
            let t = node_text(n, s);
            if t.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_./:@%+=,-".contains(&b))
                && !t.starts_with('~')
            {
                Some(t.to_owned())
            } else {
                errors.push(unsupported(
                    s,
                    n,
                    "unquoted shell word may expand, split, or glob",
                ));
                None
            }
        }
        "number" => {
            let t = node_text(n, s);
            let digits = t
                .strip_prefix('-')
                .or_else(|| t.strip_prefix('+'))
                .unwrap_or(t);
            if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) {
                Some(t.to_owned())
            } else {
                errors.push(unsupported(
                    s,
                    n,
                    "only decimal integer literals are supported",
                ));
                None
            }
        }
        "raw_string" => {
            let t = node_text(n, s);
            Some(single_quote(
                t.get(1..t.len().saturating_sub(1)).unwrap_or(""),
            ))
        }
        "string" => {
            let mut cursor = n.walk();
            let mut parts = Vec::new();
            for child in n.named_children(&mut cursor) {
                match child.kind() {
                    "string_content" => parts.push(escape_double(node_text(child, s))),
                    "simple_expansion" => {
                        let mut c = child.walk();
                        let nested: Vec<_> = child.named_children(&mut c).collect();
                        if nested.len() != 1 || nested[0].kind() != "variable_name" {
                            errors.push(unsupported(
                                s,
                                child,
                                "special or positional parameter expansion is unsupported",
                            ));
                            return None;
                        }
                        let var = node_text(nested[0], s);
                        if special_variable(var)
                            || var == "?"
                            || var == "$"
                            || var == "#"
                            || var.starts_with(|c: char| c.is_ascii_digit())
                        {
                            errors.push(unsupported(
                                s,
                                child,
                                "special variable semantics are unsupported",
                            ));
                            return None;
                        }
                        warns.push(warn(s,child,"quoted Bash scalar expansion maps to a fish list expanded as one argument"));
                        parts.push(format!("${var}"));
                    }
                    _ => {
                        errors.push(unsupported(s, child, "unsupported quoted expansion"));
                        return None;
                    }
                }
            }
            Some(format!("\"{}\"", parts.join("")))
        }
        _ => {
            errors.push(unsupported(s, n, "unsupported word syntax"));
            None
        }
    }
}
fn single_quote(s: &str) -> String {
    let mut out = String::from("'");
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\'' => out.push_str("\\'"),
            _ => out.push(ch),
        }
    }
    out.push('\'');
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
                | "HOME"
                | "PWD"
                | "OLDPWD"
                | "IFS"
                | "CDPATH"
                | "ENV"
                | "SHELL"
                | "RANDOM"
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

fn literal_starts_with_dash(node: tree_sitter::Node<'_>, source: &str) -> bool {
    let text = node_text(node, source);
    match node.kind() {
        "raw_string" | "string" => text.get(1..).is_some_and(|s| s.starts_with('-')),
        _ => text.starts_with('-'),
    }
}

fn escape_double(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars().peekable();
    while let Some(ch) = chars.next() {
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
