package crypto

import (
	"crypto/hmac"
	"crypto/sha256"
	"encoding/base64"
	"encoding/json"
	"errors"
	"strings"
	"time"
)

const (
	TokenTTL    = 30 * time.Minute
	jwtAlg      = "HS256"
	jwtType     = "JWT"
	jwtIssuer   = "statesu"
	minSecretLn = 32
)

var (
	ErrInvalidToken = errors.New("invalid token")
	ErrExpiredToken = errors.New("token expired")
)

type JWTIssuer struct {
	secret []byte
	now    func() time.Time
}

type jwtHeader struct {
	Alg string `json:"alg"`
	Typ string `json:"typ"`
}

type jwtClaims struct {
	Sub string `json:"sub"`
	Iss string `json:"iss"`
	Iat int64  `json:"iat"`
	Exp int64  `json:"exp"`
}

func NewJWTIssuer(secret []byte) (*JWTIssuer, error) {
	if len(secret) < minSecretLn {
		return nil, errors.New("jwt secret must be at least 32 bytes")
	}
	return &JWTIssuer{secret: secret, now: time.Now}, nil
}

func (j *JWTIssuer) Issue(subject string) (string, time.Time, error) {
	now := j.now().UTC()
	exp := now.Add(TokenTTL)

	header, err := encodeSegment(jwtHeader{Alg: jwtAlg, Typ: jwtType})
	if err != nil {
		return "", time.Time{}, err
	}
	payload, err := encodeSegment(jwtClaims{
		Sub: subject,
		Iss: jwtIssuer,
		Iat: now.Unix(),
		Exp: exp.Unix(),
	})
	if err != nil {
		return "", time.Time{}, err
	}

	signingInput := header + "." + payload
	sig := j.sign(signingInput)
	return signingInput + "." + sig, exp, nil
}

func (j *JWTIssuer) Verify(token string) (string, error) {
	parts := strings.Split(token, ".")
	if len(parts) != 3 {
		return "", ErrInvalidToken
	}

	expected := j.sign(parts[0] + "." + parts[1])
	if !hmac.Equal([]byte(expected), []byte(parts[2])) {
		return "", ErrInvalidToken
	}

	payload, err := base64.RawURLEncoding.DecodeString(parts[1])
	if err != nil {
		return "", ErrInvalidToken
	}
	var claims jwtClaims
	if err := json.Unmarshal(payload, &claims); err != nil {
		return "", ErrInvalidToken
	}
	if claims.Iss != jwtIssuer || claims.Sub == "" {
		return "", ErrInvalidToken
	}
	if j.now().UTC().Unix() >= claims.Exp {
		return "", ErrExpiredToken
	}
	return claims.Sub, nil
}

func (j *JWTIssuer) sign(input string) string {
	mac := hmac.New(sha256.New, j.secret)
	mac.Write([]byte(input))
	return base64.RawURLEncoding.EncodeToString(mac.Sum(nil))
}

func encodeSegment(v any) (string, error) {
	b, err := json.Marshal(v)
	if err != nil {
		return "", err
	}
	return base64.RawURLEncoding.EncodeToString(b), nil
}
