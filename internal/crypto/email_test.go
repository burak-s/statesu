package crypto

import (
	"bytes"
	"encoding/base64"
	"strings"
	"testing"
)

func newTestCipher(t *testing.T) *EmailCipher {
	t.Helper()
	enc := bytes.Repeat([]byte{0xA5}, 32)
	mac := []byte("hmac-key-for-tests")
	c, err := NewEmailCipher(enc, mac)
	if err != nil {
		t.Fatalf("NewEmailCipher: %v", err)
	}
	return c
}

func TestNewEmailCipherValidatesKeyLengths(t *testing.T) {
	cases := []struct {
		name string
		enc  []byte
		mac  []byte
	}{
		{"short encryption key", make([]byte, 16), []byte("mac")},
		{"long encryption key", make([]byte, 64), []byte("mac")},
		{"empty hmac key", make([]byte, 32), nil},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			if _, err := NewEmailCipher(tc.enc, tc.mac); err == nil {
				t.Fatal("expected error")
			}
		})
	}
}

func TestEmailCipherEncryptDecryptRoundTrip(t *testing.T) {
	c := newTestCipher(t)
	plain := "user@example.com"
	ct, err := c.Encrypt(plain)
	if err != nil {
		t.Fatalf("Encrypt: %v", err)
	}
	if ct == "" || strings.Contains(ct, plain) {
		t.Fatalf("ciphertext leaks plaintext: %q", ct)
	}
	got, err := c.Decrypt(ct)
	if err != nil {
		t.Fatalf("Decrypt: %v", err)
	}
	if got != plain {
		t.Fatalf("round trip mismatch: got %q want %q", got, plain)
	}
}

func TestEmailCipherEncryptProducesDistinctCiphertexts(t *testing.T) {
	c := newTestCipher(t)
	a, err := c.Encrypt("same")
	if err != nil {
		t.Fatalf("Encrypt: %v", err)
	}
	b, err := c.Encrypt("same")
	if err != nil {
		t.Fatalf("Encrypt: %v", err)
	}
	if a == b {
		t.Fatal("expected nonce to make ciphertexts distinct")
	}
}

func TestEmailCipherDecryptErrors(t *testing.T) {
	c := newTestCipher(t)

	if _, err := c.Decrypt("!!!not-base64!!!"); err == nil {
		t.Fatal("expected base64 decode error")
	}

	short := base64.RawStdEncoding.EncodeToString([]byte("tiny"))
	if _, err := c.Decrypt(short); err == nil {
		t.Fatal("expected error for ciphertext shorter than nonce")
	}

	ct, err := c.Encrypt("hello")
	if err != nil {
		t.Fatalf("Encrypt: %v", err)
	}
	raw, err := base64.RawStdEncoding.DecodeString(ct)
	if err != nil {
		t.Fatalf("decode: %v", err)
	}
	raw[len(raw)-1] ^= 0xFF
	tampered := base64.RawStdEncoding.EncodeToString(raw)
	if _, err := c.Decrypt(tampered); err == nil {
		t.Fatal("expected auth failure on tampered ciphertext")
	}
}

func TestEmailCipherDecryptRejectsForeignCiphertext(t *testing.T) {
	a := newTestCipher(t)

	otherKey := bytes.Repeat([]byte{0x01}, 32)
	b, err := NewEmailCipher(otherKey, []byte("mac"))
	if err != nil {
		t.Fatalf("NewEmailCipher: %v", err)
	}
	ct, err := b.Encrypt("secret")
	if err != nil {
		t.Fatalf("Encrypt: %v", err)
	}
	if _, err := a.Decrypt(ct); err == nil {
		t.Fatal("expected decrypt to fail with different key")
	}
}

func TestEmailCipherHashIsDeterministicAndNormalized(t *testing.T) {
	c := newTestCipher(t)
	base := c.Hash("user@example.com")
	if base == "" {
		t.Fatal("empty hash")
	}
	if got := c.Hash("user@example.com"); got != base {
		t.Fatal("hash is not deterministic")
	}
	if got := c.Hash("  USER@Example.COM  "); got != base {
		t.Fatalf("expected case/whitespace normalization, got %q want %q", got, base)
	}
	if got := c.Hash("other@example.com"); got == base {
		t.Fatal("different inputs produced same hash")
	}
}

func TestEmailCipherHashDependsOnKey(t *testing.T) {
	enc := bytes.Repeat([]byte{0xA5}, 32)
	a, err := NewEmailCipher(enc, []byte("key-a"))
	if err != nil {
		t.Fatalf("NewEmailCipher: %v", err)
	}
	b, err := NewEmailCipher(enc, []byte("key-b"))
	if err != nil {
		t.Fatalf("NewEmailCipher: %v", err)
	}
	if a.Hash("x@y.com") == b.Hash("x@y.com") {
		t.Fatal("expected hmac keys to yield different hashes")
	}
}
