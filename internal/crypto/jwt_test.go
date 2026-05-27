package crypto

import (
	"bytes"
	"encoding/base64"
	"encoding/json"
	"errors"
	"strings"
	"testing"
	"time"
)

func newTestIssuer(t *testing.T, now func() time.Time) *JWTIssuer {
	t.Helper()
	secret := bytes.Repeat([]byte{0x33}, 32)
	j, err := NewJWTIssuer(secret)
	if err != nil {
		t.Fatalf("NewJWTIssuer: %v", err)
	}
	if now != nil {
		j.now = now
	}
	return j
}

func TestNewJWTIssuerRequiresLongSecret(t *testing.T) {
	if _, err := NewJWTIssuer(make([]byte, minSecretLn-1)); err == nil {
		t.Fatal("expected error for short secret")
	}
	if _, err := NewJWTIssuer(make([]byte, minSecretLn)); err != nil {
		t.Fatalf("unexpected error for minimum-length secret: %v", err)
	}
}

func TestJWTIssueAndVerify(t *testing.T) {
	fixed := time.Date(2026, 5, 27, 12, 0, 0, 0, time.UTC)
	j := newTestIssuer(t, func() time.Time { return fixed })

	tok, exp, err := j.Issue("user-123")
	if err != nil {
		t.Fatalf("Issue: %v", err)
	}
	if want := fixed.Add(TokenTTL); !exp.Equal(want) {
		t.Fatalf("exp = %v, want %v", exp, want)
	}
	if parts := strings.Split(tok, "."); len(parts) != 3 {
		t.Fatalf("expected 3 segments, got %d", len(parts))
	}

	sub, err := j.Verify(tok)
	if err != nil {
		t.Fatalf("Verify: %v", err)
	}
	if sub != "user-123" {
		t.Fatalf("sub = %q, want %q", sub, "user-123")
	}
}

func TestJWTVerifyExpired(t *testing.T) {
	cur := time.Date(2026, 5, 27, 12, 0, 0, 0, time.UTC)
	j := newTestIssuer(t, func() time.Time { return cur })

	tok, _, err := j.Issue("u")
	if err != nil {
		t.Fatalf("Issue: %v", err)
	}

	cur = cur.Add(TokenTTL + time.Second)
	if _, err := j.Verify(tok); !errors.Is(err, ErrExpiredToken) {
		t.Fatalf("err = %v, want ErrExpiredToken", err)
	}
}

func TestJWTVerifyMalformed(t *testing.T) {
	j := newTestIssuer(t, nil)

	cases := []string{
		"",
		"only-one-segment",
		"two.segments",
		"a.b.c.d",
		"!!!.@@@.###",
	}
	for _, tok := range cases {
		t.Run(tok, func(t *testing.T) {
			if _, err := j.Verify(tok); !errors.Is(err, ErrInvalidToken) {
				t.Fatalf("err = %v, want ErrInvalidToken", err)
			}
		})
	}
}

func TestJWTVerifyTamperedSignature(t *testing.T) {
	j := newTestIssuer(t, nil)
	tok, _, err := j.Issue("u")
	if err != nil {
		t.Fatalf("Issue: %v", err)
	}
	parts := strings.Split(tok, ".")
	parts[2] = base64.RawURLEncoding.EncodeToString([]byte("not-the-real-sig"))
	bad := strings.Join(parts, ".")
	if _, err := j.Verify(bad); !errors.Is(err, ErrInvalidToken) {
		t.Fatalf("err = %v, want ErrInvalidToken", err)
	}
}

func TestJWTVerifyRejectsForeignSecret(t *testing.T) {
	a := newTestIssuer(t, nil)

	otherSecret := bytes.Repeat([]byte{0xAA}, 32)
	b, err := NewJWTIssuer(otherSecret)
	if err != nil {
		t.Fatalf("NewJWTIssuer: %v", err)
	}
	tok, _, err := b.Issue("u")
	if err != nil {
		t.Fatalf("Issue: %v", err)
	}
	if _, err := a.Verify(tok); !errors.Is(err, ErrInvalidToken) {
		t.Fatalf("err = %v, want ErrInvalidToken", err)
	}
}

func TestJWTVerifyRejectsWrongIssuerOrEmptySub(t *testing.T) {
	j := newTestIssuer(t, nil)

	build := func(claims jwtClaims) string {
		h, err := encodeSegment(jwtHeader{Alg: jwtAlg, Typ: jwtType})
		if err != nil {
			t.Fatalf("encodeSegment: %v", err)
		}
		p, err := encodeSegment(claims)
		if err != nil {
			t.Fatalf("encodeSegment: %v", err)
		}
		signing := h + "." + p
		return signing + "." + j.sign(signing)
	}

	now := time.Now().UTC()
	exp := now.Add(TokenTTL).Unix()

	wrongIss := build(jwtClaims{Sub: "u", Iss: "other", Iat: now.Unix(), Exp: exp})
	if _, err := j.Verify(wrongIss); !errors.Is(err, ErrInvalidToken) {
		t.Fatalf("wrong issuer: err = %v, want ErrInvalidToken", err)
	}

	emptySub := build(jwtClaims{Sub: "", Iss: jwtIssuer, Iat: now.Unix(), Exp: exp})
	if _, err := j.Verify(emptySub); !errors.Is(err, ErrInvalidToken) {
		t.Fatalf("empty sub: err = %v, want ErrInvalidToken", err)
	}
}

func TestJWTVerifyRejectsUndecodablePayload(t *testing.T) {
	j := newTestIssuer(t, nil)

	header, err := encodeSegment(jwtHeader{Alg: jwtAlg, Typ: jwtType})
	if err != nil {
		t.Fatalf("encodeSegment: %v", err)
	}
	payload := "!!!not-base64!!!"
	signing := header + "." + payload
	tok := signing + "." + j.sign(signing)
	if _, err := j.Verify(tok); !errors.Is(err, ErrInvalidToken) {
		t.Fatalf("err = %v, want ErrInvalidToken", err)
	}
}

func TestJWTPayloadClaimsShape(t *testing.T) {
	fixed := time.Date(2026, 5, 27, 12, 0, 0, 0, time.UTC)
	j := newTestIssuer(t, func() time.Time { return fixed })

	tok, _, err := j.Issue("alice")
	if err != nil {
		t.Fatalf("Issue: %v", err)
	}
	parts := strings.Split(tok, ".")
	raw, err := base64.RawURLEncoding.DecodeString(parts[1])
	if err != nil {
		t.Fatalf("decode payload: %v", err)
	}
	var claims jwtClaims
	if err := json.Unmarshal(raw, &claims); err != nil {
		t.Fatalf("unmarshal: %v", err)
	}
	if claims.Sub != "alice" || claims.Iss != jwtIssuer {
		t.Fatalf("unexpected claims: %+v", claims)
	}
	if claims.Iat != fixed.Unix() || claims.Exp != fixed.Add(TokenTTL).Unix() {
		t.Fatalf("unexpected timestamps: iat=%d exp=%d", claims.Iat, claims.Exp)
	}
}
