use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::thread;

pub fn syntax_check(code: &str) -> Result<(), String> {
    let mut child = Command::new("fish")
        .args(["--no-config", "-n"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("failed to start fish syntax validator: {e}"))?;
    let mut stdin = child.stdin.take().expect("piped stdin");
    let mut stdout = child.stdout.take().expect("piped stdout");
    let mut stderr = child.stderr.take().expect("piped stderr");
    let input = code.as_bytes().to_vec();
    let writer = thread::spawn(move || stdin.write_all(&input));
    let out_reader = thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = stdout.read_to_end(&mut bytes);
        result.map(|_| bytes)
    });
    let err_reader = thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = stderr.read_to_end(&mut bytes);
        result.map(|_| bytes)
    });
    let write_result = writer
        .join()
        .map_err(|_| "validator stdin writer panicked".to_string())?;
    let stdout = out_reader
        .join()
        .map_err(|_| "validator stdout reader panicked".to_string())?
        .map_err(|e| format!("failed to read fish validator output: {e}"))?;
    let stderr = err_reader
        .join()
        .map_err(|_| "validator stderr reader panicked".to_string())?
        .map_err(|e| format!("failed to read fish validator diagnostics: {e}"))?;
    let status = child
        .wait()
        .map_err(|e| format!("failed to wait for fish validator: {e}"))?;
    if !status.success() {
        let detail = String::from_utf8_lossy(&stderr);
        return Err(if detail.trim().is_empty() {
            format!("fish rejected generated syntax (exit {})", status)
        } else {
            format!("fish rejected generated syntax: {}", detail.trim())
        });
    }
    if write_result.is_err() {
        return Err("failed to provide generated code to fish syntax validator".into());
    }
    let _ = stdout;
    Ok(())
}
