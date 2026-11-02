use std::fs;
use std::io::Write;
use std::process::{Command, Output, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

fn fws() -> Command {
    Command::new(env!("CARGO_BIN_EXE_fws"))
}

fn run(script: &str) -> Output {
    fws().arg(script).output().expect("run fws")
}

#[cfg(unix)]
fn run_os(script: std::ffi::OsString) -> Output {
    fws().arg(script).output().expect("run fws")
}

#[test]
fn executes_bash_semantics_and_sources_exported_env_file() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let env_file = std::env::temp_dir().join(format!("fws-{unique}.env"));
    fs::write(&env_file, "FROM_ENV=loaded\n").unwrap();
    let script = format!(
        r#"set -a
source '{}'
set +a
bash -c 'printf "<%s>\n" "$FROM_ENV"'
items=(red blue)
n=4
((n += 3))
name=${{items[1]}}
[[ ${{items[1]}} == blue ]] && test "$n" -eq 7
measured=$(printf '%s' "$name")
printf '%s|%s|%s|%s|%s|%s|%s\n' "$FROM_ENV" "$name" "$n" "${{#items[@]}}" "${{name:-fallback}}" "${{name:0:2}}" "$measured"
"#,
        env_file.display()
    );
    let output = run(&script);
    fs::remove_file(env_file).unwrap();

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stdout, b"<loaded>\nloaded|blue|7|2|blue|bl|blue\n");
}

#[test]
fn empty_script_succeeds() {
    assert_eq!(run("").status.code(), Some(0));
}

#[test]
fn preserves_heredocs_and_script_exit_status() {
    let output = run("cat <<'END'\nBash $variables stay literal\nEND\nexit 23\n");
    assert_eq!(output.stdout, b"Bash $variables stay literal\n");
    assert_eq!(output.status.code(), Some(23));
}

#[test]
fn syntax_errors_keep_bash_failure_status() {
    let output = run("if then\n");
    assert_eq!(output.status.code(), Some(2));
    assert!(!output.stderr.is_empty());
}

#[test]
fn no_arguments_reads_complete_stdin_script() {
    let mut child = fws()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"first=one\nsecond=two\nprintf '%s:%s\\n' \"$first\" \"$second\"\n")
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stdout, b"one:two\n");
}

#[test]
fn command_mode_preserves_stdin_for_bash() {
    let mut child = fws()
        .arg("IFS= read -r line; printf '<%s>\\n' \"$line\"")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"kept for bash\n")
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stdout, b"<kept for bash>\n");
}

#[test]
fn rejects_multiple_arguments_instead_of_joining_them() {
    let output = fws().args(["echo", "not one script"]).output().unwrap();
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("quote") || stderr.contains("paste"));
}

#[test]
fn rejects_a_lone_separator() {
    let output = fws().arg("--").output().unwrap();
    assert_eq!(output.status.code(), Some(2));
}

#[cfg(unix)]
#[test]
fn accepts_legacy_flag_shaped_single_scripts_as_bash_scripts() {
    for script in ["--norc", "-e"] {
        let output = run(script);
        assert_eq!(output.status.code(), Some(127));
        assert!(String::from_utf8_lossy(&output.stderr).contains(script));
    }
}

#[cfg(unix)]
#[test]
fn escaped_translate_option_is_script_text() {
    let output = fws().args(["--", "--translate"]).output().unwrap();
    assert_eq!(output.status.code(), Some(127));
    assert!(String::from_utf8_lossy(&output.stderr).contains("--translate"));
}

#[test]
fn double_dash_allows_a_script_starting_with_a_dash() {
    let output = fws().args(["--", "--help"]).output().unwrap();
    assert_eq!(output.status.code(), Some(127));
    assert!(String::from_utf8_lossy(&output.stderr).contains("--help"));
}

#[cfg(unix)]
#[test]
fn preserves_non_utf8_script_arguments() {
    use std::os::unix::ffi::OsStringExt;

    let script = std::ffi::OsString::from_vec(b"printf '<%s>\\n' $'\xff'".to_vec());
    let output = run_os(script);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stdout, b"<\xff>\n");
}

#[cfg(unix)]
#[test]
fn preserves_non_utf8_stdin_script_bytes() {
    let mut child = fws()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"printf '<%s>\\n' $'\xff'")
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stdout, b"<\xff>\n");
}

#[cfg(unix)]
#[test]
fn preserves_working_directory_and_environment() {
    let output = fws()
        .arg("printf '%s|%s\\n' \"$PWD\" \"$FWS_TEST_VALUE\"")
        .current_dir("/")
        .env("FWS_TEST_VALUE", "kept")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stdout, b"/|kept\n");
}

#[cfg(unix)]
#[test]
fn command_mode_keeps_file_redirected_stdin_for_bash() {
    let path = std::env::temp_dir().join(format!("fws-stdin-{}", std::process::id()));
    fs::write(&path, "redirected input\n").unwrap();
    let file = fs::File::open(&path).unwrap();
    let output = fws()
        .arg("IFS= read -r line; printf '<%s>\\n' \"$line\"")
        .stdin(file)
        .output()
        .unwrap();
    fs::remove_file(path).unwrap();
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stdout, b"<redirected input>\n");
}

#[cfg(unix)]
#[test]
fn stdin_script_reader_leaves_bash_at_eof() {
    let mut child = fws()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(
            b"if IFS= read -r line; then printf 'not eof\\n'; exit 1; else printf 'eof\\n'; fi",
        )
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stdout, b"eof\n");
}

#[cfg(unix)]
#[test]
fn non_executable_bash_is_reported_as_126() {
    use std::os::unix::fs::PermissionsExt;

    let directory = std::env::temp_dir().join(format!("fws-path-{}", std::process::id()));
    fs::create_dir_all(&directory).unwrap();
    let bash = directory.join("bash");
    fs::write(&bash, "#!/bin/sh\n").unwrap();
    fs::set_permissions(&bash, fs::Permissions::from_mode(0o644)).unwrap();
    let output = fws().arg("true").env("PATH", &directory).output().unwrap();
    fs::remove_dir_all(directory).unwrap();
    assert_eq!(output.status.code(), Some(126));
}

#[cfg(unix)]
#[test]
fn propagates_signal_termination_from_bash() {
    use std::os::unix::process::ExitStatusExt;

    let output = run("kill -TERM $$");
    assert_eq!(output.status.signal(), Some(15));
}

#[test]
fn help_is_available() {
    for flag in ["-h", "--help"] {
        let output = fws().arg(flag).output().unwrap();
        assert_eq!(output.status.code(), Some(0));
        assert!(String::from_utf8_lossy(&output.stdout).contains("fws"));
    }
}

#[cfg(unix)]
#[test]
fn missing_bash_reports_127_even_with_empty_path() {
    let output = fws().arg("true").env("PATH", "").output().unwrap();
    assert_eq!(output.status.code(), Some(127));
    assert!(String::from_utf8_lossy(&output.stderr).contains("bash"));
}

#[cfg(unix)]
#[test]
fn interior_nul_is_reported_without_panicking() {
    let mut child = fws()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"echo\0broken")
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(126));
    assert!(!output.stderr.is_empty());
}
