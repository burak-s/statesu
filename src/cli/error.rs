use std::fmt;
use std::process::ExitCode;

/// CLI failures with exit codes: usage problems exit 2, everything else 1.
#[derive(Debug)]
pub enum CliError {
    /// Local filesystem problems.
    Io(std::io::Error),
    /// HTTP connection or response decoding failures.
    Network(reqwest::Error),
    /// The server answered with a structured API error.
    Api {
        status: u16,
        message: String,
        retry_after: Option<u64>,
    },
    /// Local configuration or storage problems.
    Config(String),
    /// The invocation itself is wrong.
    Usage(String),
}

impl From<std::io::Error> for CliError {
    fn from(err: std::io::Error) -> Self {
        CliError::Io(err)
    }
}

impl From<reqwest::Error> for CliError {
    fn from(err: reqwest::Error) -> Self {
        CliError::Network(err)
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CliError::Io(err) => write!(f, "io error: {err}"),
            CliError::Network(err) => write!(f, "request failed: {err}"),
            CliError::Api {
                status,
                message,
                retry_after,
            } => match retry_after {
                Some(seconds) => write!(f, "HTTP {status}: {message} (retry after {seconds}s)"),
                None => write!(f, "HTTP {status}: {message}"),
            },
            CliError::Config(detail) => write!(f, "local error: {detail}"),
            CliError::Usage(why) => write!(f, "usage error: {why}"),
        }
    }
}

impl std::error::Error for CliError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            CliError::Io(err) => Some(err),
            CliError::Network(err) => Some(err),
            _ => None,
        }
    }
}

impl CliError {
    /// Builds a usage error: the user or automation made a wrong call.
    pub fn usage(why: impl Into<String>) -> Self {
        CliError::Usage(why.into())
    }

    pub fn exit_code(&self) -> ExitCode {
        if matches!(self, CliError::Usage(_)) {
            ExitCode::from(2)
        } else {
            ExitCode::FAILURE
        }
    }

    /// Appends context (for example a recovery-file path) to message-bearing
    /// variants; transport errors keep their source-based rendering.
    pub(crate) fn with_note(mut self, note: &str) -> Self {
        match &mut self {
            CliError::Usage(text) => {
                text.push('\n');
                text.push_str(note);
            }
            CliError::Config(detail) => {
                detail.push('\n');
                detail.push_str(note);
            }
            CliError::Api { message, .. } => {
                message.push('\n');
                message.push_str(note);
            }
            _ => {}
        }
        self
    }
}
