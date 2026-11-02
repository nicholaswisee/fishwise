use std::process::Command;

fn translate(script: &str) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_fws"))
        .args(["--translate", script])
        .output()
        .unwrap()
}

#[test]
fn echo_prefix_proof_accounts_for_quoted_continuations_and_empty_values() {
    for source in [
        "echo \"\\\n--\"",
        "echo \"\\\n-n\"",
        "X=\"\\\n--\"; echo \"$X\"",
        "X=\"\\\n-n\"; echo \"$X\"",
        "X=''; echo \"$X--\"",
    ] {
        let out = Command::new(env!("CARGO_BIN_EXE_fws"))
            .args(["--translate", "--strict", source])
            .output()
            .unwrap();
        assert_ne!(out.status.code(), Some(0), "claimed safe echo: {source:?}");
        assert!(out.stdout.is_empty(), "partial output: {source:?}");
    }
    for source in [
        "X=one; echo \"$X\"",
        "X=\"\\\none\"; echo \"$X\"",
        "echo \"\\\none\"",
    ] {
        let out = Command::new(env!("CARGO_BIN_EXE_fws"))
            .args(["--translate", "--strict", source])
            .output()
            .unwrap();
        assert_eq!(
            out.status.code(),
            Some(0),
            "{source:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

#[test]
fn reserved_bash_and_fish_variables_reject_across_all_binding_operations() {
    for name in [
        "SRANDOM",
        "_",
        "history",
        "version",
        "hostname",
        "status_generation",
        "CMD_DURATION",
        "last_pid",
        "COLUMNS",
        "LINES",
    ] {
        for source in [
            format!("printf '%s\\n' \"${name}\""),
            format!("{name}=ordinary"),
            format!("export {name}=ordinary"),
            format!("f() {{ local {name}=ordinary; }}"),
            format!("unset {name}"),
        ] {
            let out = translate(&source);
            assert_ne!(
                out.status.code(),
                Some(0),
                "accepted reserved variable: {source}"
            );
            assert!(out.stdout.is_empty(), "partial output: {source}");
        }
    }
    for name in ["HOME", "USER", "TERM"] {
        let out = translate(&format!("printf '%s\\n' \"${name}\""));
        assert_eq!(
            out.status.code(),
            Some(0),
            "mutable default {name}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

#[test]
fn padded_shebang_comments_do_not_become_generated_interpreters() {
    for (source, comment) in [
        ("\n#!/bin/bash\nexport EDITOR=nvim", "#!/bin/bash"),
        (
            "  #!/usr/bin/python\nexport EDITOR=nvim",
            "#!/usr/bin/python",
        ),
        ("\t#!unknown\nexport EDITOR=nvim", "#!unknown"),
    ] {
        let out = Command::new(env!("CARGO_BIN_EXE_fws"))
            .args(["--translate", "--strict", source])
            .output()
            .unwrap();
        assert_eq!(
            out.status.code(),
            Some(0),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let code = String::from_utf8(out.stdout).unwrap();
        assert!(!code.starts_with("#!"), "invented interpreter: {code:?}");
        assert!(code.contains(comment), "lost comment: {code:?}");
    }
    let out = translate("#!/bin/bash\nexport EDITOR=nvim");
    assert_eq!(out.status.code(), Some(0));
    assert!(out.stdout.starts_with(b"#!/usr/bin/env fish\n"));
    for control in ['\r', '\u{b}', '\u{c}'] {
        let out = translate(&format!("#!/bin/bash{control}\nexport EDITOR=nvim"));
        assert_ne!(
            out.status.code(),
            Some(0),
            "recognized control-padded interpreter"
        );
        assert!(out.stdout.is_empty());
    }
}

#[test]
fn ignored_parser_gaps_cannot_discard_bash_argument_bytes() {
    for gap in [
        "\r", "\u{b}", "\u{c}", "\u{a0}", "\u{2003}", "\\ ", "\\\t", "\\\u{b}", "\\\u{c}", "\\\n",
        "\\\r\n",
    ] {
        for source in [
            format!("echo hello{gap}\n"),
            format!("{gap}echo hello"),
            gap.to_owned(),
            format!("printf '%s\\n' a{gap}value"),
            format!("printf '%s\\n' {gap}value"),
            format!("if true;{gap}then true; fi"),
            format!("for x in a;{gap}do true; done"),
            format!("echo prefix; f() {{ if true; then printf '%s\\n' {gap}value; fi; }}"),
        ] {
            let out = Command::new(env!("CARGO_BIN_EXE_fws"))
                .args(["--translate", "--strict", &source])
                .output()
                .unwrap();
            assert_ne!(out.status.code(), Some(0), "lost gap bytes in {source:?}");
            assert!(out.stdout.is_empty(), "partial output: {source:?}");
            assert!(
                String::from_utf8_lossy(&out.stderr).contains("fws:"),
                "missing source diagnostic: {source:?}"
            );
        }
    }
    for source in ["", " \t\n\t ", "\t echo\t hello\n"] {
        let out = translate(source);
        assert_eq!(
            out.status.code(),
            Some(0),
            "ordinary trivia {source:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

fn fish_available() -> bool {
    let available = Command::new("fish")
        .arg("--version")
        .output()
        .is_ok_and(|out| {
            out.status.success() && String::from_utf8_lossy(&out.stdout).contains("version 4.")
        });
    if !available {
        assert_ne!(
            std::env::var("FWS_REQUIRE_FISH").as_deref(),
            Ok("1"),
            "fish 4.x required"
        );
        eprintln!("SKIP: fish 4.x unavailable; trusted parity/syntax checks not run");
    }
    available
}

#[test]
fn translates_nested_blocks_and_function_scope() {
    let script = "function demo() {\n local name=world\n if [[ -n \"$name\" ]]; then\n echo \"hello $name\"\n elif [ -z \"$name\" ]; then\n false\n else\n true\n fi\n unset missing\n}\n";
    let out = translate(script);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let code = String::from_utf8(out.stdout).unwrap();
    assert!(code.contains("function demo\n"));
    assert!(code.contains("set -fu name world"));
    assert!(code.contains("if test -n \"$name\"\n"));
    assert!(code.contains("else if test -z \"$name\"\n"));
    assert!(code.contains("    if set -q -g missing\n        set -eg missing\n    end"));
    assert!(code.ends_with("end\n"));
}

#[test]
fn rejects_ambiguous_globs_and_pattern_tests_without_partial_code() {
    for script in [
        "echo *.rs",
        "echo $X",
        "[[ x == x* ]]",
        "[[ x =~ x ]]",
        "for ((i=0;i<2;i++)); do true; done",
    ] {
        let out = translate(script);
        assert_ne!(out.status.code(), Some(0), "accepted {script:?}");
        assert!(out.stdout.is_empty(), "emitted code for {script:?}");
    }
}

#[test]
fn translates_spec_nested_fixture_and_status_expansions_without_intervening_commands() {
    let source = include_str!("fixtures/translate/nested.sh");
    let out = translate(source);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        include_str!("fixtures/translate/nested.fish")
    );

    for script in [
        "false; echo $?",
        "false | true; echo $?",
        "true | false; echo $?",
        "echo $? $?; echo $?",
        "echo \"$HOME\"",
    ] {
        let out = translate(script);
        assert_eq!(
            out.status.code(),
            Some(0),
            "{script}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let code = String::from_utf8(out.stdout).unwrap();
        assert!(!code.contains("$?"), "unmapped status in {code:?}");
        if script.contains("$?") {
            assert!(code.contains("$status"), "{code}");
        }
    }
    assert!(translate("VALUE=$?").status.code() != Some(0));
}

#[test]
fn translates_literal_loops_while_and_nested_function_locals() {
    let script = "function f() { local value=old; if [ -n \"$value\" ]; then value=new; fi; unset value; for item in one two; do echo \"$item\"; done; while [ -z \"$value\" ]; do true; done; }";
    let out = translate(script);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let code = String::from_utf8(out.stdout).unwrap();
    assert!(code.contains("set -fu value old"));
    assert!(code.contains("set -f value new"));
    assert!(code.contains("set -f value ''"));
    assert!(code.contains("for item in one two\n"));
    assert!(code.contains("while test -z \"$value\"\n"));
    assert!(String::from_utf8_lossy(&out.stderr).contains("loop-variable lifetime"));
}

#[test]
fn rejects_pipeline_state_mutations_and_unresolved_scope_operators() {
    for script in [
        "VALUE=one | cat",
        "echo one |& cat",
        "[[ value == value* ]]",
        "[[ value =~ value ]]",
        "echo {a,b}",
    ] {
        let out = translate(script);
        assert_ne!(out.status.code(), Some(0), "accepted {script:?}");
        assert!(out.stdout.is_empty(), "emitted code for {script:?}");
    }
}

#[test]
fn warns_about_bash_caller_local_dynamic_lookup() {
    let script = "function g() { echo \"$X\"; X=changed; }; function f() { local X=caller; g; }";
    let out = translate(script);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(String::from_utf8_lossy(&out.stderr).contains("caller-local binding"));
    assert!(String::from_utf8_lossy(&out.stderr).contains("function assignment may differ"));
}

#[test]
fn rejects_structural_tokens_and_unsupported_variants() {
    for script in [
        "if [ -f file ]; then echo one & echo two; fi",
        "if true; then true; else echo one & echo two; fi",
        "if false; then true; elif true; then echo one & echo two; fi",
        "until [ -f file ]; do echo waiting; done",
        "select x in a b; do echo \"$x\"; done",
        "for x; do echo \"$x\"; done",
        "for x in a; do echo one & echo two; done",
        "while true; do echo one & echo two; done",
        "f() { echo hello; } > output",
        "f() { local X+=suffix; echo \"$X\"; }",
        "f() { local X[0]=value; }",
        "f() { local PATH=value; }",
        "for PATH in a; do true; done",
        "for status in a; do true; done",
        "end() { true; }",
        "if echo condition; then [ -f file ]; echo yes; fi",
        "if true; false; then echo yes; fi",
        "if true & then echo yes; fi",
        "if true; then true; elif echo condition; then [ -f file ]; fi",
        "while true; false; do echo yes; done",
        "f() { true; }; f | cat",
        "cat | f; f() { true; }",
        "if [[ 010 -eq 8 ]]; then true; fi",
        "if [[ \"$X\" -eq 8 ]]; then true; fi",
        "X=\" $?\"",
        "f() { local X=\" $?\"; }",
        "X=one; echo \"$X\\\nsuffix\"",
        "false; echo \"$?\\\nsuffix\"",
        "#!/not-bash\necho ok",
        "#!/bin/bash -e\necho ok",
        "#!/usr/bin/env bash -eu\necho ok",
    ] {
        let out = translate(script);
        assert_ne!(
            out.status.code(),
            Some(0),
            "accepted {script:?}: {}",
            String::from_utf8_lossy(&out.stdout)
        );
        assert!(out.stdout.is_empty(), "partial code for {script:?}");
    }
}

#[test]
fn true_false_conditions_and_shebang_comments() {
    for script in [
        "if true; then echo yes; elif false; then false; else true; fi",
        "while false; do echo unreachable; done",
        "#!/bin/bash\necho ok\n#!/not-bash\n",
        "#!/usr/bin/env bash\necho ok",
        "if [[ 10 -eq 10 ]]; then true; fi",
    ] {
        let out = translate(script);
        assert_eq!(
            out.status.code(),
            Some(0),
            "{script}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let out = translate("echo ok\n#!/bin/bash\n");
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        "echo ok\n#!/bin/bash\n"
    );
}

#[test]
fn trusted_status_and_local_shadow_parity() {
    if !fish_available() {
        return;
    }
    let dir = std::env::temp_dir().join(format!(
        "fws-task5-parity-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&dir).unwrap();
    for script in [
        "false; echo $? $?",
        "false | true; echo $?",
        "true | false; echo $?",
        "false; X=value; echo $?; echo \"$X\"",
        "false; export X=value; echo $?",
        "f() { false; local X=value; echo $?; X=next; echo $?; }; f",
        "f() { false; local X=value; }; f",
        "false; X=value",
        "false; f() { false; }; echo $?",
        "false; export X=value",
        "false; unset MISSING_TASK5",
        "export X=global; f() { local X=local; bash -c 'printf \"%s\\n\" \"$X\"'; }; f",
        "f() { local X=local; bash -c 'printf \"%s\\n\" \"$X\"'; }; export X=global; f",
        "X=global; f() { local X=local; export X=exported; X=updated; bash -c 'printf \"%s\\n\" \"$X\"'; }; f; echo \"$X\"",
        "X=global; f() { local X=local; unset X; echo \"$X\"; }; f; echo \"$X\"",
        "false; unset MISSING_TASK5; echo $?",
        "if false; then false; fi; echo $?",
        "while false; do false; done; echo $?",
        "false; if [ \"$?\" = '1' ]; then echo yes; fi",
        "false; echo \"$? $?\"",
        "X=one; Y=two; echo \"$X $Y \"",
        "X=one; echo \"  $X   \"",
        "X=one; echo \"$X[1]\"",
        "false; echo \"$?suffix\"",
        "echo \"a\\\nb\"",
        "echo \"escaped\\`tick\"",
        "if true; then echo 'first\n  literal line'; fi",
        "f() { if true; then local X=value; fi; echo \"$X\"; }; f",
        "export X=old; X=new; bash -c 'printf \"%s\\n\" \"$X\"'",
    ] {
        let out = translate(script);
        assert_eq!(
            out.status.code(),
            Some(0),
            "{script}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let bash = Command::new("bash")
            .env_clear().env("PATH", std::env::var_os("PATH").unwrap_or_else(|| "/usr/bin:/bin".into()))
            .env("HOME", &dir).env("XDG_CONFIG_HOME", &dir).current_dir(&dir)
            .args(["--noprofile", "--norc", "-c", script])
            .output()
            .unwrap();
        let fish = Command::new("fish")
            .env_clear().env("PATH", std::env::var_os("PATH").unwrap_or_else(|| "/usr/bin:/bin".into()))
            .env("HOME", &dir).env("XDG_CONFIG_HOME", &dir).current_dir(&dir)
            .args([
                "--no-config",
                "-c",
                std::str::from_utf8(&out.stdout).unwrap(),
            ])
            .output()
            .unwrap();
        assert_eq!(
            fish.stdout,
            bash.stdout,
            "{script}\nfish code: {}\nfish stderr: {}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&fish.stderr)
        );
        assert_eq!(
            fish.status.code(),
            bash.status.code(),
            "terminal status: {script}"
        );
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn control_flow_joins_do_not_invent_definite_local_bindings() {
    for script in [
        "f() { if true; then local X=one; fi; X=two; }",
        "f() { if true; then local X=one; else X=two; fi; unset X; }",
        "f() { while false; do local X=one; done; X=two; }",
        "f() { for item in a; do local X=one; done; unset X; }",
    ] {
        let out = translate(script);
        assert_ne!(
            out.status.code(),
            Some(0),
            "accepted ambiguous scope: {script}"
        );
        assert!(out.stdout.is_empty());
        assert!(String::from_utf8_lossy(&out.stderr).contains("scope depends"));
    }
    for script in [
        "if true; then X=one; fi; echo \"$X\"",
        "while false; do export X=one; done; echo \"$X\"",
        "f() { if true; then local X=one; fi; echo \"$X\"; }",
        "X=global; g() { echo \"$X\"; }; f() { local X=caller; g; }; f",
        "unset UNKNOWN_TASK5",
        "f() { local X=one; unset X; }",
    ] {
        let out = translate(script);
        assert_eq!(
            out.status.code(),
            Some(0),
            "{script}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(String::from_utf8_lossy(&out.stderr).contains("warning"));
        let strict = Command::new(env!("CARGO_BIN_EXE_fws"))
            .args(["--translate", "--strict", script])
            .output()
            .unwrap();
        assert_ne!(strict.status.code(), Some(0), "strict accepted {script}");
        assert!(strict.stdout.is_empty());
    }
    let script = "f() { if true; then local X=one; else local X=two; fi; X=updated; echo \"$X\"; }";
    let out = translate(script);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(String::from_utf8_lossy(&out.stdout).contains("set -f X updated"));
}

#[test]
fn strict_known_scalar_reads_and_explain_are_specific() {
    for script in [
        "X=one; echo \"$X\"",
        "if true; then X=one; else export X=two; fi; echo \"$X\"",
        "false; echo $? $?",
    ] {
        let out = Command::new(env!("CARGO_BIN_EXE_fws"))
            .args(["--translate", "--strict", "--explain", script])
            .output()
            .unwrap();
        assert_eq!(
            out.status.code(),
            Some(0),
            "{script}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let diagnostics = String::from_utf8_lossy(&out.stderr);
        assert!(!diagnostics.contains("warning"), "{diagnostics}");
        assert!(
            diagnostics.contains("scalar assignment uses set")
                || diagnostics.contains("Bash $? becomes fish $status"),
            "{diagnostics}"
        );
    }
}

#[test]
fn unsupported_nested_forms_never_emit_a_convertible_prefix() {
    for body in [
        "echo $X",
        "echo ${X:-fallback}",
        "echo $(true)",
        "echo <(true)",
        "X=(one two)",
        "export X+=two",
        "local X[0]=one",
        "local -x X=one",
        "unset -f name",
        "unset X Y",
        "unset 'X[0]'",
        "source setup.sh",
        "set -a",
        "shopt -s nullglob",
        "trap 'true' EXIT",
        "eval 'true'",
        "exec echo hi",
        "echo hi >output",
        "echo hi |& cat",
        "echo hi && true",
        "echo hi &",
        "echo hi; f() { true; }",
        "[[ a =~ a ]]",
    ] {
        let script = format!("echo prefix; f() {{ if true; then {body}; fi; }}");
        let out = translate(&script);
        assert_ne!(out.status.code(), Some(0), "accepted {script}");
        assert!(out.stdout.is_empty(), "partial output: {script}");
        let diagnostic = String::from_utf8_lossy(&out.stderr);
        assert!(
            diagnostic.contains("fws:1:"),
            "missing location: {diagnostic}"
        );
    }
}

#[test]
fn readable_goldens_preserve_literal_lines_and_pass_fish_syntax() {
    for (source, expected) in [
        (
            include_str!("fixtures/translate/nested.sh"),
            include_str!("fixtures/translate/nested.fish"),
        ),
        (
            include_str!("fixtures/translate/scope.sh"),
            include_str!("fixtures/translate/scope.fish"),
        ),
    ] {
        let out = translate(source);
        assert_eq!(
            out.status.code(),
            Some(0),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(out.stdout, expected.as_bytes());
        if fish_available() {
            let out = Command::new(env!("CARGO_BIN_EXE_fws"))
                .args(["--translate", "--validate", source])
                .output()
                .unwrap();
            assert_eq!(
                out.status.code(),
                Some(0),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
            assert_eq!(out.stdout, expected.as_bytes());
        }
    }
}

#[test]
fn comments_in_structured_headers_are_not_dropped_or_reinterpreted() {
    let script = "if # before if\ntrue; # before then\nthen # in arm\ntrue; elif # before elif test\nfalse; then false; else # in else\ntrue; fi\nwhile # before while\nfalse; # before do\ndo true; done\nfor x in a b; # for list\ndo true; done\nf() # function header\n{ #!not a shebang\ntrue; }";
    let out = translate(script);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let code = String::from_utf8(out.stdout).unwrap();
    for comment in [
        "# before if",
        "# before then",
        "# in arm",
        "# before elif test",
        "# in else",
        "# before while",
        "# before do",
        "# for list",
        "# function header",
        "#!not a shebang",
    ] {
        assert_eq!(
            code.matches(comment).count(),
            1,
            "lost or duplicated {comment}: {code}"
        );
    }
}
