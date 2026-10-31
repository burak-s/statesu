use sha2::{Digest, Sha256};

use crate::error::{Error, InternalError};

/// Token generation and hashing. All durable auth secrets (sessions,
/// verification tokens, personal access tokens) are opaque random strings
/// whose stored form is only the SHA-256 hash, so a database leak never
/// yields a usable credential.
/// Secret token lengths: 256 bits of entropy as 64 lowercase hex chars plus a
/// short type prefix that helps secret scanners identify leaked values.
const TOKEN_ENTROPY_BYTES: usize = 32;

/// Session cookie value: `<prefix><hex>`; the prefix keeps prefixes unique.
pub const SESSION_PREFIX: &str = "ss_";
/// Companion CSRF cookie value, readable by the dashboard JavaScript.
pub const CSRF_PREFIX: &str = "sc_";
/// Verification token issued in the confirmation link.
pub const VERIFICATION_PREFIX: &str = "sv_";
/// Personal access token handed to API/CLI clients exactly once.
pub const PAT_PREFIX: &str = "su_pat_";

/// Generates `su_pat_…`-style opaque token secrets.
pub fn generate(prefix: &str) -> Result<String, Error> {
    let mut entropy = [0u8; TOKEN_ENTROPY_BYTES];
    getrandom::fill(&mut entropy)
        .map_err(|e| Error::Internal(InternalError::new(std::io::Error::other(e.to_string()))))?;
    let mut token = String::with_capacity(prefix.len() + entropy.len() * 2);
    token.push_str(prefix);
    for byte in entropy {
        use std::fmt::Write as _;
        let _ = write!(token, "{byte:02x}");
    }
    Ok(token)
}

/// SHA-256 over the full token string; the database never sees the secret.
pub fn hash(token: &str) -> [u8; 32] {
    Sha256::digest(token.as_bytes()).into()
}

/// Prefixed-token shape check so obviously malformed input is rejected before
/// any database work: prefix matches, remainder is exactly 64 hex characters.
pub fn well_formed(token: &str, prefix: &str) -> bool {
    let Some(hex_part) = token.strip_prefix(prefix) else {
        return false;
    };
    hex_part.len() == TOKEN_ENTROPY_BYTES * 2
        && hex_part
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_tokens_have_expected_shape_and_entropy() {
        let token = generate(PAT_PREFIX).unwrap();
        assert!(token.starts_with(PAT_PREFIX));
        assert_eq!(token.len(), PAT_PREFIX.len() + 64);
        assert_ne!(
            generate(SESSION_PREFIX).unwrap(),
            generate(SESSION_PREFIX).unwrap()
        );
        assert!(well_formed(&token, PAT_PREFIX));
        assert!(!well_formed(&token, SESSION_PREFIX));
        assert!(!well_formed("su_pat_nonsense", PAT_PREFIX));
        assert!(!well_formed(
            "su_pat_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
            PAT_PREFIX
        ));
    }

    #[test]
    fn hashes_are_stable_and_length_bound() {
        assert_eq!(hash("a"), hash("a"));
        assert_ne!(hash("a"), hash("b"));
        assert_eq!(hash("").len(), 32);
    }
}
