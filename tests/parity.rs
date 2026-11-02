use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT: AtomicUsize = AtomicUsize::new(0);

const FIXTURES: &[&str] = &[
    "echo hello",
    "false; echo $?",
    "X=one; echo \"$X\"",
    "if true; then echo ok; fi",
    "printf '%s\\n' value",
    "printf '%s\\n' 'two words' 'three'",
    "X=''; echo \"$X\"; echo 'héllo 世界'",
    include_str!("fixtures/translate/nested.sh"),
    include_str!("fixtures/translate/scope.sh"),
];

fn private_dir() -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "fws-parity-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&path).unwrap();
    path
}

fn translator(script: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fws"))
        .args(["--translate", script])
        .output()
        .unwrap()
}

fn fish_4() -> bool {
    let version = Command::new("fish").arg("--version").output();
    let available = version.is_ok_and(|out| {
        out.status.success() && String::from_utf8_lossy(&out.stdout).contains("version 4.")
    });
    if !available {
        if std::env::var("FWS_REQUIRE_FISH").as_deref() == Ok("1") {
            panic!("FWS_REQUIRE_FISH=1 requires fish 4.x on PATH");
        }
        eprintln!("SKIP: fish 4.x unavailable; parity and syntax fixtures not executed");
    }
    available
}

#[test]
fn supported_goldens_pass_fish_syntax_and_trusted_fixtures_match_bash() {
    if !fish_4() {
        return;
    }
    let dir = private_dir();
    let path = std::env::var_os("PATH").unwrap_or_else(|| "/usr/bin:/bin".into());
    let home = dir.join("home");
    fs::create_dir(&home).unwrap();

    for &source in FIXTURES {
        let translated = translator(source);
        assert_eq!(
            translated.status.code(),
            Some(0),
            "{source:?}: {}",
            String::from_utf8_lossy(&translated.stderr)
        );
        let syntax = Command::new("fish")
            .args(["--no-config", "-n"])
            .env_clear()
            .env("PATH", &path)
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", &home)
            .current_dir(&dir)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .and_then(|mut child| {
                use std::io::Write;
                child.stdin.take().unwrap().write_all(&translated.stdout)?;
                child.wait_with_output()
            })
            .unwrap();
        assert!(
            syntax.status.success(),
            "invalid fish for {source:?}: {}",
            String::from_utf8_lossy(&syntax.stderr)
        );
    }

    for &source in &FIXTURES[..7] {
        let translated = translator(source);
        let bash = Command::new("bash")
            .args(["--noprofile", "--norc", "-c", source])
            .env_clear()
            .env("PATH", &path)
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", &home)
            .current_dir(&dir)
            .output()
            .unwrap();
        let fish = Command::new("fish")
            .args([
                "--no-config",
                "-c",
                std::str::from_utf8(&translated.stdout).unwrap(),
            ])
            .env_clear()
            .env("PATH", &path)
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", &home)
            .current_dir(&dir)
            .output()
            .unwrap();
        assert_eq!(
            fish.stdout,
            bash.stdout,
            "{source:?}; fish code: {}",
            String::from_utf8_lossy(&translated.stdout)
        );
        assert_eq!(fish.status.code(), bash.status.code(), "{source:?}");
    }
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn unsupported_adversarial_syntax_fails_closed_without_partial_code() {
    for source in [
        "items=(one two)",
        "declare -A map=([x]=y)",
        "echo \"$@\"",
        "echo ${value:-fallback}",
        "echo $((1 + 2))",
        "echo $(true)",
        "cat <<'EOF'\ntext\nEOF",
        "echo hi >out 2>&1",
        "source .env",
        ". ./.env",
        "set -a",
        "shopt -s nullglob",
        "trap 'true' EXIT",
        "eval true",
        "exec true",
        "unknown_syntax ((",
        "until true; do echo nope; done",
        "select x in a; do echo \"$x\"; done",
        "for x; do echo \"$x\"; done",
        "function f { true; } >out",
    ] {
        let result = translator(source);
        assert_ne!(result.status.code(), Some(0), "accepted {source:?}");
        assert!(result.stdout.is_empty(), "partial code for {source:?}");
    }
}
