//! Interactive Markdown editing through the user's editor. The draft lives
//! in a temp file that survives every failure, so nothing written in the
//! editor is ever lost.

use std::io::IsTerminal;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::cli::error::CliError;

/// An edited draft: the file path (kept on failure for recovery) and the
/// resulting content.
pub(crate) struct Draft {
    pub path: PathBuf,
    pub content: String,
}

impl Draft {
    /// Deletes the recovery copy once the server accepted the content.
    pub(crate) fn discard(self) {
        let _ = std::fs::remove_file(self.path);
    }
}

/// Opens `initial` in `$VISUAL`/`$EDITOR` and returns the edited content.
pub(crate) fn open(initial: &str) -> Result<Draft, CliError> {
    if !std::io::stdin().is_terminal() {
        return Err(CliError::usage(
            "editing needs an interactive terminal; use `post create --file` or a piped stdin instead",
        ));
    }
    let path = temp_path();
    std::fs::write(&path, initial).map_err(CliError::Io)?;
    if let Err(err) = run_editor(&path) {
        return Err(err.with_note(&format!(
            "your text is kept at {} for recovery",
            path.display()
        )));
    }
    let content = std::fs::read_to_string(&path).map_err(CliError::Io)?;
    Ok(Draft { path, content })
}

fn run_editor(path: &std::path::Path) -> Result<(), CliError> {
    let editor = std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .unwrap_or_else(|_| "vi".to_owned());
    if editor.trim().is_empty() {
        return Err(CliError::usage(
            "$VISUAL/$EDITOR is unset or empty; set one to edit posts",
        ));
    }
    // Whitespace splitting lets plain commands like `code -w` work without
    // shell quoting games.
    let mut words = editor.split_whitespace();
    let program = words.next().expect("editor is non-empty");
    let args: Vec<&str> = words.collect();
    let status = Command::new(program)
        .args(args)
        .arg(path)
        .status()
        .map_err(|err| CliError::usage(format!("cannot run editor '{editor}': {err}")))?;
    if !status.success() {
        return Err(CliError::usage(format!(
            "editor '{editor}' exited with {status}"
        )));
    }
    Ok(())
}

fn temp_path() -> PathBuf {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    std::env::temp_dir().join(format!(
        "statesu-edit-{}-{}.md",
        std::process::id(),
        now.as_millis()
    ))
}
