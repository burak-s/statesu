package middleware

import (
	"context"
	"errors"
	"net/http"
	"strings"

	"statesu.com/internal/crypto"
	"statesu.com/internal/httputils"
)

type TokenVerifier interface {
	Verify(token string) (string, error)
}

type ctxKey int

const userIDKey ctxKey = iota

func RequireAuth(tokens TokenVerifier) func(http.HandlerFunc) http.HandlerFunc {
	return func(next http.HandlerFunc) http.HandlerFunc {
		return func(w http.ResponseWriter, r *http.Request) {
			token := TokenFromRequest(r)
			if token == "" {
				httputils.WriteError(w, http.StatusUnauthorized, "missing token")
				return
			}

			userID, err := tokens.Verify(token)
			if err != nil {
				if errors.Is(err, crypto.ErrExpiredToken) {
					httputils.WriteError(w, http.StatusUnauthorized, "token expired")
					return
				}
				httputils.WriteError(w, http.StatusUnauthorized, "invalid token")
				return
			}

			ctx := context.WithValue(r.Context(), userIDKey, userID)
			next(w, r.WithContext(ctx))
		}
	}
}

func TokenFromRequest(r *http.Request) string {
	auth := r.Header.Get("Authorization")
	if token, ok := strings.CutPrefix(auth, "Bearer "); ok && token != "" {
		return token
	}
	if cookie, err := r.Cookie("token"); err == nil {
		return cookie.Value
	}
	return ""
}

func OptionalAuth(tokens TokenVerifier, next http.HandlerFunc) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		token := TokenFromRequest(r)
		if token != "" {
			if userID, err := tokens.Verify(token); err == nil {
				ctx := context.WithValue(r.Context(), userIDKey, userID)
				r = r.WithContext(ctx)
			}
		}
		next(w, r)
	}
}

func UserIDFromContext(ctx context.Context) string {
	id, _ := ctx.Value(userIDKey).(string)
	return id
}
