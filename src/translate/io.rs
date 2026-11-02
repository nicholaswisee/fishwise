use super::Source;
use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::{self, IsTerminal, Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT_TEMP: AtomicUsize = AtomicUsize::new(0);

pub fn read_source(source: &Source) -> Result<String, String> {
    let bytes = match source {
        Source::Script(script) => script
            .clone()
            .into_string()
            .map_err(|_| "translation input is not valid UTF-8".to_string())?
            .into_bytes(),
        Source::File(path) => {
            fs::read(path).map_err(|e| format!("failed to read {}: {e}", path.display()))?
        }
        Source::Stdin => {
            let stdin = io::stdin();
            if stdin.is_terminal() {
                eprint!("Paste Bash script, then press Ctrl-D to translate: ");
                let _ = io::stderr().flush();
            }
            let mut bytes = Vec::new();
            stdin
                .lock()
                .read_to_end(&mut bytes)
                .map_err(|e| format!("failed to read stdin: {e}"))?;
            bytes
        }
    };
    String::from_utf8(bytes).map_err(|_| "translation input is not valid UTF-8".into())
}

pub fn reject_same_file(input: &Source, output: &Path) -> Result<(), String> {
    let Source::File(input) = input else {
        return Ok(());
    };
    let input_meta = fs::metadata(input)
        .map_err(|e| format!("failed to inspect input {}: {e}", input.display()))?;
    if let Ok(output_meta) = fs::metadata(output) {
        use std::os::unix::fs::MetadataExt;
        if input_meta.dev() == output_meta.dev() && input_meta.ino() == output_meta.ino() {
            return Err("input and output refer to the same file".into());
        }
    }
    Ok(())
}

pub fn publish(path: &Path, bytes: &[u8], force: bool) -> io::Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "refusing to overwrite a symlink destination",
            ));
        }
        Ok(_) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    if path.file_name().is_none() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "output path has no file name",
        ));
    }
    let (stage, mut file) = loop {
        let n = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let mut temp_name = OsString::from(".fws-tmp-");
        temp_name.push(std::process::id().to_string());
        temp_name.push("-");
        temp_name.push(n.to_string());
        let stage = parent.join(temp_name);
        if stage == path {
            continue;
        }
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&stage)
        {
            Ok(file) => break (stage, file),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    };
    let result = (|| {
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        if force {
            fs::rename(&stage, path)?;
        } else {
            fs::hard_link(&stage, path)?;
            if let Err(e) = fs::remove_file(&stage) {
                eprintln!(
                    "fws: warning: published output but failed to remove staged link {}: {e}",
                    stage.display()
                );
            }
        }
        Ok(())
    })();
    if result.is_err() {
        if let Err(cleanup) = fs::remove_file(&stage) {
            if cleanup.kind() != io::ErrorKind::NotFound {
                eprintln!(
                    "fws: warning: failed to remove staged output {}: {cleanup}",
                    stage.display()
                );
            }
        }
    }
    result
}
