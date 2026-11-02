use std::io::Write;
use std::process::{Command, Stdio};

fn fws() -> Command {
    Command::new(env!("CARGO_BIN_EXE_fws"))
}

#[test]
fn translates_script_without_running_commands_and_preserves_argument_data() {
    let out = fws()
        .args(["--translate", "--", "echo 'hello world'"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(out.stdout, b"echo 'hello world'\n");
    assert!(out.stderr.is_empty());
}

#[test]
fn translates_assignments_and_quoted_words() {
    let out = fws()
        .args(["-t", "export EDITOR=nvim; NAME='two words'; echo \"$NAME\""])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    let code = String::from_utf8(out.stdout).unwrap();
    assert!(code.contains("set -gx EDITOR nvim"));
    assert!(code.contains("set -g NAME 'two words'"));
    assert!(code.contains("echo \"$NAME\""));
}

#[test]
fn stdin_is_consumed_completely_and_empty_input_is_valid() {
    for input in [b"echo first\necho second\n".as_slice(), b""] {
        let mut child = fws()
            .arg("--translate")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(input).unwrap();
        let out = child.wait_with_output().unwrap();
        assert_eq!(out.status.code(), Some(0));
        if input.is_empty() {
            assert!(out.stdout.is_empty());
        } else {
            assert!(String::from_utf8_lossy(&out.stdout).contains("echo second"));
        }
    }
}

#[test]
fn unsupported_construct_fails_without_partial_output_or_execution() {
    let out = fws()
        .args([
            "--translate",
            "echo safe; echo $(touch /tmp/fws-translate-sentinel)",
        ])
        .output()
        .unwrap();
    assert_ne!(out.status.code(), Some(0));
    assert!(out.stdout.is_empty());
    assert!(!std::path::Path::new("/tmp/fws-translate-sentinel").exists());
    assert!(!out.stderr.is_empty());
}

#[test]
fn external_commands_are_data_but_substitutions_are_never_run() {
    let sentinel = format!("/tmp/fws-translate-{}", std::process::id());
    let _ = std::fs::remove_file(&sentinel);
    let out = fws()
        .arg("--translate")
        .arg(format!("touch {sentinel}"))
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    assert!(!std::path::Path::new(&sentinel).exists());
    assert!(String::from_utf8_lossy(&out.stdout).contains("touch"));

    let out = fws()
        .arg("--translate")
        .arg(format!("echo $(touch {sentinel})"))
        .output()
        .unwrap();
    assert_ne!(out.status.code(), Some(0));
    assert!(out.stdout.is_empty());
    assert!(!std::path::Path::new(&sentinel).exists());
}

#[test]
fn quoted_literal_dollars_and_empty_values_are_encoded_as_data() {
    let out = fws()
        .arg("--translate")
        .arg("echo \"\\$HOME\"; NAME='' ")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    let code = String::from_utf8(out.stdout).unwrap();
    assert!(code.contains("echo \"\\$HOME\""));
    assert!(code.contains("set -g NAME ''"));
}

#[test]
fn printf_and_embedded_program_strings_remain_literal_arguments() {
    let out = fws()
        .arg("--translate")
        .arg("printf '%s\\n' 'echo $HOME'; python -c 'print(\"$HOME\")'")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    let code = String::from_utf8(out.stdout).unwrap();
    assert!(code.contains("printf '%s\\\\n' 'echo $HOME'"));
    assert!(code.contains("python -c 'print(\"$HOME\")'"));
}

fn translate(script: &str) -> std::process::Output {
    fws().arg("--translate").arg(script).output().unwrap()
}

#[test]
fn rejects_semantic_operators_and_dynamic_or_quoted_command_names() {
    for script in [
        "echo one & echo two",
        "$(echo cmd) argument",
        "$CMD arg",
        "'set' -a",
        "echo one && echo two",
        "echo one || echo two",
    ] {
        let out = translate(script);
        assert_ne!(out.status.code(), Some(0), "accepted {script:?}");
        assert!(out.stdout.is_empty(), "emitted code for {script:?}");
    }
}

#[test]
fn restricts_echo_and_printf_to_semantically_safe_forms() {
    for script in ["echo -- -n", "echo -n", "echo '--'", "printf '%q\\n' 'a b'"] {
        let out = translate(script);
        assert_ne!(out.status.code(), Some(0), "accepted {script:?}");
        assert!(out.stdout.is_empty(), "emitted code for {script:?}");
    }
    for script in ["echo", "true", "false", "printf '%s\\n' value"] {
        let out = translate(script);
        assert_eq!(
            out.status.code(),
            Some(0),
            "rejected {script:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

#[test]
fn assignment_status_is_normalized_before_scalar_set() {
    let out = translate("false; VALUE=1");
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let code = String::from_utf8(out.stdout).unwrap();
    assert!(code.contains("builtin true\nset -g VALUE 1"));
    let fish = Command::new("fish")
        .args(["--no-config", "-c", &code])
        .output()
        .unwrap();
    assert_eq!(fish.status.code(), Some(0));
    assert!(translate("VALUE=$?").status.code() != Some(0));
}

#[test]
fn raw_string_backslashes_preserve_value_and_ansi_c_quotes_reject() {
    for script in [
        "printf '%s' 'a\\\\b'",
        "printf '%s\\n' 'a\\\\b'",
        "echo 'a\\\\b'",
    ] {
        let out = translate(script);
        assert_eq!(
            out.status.code(),
            Some(0),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let code = String::from_utf8(out.stdout).unwrap();
        let fish = Command::new("fish")
            .args(["--no-config", "-c", &code])
            .output()
            .unwrap();
        let bash = Command::new("bash").args(["-c", script]).output().unwrap();
        assert_eq!(fish.stdout, bash.stdout, "{script:?}");
    }
    assert_ne!(translate("echo $'a\\nb'").status.code(), Some(0));
}

#[test]
fn special_variable_writes_and_unimplemented_builtins_reject() {
    for script in [
        "BASH_ENV=anything",
        "IFS=x",
        "PATH=x",
        "PWD=x",
        "status=x",
        "fish_user_paths=x",
        "trap x EXIT",
        "shopt -s nullglob",
        "alias ll='ls -l'",
        ". file",
        "declare X=1",
        "shift",
        "block",
        "breakpoint",
    ] {
        let out = translate(script);
        assert_ne!(out.status.code(), Some(0), "accepted {script:?}");
        assert!(out.stdout.is_empty(), "emitted code for {script:?}");
    }
    for (script, expected) in [
        ("COUNT=42", "set -g COUNT 42"),
        ("COUNT=-42", "set -g COUNT -42"),
    ] {
        let numeric = translate(script);
        assert_eq!(
            numeric.status.code(),
            Some(0),
            "{}",
            String::from_utf8_lossy(&numeric.stderr)
        );
        assert!(String::from_utf8_lossy(&numeric.stdout).contains(expected));
    }
}

#[test]
fn invalid_and_multiple_translation_arguments_fail() {
    for args in [
        vec!["--translate", "one", "two"],
        vec!["--translate", "--unknown-option"],
    ] {
        let out = fws().args(args).output().unwrap();
        assert_eq!(out.status.code(), Some(2));
        assert!(out.stdout.is_empty());
    }

    let out = fws()
        .args(["--translate", "--strict"])
        .stdin(Stdio::piped())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    assert!(out.stdout.is_empty());
}

#[test]
fn rejects_malformed_input_with_location_and_nul_without_output() {
    let malformed = fws()
        .args(["--translate", "echo ok\nif then"])
        .output()
        .unwrap();
    assert_eq!(malformed.status.code(), Some(1));
    assert!(malformed.stdout.is_empty());
    assert!(String::from_utf8_lossy(&malformed.stderr).contains("2:"));

    let mut child = fws()
        .arg("--translate")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(b"echo\0bad").unwrap();
    let out = child.wait_with_output().unwrap();
    assert_ne!(out.status.code(), Some(0));
    assert!(out.stdout.is_empty());
}

#[test]
fn comments_and_shebang_are_preserved_without_inventing_one() {
    let out = fws()
        .args(["--translate", "#!/usr/bin/env bash\n# note\necho ok"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&out.stdout).contains("#!/usr/bin/env fish"));
    assert!(String::from_utf8_lossy(&out.stdout).contains("# note"));

    let out = fws().args(["--translate", "echo ok"]).output().unwrap();
    assert!(!String::from_utf8_lossy(&out.stdout).starts_with("#!"));
}

#[cfg(unix)]
#[test]
fn rejects_invalid_utf8_translation_argument() {
    use std::os::unix::ffi::OsStringExt;
    let out = fws()
        .arg("--translate")
        .arg(std::ffi::OsString::from_vec(b"echo \xff".to_vec()))
        .output()
        .unwrap();
    assert_ne!(out.status.code(), Some(0));
    assert!(out.stdout.is_empty());
}
