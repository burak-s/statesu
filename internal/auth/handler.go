package auth

import (
	"context"
	"errors"
	"net/http"
	"time"

	cerr "statesu.com/internal/error"
	"statesu.com/internal/httputils"
	"statesu.com/internal/middleware"
	"statesu.com/internal/model"
)

const writeTimeout = 5 * time.Second

type authService interface {
	Register(ctx context.Context, email, password string) (model.User, error)
	Login(ctx context.Context, email, password string) (model.User, error)
	Me(ctx context.Context, userID string) (model.User, error)
}

type tokenIssuer interface {
	Issue(subject string) (string, time.Time, error)
	Verify(token string) (string, error)
}

type Handler struct {
	svc    authService
	tokens tokenIssuer
}

func NewHandler(svc authService, tokens tokenIssuer) *Handler {
	return &Handler{svc: svc, tokens: tokens}
}

func (h *Handler) Register(w http.ResponseWriter, r *http.Request) {
	var req model.CredentialsRequest
	if err := httputils.DecodeJSON(r, &req); err != nil {
		httputils.WriteError(w, http.StatusBadRequest, "invalid body")
		return
	}

	ctx, cancel := context.WithTimeout(r.Context(), writeTimeout)
	defer cancel()

	u, err := h.svc.Register(ctx, req.Email, req.Password)
	if err != nil {
		switch {
		case errors.Is(err, cerr.ErrInvalidEmail):
			httputils.WriteError(w, http.StatusBadRequest, "invalid email")
		case errors.Is(err, cerr.ErrInvalidPassword):
			httputils.WriteError(w, http.StatusBadRequest, "password must be 8-72 chars with at least one letter and one digit")
		case errors.Is(err, cerr.ErrUserExists):
			httputils.WriteError(w, http.StatusConflict, "user already exists")
		default:
			httputils.WriteError(w, http.StatusInternalServerError, "internal error")
		}
		return
	}

	h.writeAuthResponse(w, http.StatusCreated, u)
}

func (h *Handler) Login(w http.ResponseWriter, r *http.Request) {
	var req model.CredentialsRequest
	if err := httputils.DecodeJSON(r, &req); err != nil {
		httputils.WriteError(w, http.StatusBadRequest, "invalid body")
		return
	}

	ctx, cancel := context.WithTimeout(r.Context(), writeTimeout)
	defer cancel()

	u, err := h.svc.Login(ctx, req.Email, req.Password)
	if err != nil {
		if errors.Is(err, cerr.ErrInvalidCredentials) {
			httputils.WriteError(w, http.StatusUnauthorized, "invalid credentials")
			return
		}
		httputils.WriteError(w, http.StatusInternalServerError, "internal error")
		return
	}

	h.writeAuthResponse(w, http.StatusOK, u)
}

// Me reports the authenticated user. The browser SPA uses it to restore its
// session from the HttpOnly auth cookie; API clients may also call it with a
// bearer token.
func (h *Handler) Me(w http.ResponseWriter, r *http.Request) {
	token := middleware.TokenFromRequest(r)
	if token == "" {
		httputils.WriteError(w, http.StatusUnauthorized, "missing token")
		return
	}

	userID, err := h.tokens.Verify(token)
	if err != nil {
		httputils.WriteError(w, http.StatusUnauthorized, "invalid token")
		return
	}

	u, err := h.svc.Me(r.Context(), userID)
	if err != nil {
		httputils.WriteError(w, http.StatusUnauthorized, "invalid token")
		return
	}

	httputils.WriteJSON(w, http.StatusOK, map[string]string{"id": u.ID, "email": u.Email})
}

// Logout clears the auth cookie so the SPA session ends.
func (h *Handler) Logout(w http.ResponseWriter, r *http.Request) {
	http.SetCookie(w, &http.Cookie{
		Name:     "token",
		Value:    "",
		MaxAge:   -1,
		HttpOnly: true,
		SameSite: http.SameSiteStrictMode,
		Path:     "/",
	})
	w.WriteHeader(http.StatusNoContent)
}

func (h *Handler) writeAuthResponse(w http.ResponseWriter, status int, u model.User) {
	token, exp, err := h.tokens.Issue(u.ID)
	if err != nil {
		httputils.WriteError(w, http.StatusInternalServerError, "internal error")
		return
	}

	// Mirror the token into an HttpOnly cookie so the browser SPA can
	// authenticate without touching the token. API clients keep using the
	// bearer token returned in the body.
	http.SetCookie(w, &http.Cookie{
		Name:     "token",
		Value:    token,
		Expires:  exp,
		HttpOnly: true,
		SameSite: http.SameSiteStrictMode,
		Path:     "/",
	})

	httputils.WriteJSON(w, status, model.AuthResponse{
		ID:        u.ID,
		Email:     u.Email,
		Token:     token,
		ExpiresAt: exp.Unix(),
	})
}
