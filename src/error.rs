use std::fmt;

///The single error type for the whole crate. Domain and service code returns
/// `Error`; status codes and JSON bodies are assigned only in `http::error`.
#[derive(Debug)]
pub enum Error {
    InvalidBody,
    InvalidEmail,
    InvalidPassword,
    UserAlreadyExists,
    InvalidCredentials,
    MissingToken,
    InvalidToken,
    TokenExpired,
    InvalidInput,
    /// Authenticated but not permitted: entity limits reached.
    Forbidden,
    BlogNotFound,
    PostNotFound,
    PatNotFound,
    /// Login succeeded cryptographically but the address was never confirmed.
    EmailNotVerified,
    SlugTaken,
    DomainTaken,
    PayloadTooLarge,
    UnsupportedMediaType,
    RateLimited {
        retry_after_secs: u64,
    },
    ServiceUnavailable,
    RequestTimeout,
    /// Unexpected infrastructure failures. Details are logged, never returned.
    Internal(InternalError),
    /// Configuration is validated at startup and never reaches HTTP; this
    /// variant exists so config and runtime code share one error type.
    Config(String),
}

/// Wrapper so `Internal` variants never leak raw error text into responses.
#[derive(Debug)]
pub struct InternalError(pub Box<dyn std::error::Error + Send + Sync + 'static>);

impl InternalError {
    pub fn new<E: std::error::Error + Send + Sync + 'static>(err: E) -> InternalError {
        InternalError(Box::new(err))
    }
}

impl fmt::Display for InternalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "internal error: {}", self.0)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Config(detail) => write!(f, "configuration error: {detail}"),
            Error::Internal(internal) => write!(f, "{internal}"),
            _ => f.write_str(self.message()),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Internal(internal) => Some(internal),
            _ => None,
        }
    }
}

impl std::error::Error for InternalError {}

impl Error {
    /// Stable wire-level message; also the canonical strings documented in the
    /// public API guide, so these must stay an exact match.
    pub fn message(&self) -> &'static str {
        match self {
            Error::InvalidBody => "invalid body",
            Error::InvalidEmail => "invalid email",
            Error::InvalidPassword => {
                "password must be 8-72 chars with at least one letter and one digit"
            }
            Error::UserAlreadyExists => "user already exists",
            Error::InvalidCredentials => "invalid credentials",
            Error::MissingToken => "missing token",
            Error::InvalidToken => "invalid token",
            Error::TokenExpired => "token expired",
            Error::InvalidInput => "invalid input",
            Error::Forbidden => "forbidden",
            Error::BlogNotFound => "blog not found",
            Error::PostNotFound => "post not found",
            Error::PatNotFound => "token not found",
            Error::EmailNotVerified => "email not verified",
            Error::SlugTaken => "slug already in use",
            Error::DomainTaken => "domain already claimed",
            Error::PayloadTooLarge => "request body too large",
            Error::UnsupportedMediaType => "unsupported media type",
            Error::RateLimited { .. } => "rate limit exceeded",
            Error::ServiceUnavailable => "service unavailable",
            Error::RequestTimeout => "request timeout",
            Error::Internal(_) => "internal error",
            Error::Config(_) => "internal error",
        }
    }

    pub fn status_code(&self) -> http::StatusCode {
        use http::StatusCode;
        match self {
            Error::InvalidBody
            | Error::InvalidEmail
            | Error::InvalidPassword
            | Error::InvalidInput => StatusCode::BAD_REQUEST,
            Error::UserAlreadyExists | Error::SlugTaken | Error::DomainTaken => {
                StatusCode::CONFLICT
            }
            Error::InvalidCredentials
            | Error::MissingToken
            | Error::InvalidToken
            | Error::TokenExpired => StatusCode::UNAUTHORIZED,
            Error::Forbidden => StatusCode::FORBIDDEN,
            Error::BlogNotFound | Error::PostNotFound | Error::PatNotFound => StatusCode::NOT_FOUND,
            Error::EmailNotVerified => StatusCode::FORBIDDEN,
            Error::PayloadTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            Error::UnsupportedMediaType => StatusCode::UNSUPPORTED_MEDIA_TYPE,
            Error::RateLimited { .. } => StatusCode::TOO_MANY_REQUESTS,
            Error::ServiceUnavailable => StatusCode::SERVICE_UNAVAILABLE,
            Error::RequestTimeout => StatusCode::GATEWAY_TIMEOUT,
            Error::Internal(_) | Error::Config(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_strings_are_stable() {
        assert_eq!(Error::InvalidBody.message(), "invalid body");
        assert_eq!(
            Error::InvalidPassword.message(),
            "password must be 8-72 chars with at least one letter and one digit"
        );
        assert_eq!(Error::UserAlreadyExists.message(), "user already exists");
        assert_eq!(Error::InvalidCredentials.message(), "invalid credentials");
        assert_eq!(Error::MissingToken.message(), "missing token");
        assert_eq!(Error::InvalidToken.message(), "invalid token");
        assert_eq!(Error::TokenExpired.message(), "token expired");
        assert_eq!(Error::InvalidInput.message(), "invalid input");
        assert_eq!(Error::Forbidden.message(), "forbidden");
        assert_eq!(Error::BlogNotFound.message(), "blog not found");
        assert_eq!(Error::PostNotFound.message(), "post not found");
        assert_eq!(Error::PatNotFound.message(), "token not found");
        assert_eq!(Error::EmailNotVerified.message(), "email not verified");
        assert_eq!(Error::SlugTaken.message(), "slug already in use");
        assert_eq!(Error::DomainTaken.message(), "domain already claimed");
        assert_eq!(Error::PayloadTooLarge.message(), "request body too large");
        assert_eq!(
            Error::UnsupportedMediaType.message(),
            "unsupported media type"
        );
        assert_eq!(Error::ServiceUnavailable.message(), "service unavailable");
        assert_eq!(Error::RequestTimeout.message(), "request timeout");
        assert_eq!(
            Error::Internal(InternalError::new(std::io::Error::other("x"))).message(),
            "internal error"
        );
    }

    #[test]
    fn http_status_codes() {
        use http::StatusCode;
        assert_eq!(Error::UserAlreadyExists.status_code(), StatusCode::CONFLICT);
        assert_eq!(Error::TokenExpired.status_code(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            Error::PayloadTooLarge.status_code(),
            StatusCode::PAYLOAD_TOO_LARGE
        );
        assert_eq!(
            Error::UnsupportedMediaType.status_code(),
            StatusCode::UNSUPPORTED_MEDIA_TYPE
        );
        assert_eq!(
            Error::ServiceUnavailable.status_code(),
            StatusCode::SERVICE_UNAVAILABLE
        );
        assert_eq!(
            Error::RequestTimeout.status_code(),
            StatusCode::GATEWAY_TIMEOUT
        );
    }
}
