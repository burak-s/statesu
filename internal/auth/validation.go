package auth

import (
	"net/mail"
	"strings"
	"unicode"

	cerr "statesu.com/internal/error"
)

const (
	minPasswordLen = 8
	maxPasswordLen = 72
	maxEmailLen    = 254
)

func normalizeEmail(email string) string {
	return strings.ToLower(strings.TrimSpace(email))
}

func validateEmail(email string) error {
	if email == "" || len(email) > maxEmailLen {
		return cerr.ErrInvalidEmail
	}
	addr, err := mail.ParseAddress(email)
	if err != nil || addr.Address != email {
		return cerr.ErrInvalidEmail
	}
	return nil
}

func validatePassword(p string) error {
	if len(p) < minPasswordLen || len(p) > maxPasswordLen {
		return cerr.ErrInvalidPassword
	}
	var hasLetter, hasDigit bool
	for _, r := range p {
		switch {
		case unicode.IsLetter(r):
			hasLetter = true
		case unicode.IsDigit(r):
			hasDigit = true
		}
	}
	if !hasLetter || !hasDigit {
		return cerr.ErrInvalidPassword
	}
	return nil
}
