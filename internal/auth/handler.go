package auth

import (
	"context"
	"errors"
	"net/http"
	"time"

	cerr "statesu.com/internal/error"
	"statesu.com/internal/httputils"
	"statesu.com/internal/model"
)

const writeTimeout = 5 * time.Second

type authService interface {
	Register(ctx context.Context, email, password string) (model.User, error)
	Login(ctx context.Context, email, password string) (model.User, error)
}

type tokenIssuer interface {
	Issue(subject string) (string, time.Time, error)
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

func (h *Handler) writeAuthResponse(w http.ResponseWriter, status int, u model.User) {
	token, exp, err := h.tokens.Issue(u.ID)
	if err != nil {
		httputils.WriteError(w, http.StatusInternalServerError, "internal error")
		return
	}
	httputils.WriteJSON(w, status, model.AuthResponse{
		ID:        u.ID,
		Email:     u.Email,
		Token:     token,
		ExpiresAt: exp.Unix(),
	})
}
