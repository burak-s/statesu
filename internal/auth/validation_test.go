package auth

import (
	"errors"
	"strings"
	"testing"

	cerr "statesu.com/internal/error"
)

func TestNormalizeEmail(t *testing.T) {
	cases := []struct {
		in   string
		want string
	}{
		{"user@example.com", "user@example.com"},
		{"  user@example.com  ", "user@example.com"},
		{"USER@Example.COM", "user@example.com"},
		{"\tMiXeD@Case.IO\n", "mixed@case.io"},
		{"", ""},
		{"   ", ""},
	}
	for _, tc := range cases {
		t.Run(tc.in, func(t *testing.T) {
			if got := normalizeEmail(tc.in); got != tc.want {
				t.Fatalf("normalizeEmail(%q) = %q, want %q", tc.in, got, tc.want)
			}
		})
	}
}

func TestValidateEmailValid(t *testing.T) {
	cases := []string{
		"user@example.com",
		"a@b.co",
		"first.last@sub.example.org",
		"user+tag@example.com",
	}
	for _, e := range cases {
		t.Run(e, func(t *testing.T) {
			if err := validateEmail(e); err != nil {
				t.Fatalf("validateEmail(%q) = %v, want nil", e, err)
			}
		})
	}
}

func TestValidateEmailInvalid(t *testing.T) {
	long := strings.Repeat("a", maxEmailLen-len("@example.com")+1) + "@example.com"

	cases := map[string]string{
		"empty":             "",
		"too long":          long,
		"missing @":         "userexample.com",
		"missing local":     "@example.com",
		"missing domain":    "user@",
		"spaces inside":     "us er@example.com",
		"display name form": "User <user@example.com>",
		"trailing junk":     "user@example.com extra",
		"just text":         "not-an-email",
	}
	for name, in := range cases {
		t.Run(name, func(t *testing.T) {
			err := validateEmail(in)
			if !errors.Is(err, cerr.ErrInvalidEmail) {
				t.Fatalf("validateEmail(%q) = %v, want ErrInvalidEmail", in, err)
			}
		})
	}
}

func TestValidateEmailMaxLengthBoundary(t *testing.T) {
	local := strings.Repeat("a", maxEmailLen-len("@example.com"))
	atMax := local + "@example.com"
	if len(atMax) != maxEmailLen {
		t.Fatalf("setup: len=%d, want %d", len(atMax), maxEmailLen)
	}
	if err := validateEmail(atMax); err != nil {
		t.Fatalf("expected email at max length to be accepted, got %v", err)
	}
}

func TestValidatePasswordValid(t *testing.T) {
	cases := []string{
		"abcdefg1",
		"Pa55word",
		"longer-password-with-1-digit",
		strings.Repeat("a", maxPasswordLen-1) + "1",
	}
	for _, p := range cases {
		t.Run(p, func(t *testing.T) {
			if err := validatePassword(p); err != nil {
				t.Fatalf("validatePassword(%q) = %v, want nil", p, err)
			}
		})
	}
}

func TestValidatePasswordInvalid(t *testing.T) {
	cases := map[string]string{
		"too short":      "ab1",
		"empty":          "",
		"too long":       strings.Repeat("a1", maxPasswordLen),
		"letters only":   "abcdefgh",
		"digits only":    "12345678",
		"symbols only":   "!@#$%^&*",
		"digits+symbols": "1234567!",
		"letters+spaces": "abcdefg ",
	}
	for name, in := range cases {
		t.Run(name, func(t *testing.T) {
			err := validatePassword(in)
			if !errors.Is(err, cerr.ErrInvalidPassword) {
				t.Fatalf("validatePassword(%q) = %v, want ErrInvalidPassword", in, err)
			}
		})
	}
}

func TestValidatePasswordLengthBoundaries(t *testing.T) {
	atMin := strings.Repeat("a", minPasswordLen-1) + "1"
	if len(atMin) != minPasswordLen {
		t.Fatalf("setup: len=%d, want %d", len(atMin), minPasswordLen)
	}
	if err := validatePassword(atMin); err != nil {
		t.Fatalf("expected min-length password to be accepted, got %v", err)
	}

	belowMin := strings.Repeat("a", minPasswordLen-2) + "1"
	if err := validatePassword(belowMin); !errors.Is(err, cerr.ErrInvalidPassword) {
		t.Fatalf("expected below-min password to be rejected, got %v", err)
	}

	atMax := strings.Repeat("a", maxPasswordLen-1) + "1"
	if len(atMax) != maxPasswordLen {
		t.Fatalf("setup: len=%d, want %d", len(atMax), maxPasswordLen)
	}
	if err := validatePassword(atMax); err != nil {
		t.Fatalf("expected max-length password to be accepted, got %v", err)
	}

	overMax := strings.Repeat("a", maxPasswordLen) + "1"
	if err := validatePassword(overMax); !errors.Is(err, cerr.ErrInvalidPassword) {
		t.Fatalf("expected over-max password to be rejected, got %v", err)
	}
}

func TestValidatePasswordUnicodeLettersAndDigits(t *testing.T) {
	if err := validatePassword("şifrelik1"); err != nil {
		t.Fatalf("expected unicode letter + digit to validate, got %v", err)
	}
	if err := validatePassword("١٢٣٤٥٦٧abc"); err != nil {
		t.Fatalf("expected unicode digits + letters to validate, got %v", err)
	}
}
