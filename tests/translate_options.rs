use std::fs;
use std::os::unix::fs::symlink;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};

static NEXT: AtomicUsize = AtomicUsize::new(0);

fn temp_dir() -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "fws-options-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&path).unwrap();
    path
}

fn fws(args: &[&std::ffi::OsStr], stdin: &[u8]) -> Output {
    use std::io::Write;
    use std::process::Stdio;

    let mut child = Command::new(env!("CARGO_BIN_EXE_fws"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(stdin).unwrap();
    child.wait_with_output().unwrap()
}

fn args<'a>(items: &'a [&'a str]) -> Vec<&'a std::ffi::OsStr> {
    use std::os::unix::ffi::OsStrExt;
    items
        .iter()
        .map(|s| std::ffi::OsStr::from_bytes(s.as_bytes()))
        .collect()
}

#[test]
fn file_input_output_emits_only_code_and_preserves_input() {
    let dir = temp_dir();
    let input = dir.join("source.sh");
    let output = dir.join("translated.fish");
    fs::write(&input, "echo hello\n").unwrap();
    let result = fws(
        &[
            std::ffi::OsStr::new("--translate"),
            std::ffi::OsStr::new("--file"),
            input.as_os_str(),
            std::ffi::OsStr::new("--output"),
            output.as_os_str(),
        ],
        b"",
    );
    assert_eq!(
        result.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(result.stdout.is_empty());
    assert_eq!(fs::read(&input).unwrap(), b"echo hello\n");
    assert_eq!(fs::read(&output).unwrap(), b"echo hello\n");
}

#[test]
fn existing_destination_is_preserved_unless_force_is_given() {
    let dir = temp_dir();
    let output = dir.join("out.fish");
    fs::write(&output, b"keep me").unwrap();
    let result = fws(
        &[
            std::ffi::OsStr::new("--translate"),
            std::ffi::OsStr::new("echo hello"),
            std::ffi::OsStr::new("--output"),
            output.as_os_str(),
        ],
        b"",
    );
    assert_eq!(result.status.code(), Some(1));
    assert_eq!(fs::read(&output).unwrap(), b"keep me");

    let result = fws(
        &[
            std::ffi::OsStr::new("--translate"),
            std::ffi::OsStr::new("echo hello"),
            std::ffi::OsStr::new("--output"),
            output.as_os_str(),
            std::ffi::OsStr::new("--force"),
        ],
        b"",
    );
    assert_eq!(
        result.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(fs::read(&output).unwrap(), b"echo hello\n");
}

#[test]
fn dangling_destination_symlink_is_never_followed_or_clobbered() {
    let dir = temp_dir();
    let target = dir.join("target");
    let output = dir.join("link");
    symlink(&target, &output).unwrap();
    let result = fws(
        &[
            std::ffi::OsStr::new("--translate"),
            std::ffi::OsStr::new("echo hello"),
            std::ffi::OsStr::new("--output"),
            output.as_os_str(),
        ],
        b"",
    );
    assert_eq!(result.status.code(), Some(1));
    assert!(fs::symlink_metadata(&output)
        .unwrap()
        .file_type()
        .is_symlink());
    assert!(!target.exists());
}

#[test]
fn force_refuses_observed_symlinks_without_writing_through_them() {
    let dir = temp_dir();
    let target = dir.join("target");
    let output = dir.join("link");
    fs::write(&target, b"target contents").unwrap();
    symlink(&target, &output).unwrap();
    let result = fws(
        &[
            std::ffi::OsStr::new("--translate"),
            std::ffi::OsStr::new("echo hello"),
            std::ffi::OsStr::new("--output"),
            output.as_os_str(),
            std::ffi::OsStr::new("--force"),
        ],
        b"",
    );
    assert_eq!(result.status.code(), Some(1));
    assert!(fs::symlink_metadata(&output)
        .unwrap()
        .file_type()
        .is_symlink());
    assert_eq!(fs::read(&target).unwrap(), b"target contents");

    let dangling = dir.join("dangling");
    let missing_target = dir.join("missing-target");
    symlink(&missing_target, &dangling).unwrap();
    let result = fws(
        &[
            std::ffi::OsStr::new("--translate"),
            std::ffi::OsStr::new("echo hello"),
            std::ffi::OsStr::new("--output"),
            dangling.as_os_str(),
            std::ffi::OsStr::new("--force"),
        ],
        b"",
    );
    assert_eq!(result.status.code(), Some(1));
    assert!(fs::symlink_metadata(dangling)
        .unwrap()
        .file_type()
        .is_symlink());
    assert!(!missing_target.exists());
}

#[test]
fn non_utf8_positional_script_is_a_processing_error_with_or_without_separator() {
    use std::os::unix::ffi::OsStrExt;

    for args in [
        vec![
            std::ffi::OsStr::new("--translate"),
            std::ffi::OsStr::from_bytes(b"echo \xff"),
        ],
        vec![
            std::ffi::OsStr::new("--translate"),
            std::ffi::OsStr::new("--"),
            std::ffi::OsStr::from_bytes(b"echo \xff"),
        ],
    ] {
        let result = fws(&args, b"");
        assert_eq!(result.status.code(), Some(1));
        assert!(result.stdout.is_empty());
        assert!(String::from_utf8_lossy(&result.stderr).contains("not valid UTF-8"));
    }
}

#[test]
fn output_matching_first_staging_candidate_is_published_atomically() {
    let dir = temp_dir();
    let pid_file = dir.join("pid");
    let result = Command::new("bash")
        .args(["-c", "printf %s $$ > \"$2/pid\"; exec \"$1\" --translate --output \"$2/.fws-tmp-$$-0\" -- 'echo hello'", "driver"])
        .arg(env!("CARGO_BIN_EXE_fws"))
        .arg(&dir)
        .output()
        .unwrap();
    let pid = fs::read_to_string(pid_file).unwrap();
    let output = dir.join(format!(".fws-tmp-{pid}-0"));
    assert_eq!(
        result.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(fs::read(&output).unwrap(), b"echo hello\n");
    assert_eq!(fs::read_dir(&dir).unwrap().count(), 2);
}

#[test]
fn input_output_alias_is_rejected_even_with_force() {
    let dir = temp_dir();
    let input = dir.join("source.sh");
    let alias = dir.join("alias.sh");
    fs::write(&input, "echo hello\n").unwrap();
    symlink(&input, &alias).unwrap();
    let result = fws(
        &[
            std::ffi::OsStr::new("--translate"),
            std::ffi::OsStr::new("--file"),
            input.as_os_str(),
            std::ffi::OsStr::new("--output"),
            alias.as_os_str(),
            std::ffi::OsStr::new("--force"),
        ],
        b"",
    );
    assert_eq!(result.status.code(), Some(1));
    assert_eq!(fs::read(&input).unwrap(), b"echo hello\n");
}

#[test]
fn check_and_explain_never_put_generated_code_on_stdout() {
    let checked = fws(
        &[
            std::ffi::OsStr::new("--translate"),
            std::ffi::OsStr::new("echo hello"),
            std::ffi::OsStr::new("--check"),
        ],
        b"",
    );
    assert_eq!(checked.status.code(), Some(0));
    assert!(checked.stdout.is_empty());
    let explained = fws(
        &[
            std::ffi::OsStr::new("--translate"),
            std::ffi::OsStr::new("echo hello"),
            std::ffi::OsStr::new("--explain"),
        ],
        b"",
    );
    assert_eq!(explained.status.code(), Some(0));
    assert_eq!(explained.stdout, b"echo hello\n");
    assert!(!explained.stderr.is_empty());
    let plain = fws(
        &[
            std::ffi::OsStr::new("--translate"),
            std::ffi::OsStr::new("echo hello"),
        ],
        b"",
    );
    assert!(
        plain.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&plain.stderr)
    );
}

#[test]
fn dynamic_echo_options_are_approximate_and_strict_rejects_them() {
    let ordinary = fws(
        &[
            std::ffi::OsStr::new("--translate"),
            std::ffi::OsStr::new("X=--; echo \"$X\""),
        ],
        b"",
    );
    assert_eq!(ordinary.status.code(), Some(0));
    assert_eq!(ordinary.stdout, b"builtin true\nset -g X --\necho \"$X\"\n");
    assert!(String::from_utf8_lossy(&ordinary.stderr)
        .contains("echo argument may be interpreted as a fish option"));

    let strict = fws(
        &[
            std::ffi::OsStr::new("--translate"),
            std::ffi::OsStr::new("X=--; echo \"$X\""),
            std::ffi::OsStr::new("--strict"),
        ],
        b"",
    );
    assert_eq!(strict.status.code(), Some(1));
    assert!(strict.stdout.is_empty());
}

#[test]
fn duplicate_and_conflicting_options_are_usage_errors() {
    for options in [
        args(&["--translate", "--check", "--check"]),
        args(&["--translate", "--check", "--output", "out"]),
        args(&["--translate", "--file", "one", "--file", "two"]),
        args(&["--translate", "--output", "one", "--output", "two"]),
        args(&["--translate", "echo hello", "--file", "input.sh"]),
        args(&["--translate", "--force"]),
    ] {
        let result = fws(&options, b"");
        assert_eq!(result.status.code(), Some(2), "args: {options:?}");
    }
}

#[test]
fn failed_translation_does_not_publish_partial_output() {
    let dir = temp_dir();
    let output = dir.join("out.fish");
    fs::write(&output, b"prior").unwrap();
    let result = fws(
        &[
            std::ffi::OsStr::new("--translate"),
            std::ffi::OsStr::new("if then"),
            std::ffi::OsStr::new("--output"),
            output.as_os_str(),
            std::ffi::OsStr::new("--force"),
        ],
        b"",
    );
    assert_eq!(result.status.code(), Some(1));
    assert!(result.stdout.is_empty());
    assert_eq!(fs::read(&output).unwrap(), b"prior");
}

#[test]
fn rejected_side_effect_sentinels_are_inert_in_every_analysis_mode() {
    let dir = temp_dir();
    let sentinel = dir.join("must-not-exist");
    let script = format!("echo prefix; echo \"$(touch {})\"", sentinel.display());
    for flags in [
        vec![],
        vec!["--check"],
        vec!["--explain"],
        vec!["--strict"],
        vec!["--validate"],
    ] {
        let mut argv = vec![
            std::ffi::OsString::from("--translate"),
            script.clone().into(),
        ];
        argv.extend(flags.iter().map(std::ffi::OsString::from));
        let refs: Vec<_> = argv.iter().map(|arg| arg.as_os_str()).collect();
        let result = fws(&refs, b"");
        assert_ne!(result.status.code(), Some(0), "flags: {flags:?}");
        assert!(result.stdout.is_empty(), "flags: {flags:?}");
        assert!(!sentinel.exists(), "executed source for flags: {flags:?}");
    }

    let destination = dir.join("existing.fish");
    fs::write(&destination, b"preserve").unwrap();
    let second_script = format!("echo prefix; echo $(touch {})", sentinel.display());
    let result = fws(
        &[
            std::ffi::OsStr::new("--translate"),
            std::ffi::OsStr::new(&second_script),
            std::ffi::OsStr::new("--output"),
            destination.as_os_str(),
            std::ffi::OsStr::new("--force"),
        ],
        b"",
    );
    assert_ne!(result.status.code(), Some(0));
    assert!(result.stdout.is_empty());
    assert_eq!(fs::read(destination).unwrap(), b"preserve");
}

#[test]
fn staged_write_failure_preserves_existing_destination() {
    let dir = temp_dir();
    let output = dir.join("existing.fish");
    fs::write(&output, b"prior bytes").unwrap();
    let result = Command::new("bash")
        .args(["-c", "ulimit -f 0; trap '' XFSZ; exec \"$1\" --translate --output \"$2\" --force -- 'echo hello'", "driver"])
        .arg(env!("CARGO_BIN_EXE_fws"))
        .arg(&output)
        .output()
        .unwrap();
    assert_eq!(
        result.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(fs::read(output).unwrap(), b"prior bytes");
    assert_eq!(fs::read_dir(dir).unwrap().count(), 1, "staging file leaked");
}

#[test]
fn missing_file_is_a_processing_error() {
    let dir = temp_dir();
    let missing = dir.join("missing.sh");
    let result = fws(
        &[
            std::ffi::OsStr::new("--translate"),
            std::ffi::OsStr::new("--file"),
            missing.as_os_str(),
        ],
        b"",
    );
    assert_eq!(result.status.code(), Some(1));
    assert!(result.stdout.is_empty());
}

#[test]
fn competing_no_clobber_publishers_leave_one_complete_result() {
    let dir = temp_dir();
    let output = Arc::new(dir.join("shared.fish"));
    let barrier = Arc::new(Barrier::new(3));
    let threads: Vec<_> = ["echo alpha", "echo beta"]
        .into_iter()
        .map(|script| {
            let output = Arc::clone(&output);
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                let args = [
                    std::ffi::OsStr::new("--translate"),
                    std::ffi::OsStr::new(script),
                    std::ffi::OsStr::new("--output"),
                    output.as_os_str(),
                ];
                barrier.wait();
                fws(&args, b"")
            })
        })
        .collect();
    barrier.wait();
    let results: Vec<_> = threads.into_iter().map(|t| t.join().unwrap()).collect();
    assert_eq!(results.iter().filter(|r| r.status.success()).count(), 1);
    let bytes = fs::read(output.as_path()).unwrap();
    assert!(bytes == b"echo alpha\n" || bytes == b"echo beta\n");
}

#[test]
fn validator_receives_only_syntax_check_arguments_and_generated_stdin() {
    use std::os::unix::fs::PermissionsExt;
    let dir = temp_dir();
    let fish = dir.join("fish");
    let argv = dir.join("argv");
    let input = dir.join("input");
    fs::write(
        &fish,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\n/bin/cat > '{}'\n",
            argv.display(),
            input.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&fish, fs::Permissions::from_mode(0o755)).unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_fws"))
        .args(["--translate", "echo hello", "--check", "--validate"])
        .env("PATH", &dir)
        .output()
        .unwrap();
    assert_eq!(
        result.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(result.stdout.is_empty());
    assert_eq!(fs::read(argv).unwrap(), b"--no-config\n-n\n");
    assert_eq!(fs::read(input).unwrap(), b"echo hello\n");
}

#[test]
fn missing_validator_fails_without_output() {
    let dir = temp_dir();
    let result = Command::new(env!("CARGO_BIN_EXE_fws"))
        .args(["--translate", "echo hello", "--validate"])
        .env("PATH", dir)
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(1));
    assert!(result.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&result.stderr).contains("failed to start fish syntax validator")
    );
}

#[test]
fn validator_rejection_keeps_forced_destination_unchanged() {
    use std::os::unix::fs::PermissionsExt;
    let dir = temp_dir();
    let fish = dir.join("fish");
    let output = dir.join("out.fish");
    fs::write(&fish, "#!/bin/sh\necho syntax rejected >&2\nexit 7\n").unwrap();
    fs::set_permissions(&fish, fs::Permissions::from_mode(0o755)).unwrap();
    fs::write(&output, b"prior file").unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_fws"))
        .args(["--translate", "echo hello", "--output"])
        .arg(&output)
        .args(["--force", "--validate"])
        .env("PATH", &dir)
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(1));
    assert!(result.stdout.is_empty());
    assert!(String::from_utf8_lossy(&result.stderr).contains("syntax rejected"));
    assert_eq!(fs::read(output).unwrap(), b"prior file");
}

#[test]
fn validation_drains_large_generated_input_without_deadlock() {
    use std::os::unix::fs::PermissionsExt;
    let dir = temp_dir();
    let fish = dir.join("fish");
    let capture = dir.join("input");
    fs::write(
        &fish,
        format!("#!/bin/sh\n/bin/cat > '{}'\n", capture.display()),
    )
    .unwrap();
    fs::set_permissions(&fish, fs::Permissions::from_mode(0o755)).unwrap();
    let script = "echo x\n".repeat(12_000);
    let result = Command::new(env!("CARGO_BIN_EXE_fws"))
        .args(["--translate", &script, "--validate"])
        .env("PATH", &dir)
        .output()
        .unwrap();
    assert_eq!(
        result.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(fs::metadata(capture).unwrap().len() > 64 * 1024);
}

#[test]
fn strict_rejects_ambient_scalar_assumptions_before_output() {
    let normal = fws(
        &[
            std::ffi::OsStr::new("--translate"),
            std::ffi::OsStr::new("echo \"$AMBIENT_UNKNOWN\""),
        ],
        b"",
    );
    assert_eq!(normal.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&normal.stderr).contains("warning"));
    let strict = fws(
        &[
            std::ffi::OsStr::new("--translate"),
            std::ffi::OsStr::new("echo \"$AMBIENT_UNKNOWN\""),
            std::ffi::OsStr::new("--strict"),
        ],
        b"",
    );
    assert_eq!(strict.status.code(), Some(1));
    assert!(strict.stdout.is_empty());
}

#[test]
fn non_utf8_file_name_is_supported() {
    use std::os::unix::ffi::OsStringExt;
    let dir = temp_dir();
    let input = dir.join(std::ffi::OsString::from_vec(b"in-\xff.sh".to_vec()));
    fs::write(&input, "echo hello\n").unwrap();
    let result = fws(
        &[
            std::ffi::OsStr::new("--translate"),
            std::ffi::OsStr::new("--file"),
            input.as_os_str(),
        ],
        b"",
    );
    assert_eq!(
        result.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(result.stdout, b"echo hello\n");
}

#[test]
fn help_describes_both_modes_stdin_and_translation_flags() {
    for args in [vec!["--help"]] {
        let result = Command::new(env!("CARGO_BIN_EXE_fws"))
            .args(args)
            .output()
            .unwrap();
        assert_eq!(result.status.code(), Some(0));
        let help = String::from_utf8(result.stdout).unwrap();
        for phrase in [
            "Run mode (default)",
            "read stdin to EOF",
            "translate stdin to EOF",
            "--file reads a file",
            "--check analyzes without code output",
            "--explain shows transformations and assumptions",
            "--strict rejects approximate translations",
            "--validate runs fish -n",
            "-- before a script beginning with a dash",
        ] {
            assert!(help.contains(phrase), "missing {phrase:?} in {help}");
        }
    }
}

#[test]
fn help_is_recognized_only_as_the_exact_mode_help_pair() {
    for argv in [
        args(&["--translate", "--help"]),
        args(&["--translate", "-h"]),
        args(&["-t", "--help"]),
    ] {
        let result = fws(&argv, b"must not be read");
        assert_eq!(result.status.code(), Some(0), "args: {argv:?}");
        assert!(String::from_utf8_lossy(&result.stdout).contains("Run mode (default)"));
    }

    let literal = fws(&args(&["--translate", "--", "--help"]), b"");
    assert_eq!(literal.status.code(), Some(1));
    assert!(!String::from_utf8_lossy(&literal.stdout).contains("Run mode (default)"));

    let run_script = fws(&args(&["--", "--help"]), b"");
    assert_ne!(run_script.status.code(), Some(0));

    let filename = fws(&args(&["--translate", "--file", "--help"]), b"");
    assert_eq!(filename.status.code(), Some(1));
    assert!(!String::from_utf8_lossy(&filename.stdout).contains("Run mode (default)"));

    let extra = fws(&args(&["--translate", "--help", "extra"]), b"");
    assert_eq!(extra.status.code(), Some(2));
}

#[test]
fn staged_write_failure_does_not_modify_destination() {
    let dir = temp_dir();
    let output = dir.join("directory");
    fs::create_dir(&output).unwrap();
    let result = fws(
        &[
            std::ffi::OsStr::new("--translate"),
            std::ffi::OsStr::new("echo hello"),
            std::ffi::OsStr::new("--output"),
            output.as_os_str(),
            std::ffi::OsStr::new("--force"),
        ],
        b"",
    );
    assert_eq!(result.status.code(), Some(1));
    assert!(output.is_dir());
}

#[test]
fn alias_parent_and_hardlink_input_output_are_rejected() {
    let dir = temp_dir();
    let input = dir.join("source.sh");
    fs::write(&input, "echo hello\n").unwrap();
    let parent_alias = dir.join("sub");
    fs::create_dir(&parent_alias).unwrap();
    let output_parent = dir.join("alias-parent");
    symlink(&dir, &output_parent).unwrap();
    let output = output_parent.join("source.sh");
    let result = fws(
        &[
            std::ffi::OsStr::new("--translate"),
            std::ffi::OsStr::new("--file"),
            input.as_os_str(),
            std::ffi::OsStr::new("--output"),
            output.as_os_str(),
            std::ffi::OsStr::new("--force"),
        ],
        b"",
    );
    assert_eq!(result.status.code(), Some(1));
    assert_eq!(fs::read(&input).unwrap(), b"echo hello\n");

    let hardlink = dir.join("hardlink.sh");
    fs::hard_link(&input, &hardlink).unwrap();
    let result = fws(
        &[
            std::ffi::OsStr::new("--translate"),
            std::ffi::OsStr::new("--file"),
            input.as_os_str(),
            std::ffi::OsStr::new("--output"),
            hardlink.as_os_str(),
            std::ffi::OsStr::new("--force"),
        ],
        b"",
    );
    assert_eq!(result.status.code(), Some(1));
}
