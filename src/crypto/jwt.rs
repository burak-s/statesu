use std::sync::Arc;

use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, encode};

use crate::clock::Clock;
use crate::error::{Error, InternalError};

/// Bearer tokens outlive short request bursts only; re-authentication is the
/// documented revocation-adjacent behavior.
pub const TOKEN_TTL_SECS: i64 = 30 * 60;

/// Allowed skew for `iat` values that sit slightly in the future.
const MAX_CLOCK_SKEW_SECS: i64 = 60;

pub const ISSUER: &str = "statesu";

/// HS256 bearer tokens with `sub`, `iss`, `aud`, `iat`, `exp` unix-second claims.
#[derive(Clone)]
pub struct JwtIssuer {
    encoding_key: Arc<EncodingKey>,
    decoding_key: Arc<DecodingKey>,
    audience: Arc<str>,
    clock: Arc<dyn Clock>,
}

impl JwtIssuer {
    pub fn new(jwt_secret: Vec<u8>, audience: String, clock: Arc<dyn Clock>) -> JwtIssuer {
        JwtIssuer {
            encoding_key: Arc::new(EncodingKey::from_secret(&jwt_secret)),
            decoding_key: Arc::new(DecodingKey::from_secret(&jwt_secret)),
            audience: Arc::from(audience.into_boxed_str()),
            clock,
        }
    }

    pub fn issue(&self, subject: &uuid::Uuid) -> Result<(String, i64), Error> {
        let now = self.clock.now();
        let expires_at = now
            .checked_add(TOKEN_TTL_SECS)
            .ok_or_else(|| Error::Internal(InternalError::new(time_overflow_error())))?;
        let claims = Claims {
            sub: subject.to_string(),
            iss: ISSUER.to_owned(),
            aud: self.audience.to_string(),
            iat: now,
            exp: expires_at,
        };
        let token = encode(&Header::new(Algorithm::HS256), &claims, &self.encoding_key)
            .map_err(|e| Error::Internal(InternalError::new(e)))?;
        Ok((token, expires_at))
    }

    /// Strict validation: HS256 only, exact issuer and audience, required
    /// claims, expired tokens, and future-dated issuance beyond the clock-skew
    /// tolerance. Expired and otherwise-bad tokens map to distinct errors.
    pub fn verify(&self, token: &str) -> Result<Claims, Error> {
        let mut validation = Validation::new(Algorithm::HS256);
        validation.set_issuer(&[ISSUER]);
        validation.set_audience(&[&*self.audience]);
        validation.leeway = 0;
        validation.validate_exp = false;
        validation.validate_aud = true;
        validation.validate_nbf = false;
        validation.set_required_spec_claims(&["sub", "iss", "aud", "iat", "exp"]);

        let data = decode::<Claims>(token, &self.decoding_key, &validation).map_err(|e| match e
            .kind()
        {
            jsonwebtoken::errors::ErrorKind::ExpiredSignature => Error::TokenExpired,
            _ => Error::InvalidToken,
        })?;
        let claims = data.claims;
        let now = self.clock.now();
        if claims.exp <= now {
            return Err(Error::TokenExpired);
        }
        let max_iat = now.saturating_add(MAX_CLOCK_SKEW_SECS);
        if claims.iat > max_iat {
            return Err(Error::InvalidToken);
        }
        // The signing key only exists for the current issuer's tokens.
        if claims.sub.parse::<uuid::Uuid>().is_err() {
            return Err(Error::InvalidToken);
        }
        Ok(claims)
    }
}

fn time_overflow_error() -> std::io::Error {
    std::io::Error::other("token expiry timestamp overflow")
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct Claims {
    pub sub: String,
    pub iss: String,
    pub aud: String,
    pub iat: i64,
    pub exp: i64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::TestClock;
    use base64::Engine as _;

    fn issuer(clamp: TestClock) -> JwtIssuer {
        JwtIssuer::new(vec![7u8; 32], "statesu-api".to_owned(), Arc::new(clamp))
    }

    fn test_clock() -> TestClock {
        TestClock::at_start(1_700_000_000)
    }

    #[test]
    fn issue_and_verify_round_trip() {
        let clock = test_clock();
        let jwt = issuer(clock.clone());
        let user = uuid::Uuid::now_v7();
        let (token, expires_at) = jwt.issue(&user).unwrap();
        assert_eq!(expires_at, clock.now() + TOKEN_TTL_SECS);
        let claims = jwt.verify(&token).unwrap();
        assert_eq!(claims.sub, user.to_string());
        assert_eq!(claims.iss, ISSUER);
        assert_eq!(claims.aud, "statesu-api");
        assert_eq!(claims.iat, clock.now());
        assert_eq!(claims.exp, expires_at);
    }

    #[test]
    fn tokens_from_other_issuers_are_wrong() {
        let clock = test_clock();
        let jwt = issuer(clock.clone());
        let foreign = JwtIssuer::new(
            vec![8u8; 32],
            "statesu-api".to_owned(),
            Arc::new(clock.clone()),
        );
        let (foreign_token, _) = foreign.issue(&uuid::Uuid::now_v7()).unwrap();
        assert!(matches!(
            jwt.verify(&foreign_token),
            Err(Error::InvalidToken)
        ));
        let aud_mismatch = JwtIssuer::new(
            vec![7u8; 32],
            "other-aud".to_owned(),
            Arc::new(clock.clone()),
        );
        let (other_aud, _) = aud_mismatch.issue(&uuid::Uuid::now_v7()).unwrap();
        assert!(matches!(jwt.verify(&other_aud), Err(Error::InvalidToken)));
    }

    #[test]
    fn expired_token_maps_to_token_expired() {
        let clock = test_clock();
        let jwt = issuer(clock.clone());
        let (token, _) = jwt.issue(&uuid::Uuid::now_v7()).unwrap();
        clock.advance(TOKEN_TTL_SECS + 5);
        assert!(matches!(jwt.verify(&token), Err(Error::TokenExpired)));
    }

    #[test]
    fn tampered_signature_is_invalid_token() {
        let clock = test_clock();
        let jwt = issuer(clock.clone());
        let (token, _) = jwt.issue(&uuid::Uuid::now_v7()).unwrap();
        let mut parts: Vec<String> = token.split('.').map(str::to_owned).collect();
        let mut payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(&parts[1])
            .unwrap();
        payload[0] ^= 0x40;
        parts[1] = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(payload);
        assert!(matches!(
            jwt.verify(&parts.join(".")),
            Err(Error::InvalidToken)
        ));
    }

    #[test]
    fn future_issued_token_is_invalid() {
        let clock = test_clock();
        let jwt = issuer(clock.clone());
        let (token, _) = jwt.issue(&uuid::Uuid::now_v7()).unwrap();
        clock.advance(-90);
        assert!(matches!(jwt.verify(&token), Err(Error::InvalidToken)));
        clock.advance(30); // within the 60s tolerance
        assert!(jwt.verify(&token).is_ok());
    }

    #[test]
    fn garbage_tokens_are_invalid() {
        let clock = test_clock();
        let jwt = issuer(clock);
        assert!(matches!(jwt.verify(""), Err(Error::InvalidToken)));
        assert!(matches!(jwt.verify("a.b.c"), Err(Error::InvalidToken)));
        assert!(matches!(
            jwt.verify("not-a-token"),
            Err(Error::InvalidToken)
        ));
    }

    #[test]
    fn ttl_is_thirty_minutes() {
        assert_eq!(TOKEN_TTL_SECS, 1800);
    }
}
