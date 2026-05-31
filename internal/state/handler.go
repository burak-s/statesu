package state

import (
	"context"
	"errors"
	"net/http"
	"strconv"
	"time"

	cerr "statesu.com/internal/error"
	"statesu.com/internal/httputils"
	"statesu.com/internal/middleware"
	"statesu.com/internal/model"
)

const (
	readTimeout  = 10 * time.Second
	writeTimeout = 5 * time.Second
)

type stateService interface {
	Create(ctx context.Context, userID, text string, expiresAt time.Time) (model.State, error)
	List(ctx context.Context, f model.StateFilter, page, size int) (ListResult, error)
	Latest(ctx context.Context, f model.StateFilter) (model.State, string, error)
	Delete(ctx context.Context, stateID, userID string) error
}

type tokenVerifier interface {
	Verify(token string) (string, error)
}

type Handler struct {
	svc    stateService
	tokens tokenVerifier
}

func NewHandler(svc stateService, tokens tokenVerifier) *Handler {
	return &Handler{svc: svc, tokens: tokens}
}

func (h *Handler) Create(w http.ResponseWriter, r *http.Request) {
	userID := middleware.UserIDFromContext(r.Context())

	var req model.CreateStateRequest
	if err := httputils.DecodeJSON(r, &req); err != nil {
		httputils.WriteError(w, http.StatusBadRequest, "invalid body")
		return
	}

	ctx, cancel := context.WithTimeout(r.Context(), writeTimeout)
	defer cancel()

	st, err := h.svc.Create(ctx, userID, req.Text, time.Unix(req.ExpiresAt, 0))
	if err != nil {
		if errors.Is(err, cerr.ErrInvalidInput) {
			httputils.WriteError(w, http.StatusBadRequest, "invalid input")
			return
		}
		httputils.WriteError(w, http.StatusInternalServerError, "internal error")
		return
	}

	httputils.WriteJSON(w, http.StatusCreated, toResponse(st))
}

func (h *Handler) List(w http.ResponseWriter, r *http.Request) {
	q := r.URL.Query()

	email := q.Get("email")
	if email == "" {
		httputils.WriteError(w, http.StatusBadRequest, "missing email")
		return
	}

	page, _ := strconv.Atoi(q.Get("page"))
	size, _ := strconv.Atoi(q.Get("size"))

	ctx, cancel := context.WithTimeout(r.Context(), readTimeout)
	defer cancel()

	result, err := h.svc.List(ctx, model.StateFilter{Email: email}, page, size)
	if err != nil {
		if errors.Is(err, cerr.ErrInvalidInput) {
			httputils.WriteError(w, http.StatusBadRequest, "invalid input")
			return
		}
		httputils.WriteError(w, http.StatusInternalServerError, "internal error")
		return
	}

	items := make([]model.StateResponse, 0, len(result.Items))
	for _, st := range result.Items {
		items = append(items, toResponse(st))
	}

	httputils.WriteJSON(w, http.StatusOK, model.PaginatedStatesResponse{
		Items: items,
		Page:  result.Page,
		Size:  result.Size,
		Total: result.Total,
	})
}

func (h *Handler) Latest(w http.ResponseWriter, r *http.Request) {
	// email is optional: when supplied the lookup is scoped to that user,
	// otherwise the most recent state across all users is returned.
	filter := model.StateFilter{Email: r.URL.Query().Get("email")}

	ctx, cancel := context.WithTimeout(r.Context(), readTimeout)
	defer cancel()

	st, email, err := h.svc.Latest(ctx, filter)
	if err != nil {
		if errors.Is(err, cerr.ErrStateNotFound) {
			httputils.WriteError(w, http.StatusNotFound, "state not found")
			return
		}
		if errors.Is(err, cerr.ErrInvalidInput) {
			httputils.WriteError(w, http.StatusBadRequest, "invalid input")
			return
		}
		httputils.WriteError(w, http.StatusInternalServerError, "internal error")
		return
	}

	httputils.WriteJSON(w, http.StatusOK, model.LatestStateResponse{
		ID:        st.ID,
		UserID:    st.UserID,
		Email:     email,
		Text:      st.Text,
		CreatedAt: st.CreatedAt.Unix(),
		ExpiresAt: st.ExpiresAt.Unix(),
	})
}

func (h *Handler) Delete(w http.ResponseWriter, r *http.Request) {
	userID := middleware.UserIDFromContext(r.Context())
	stateID := r.PathValue("stateID")

	ctx, cancel := context.WithTimeout(r.Context(), writeTimeout)
	defer cancel()

	if err := h.svc.Delete(ctx, stateID, userID); err != nil {
		if errors.Is(err, cerr.ErrStateNotFound) {
			httputils.WriteError(w, http.StatusNotFound, "state not found")
			return
		}
		if errors.Is(err, cerr.ErrInvalidInput) {
			httputils.WriteError(w, http.StatusBadRequest, "invalid input")
			return
		}
		httputils.WriteError(w, http.StatusInternalServerError, "internal error")
		return
	}

	w.WriteHeader(http.StatusNoContent)
}

func toResponse(s model.State) model.StateResponse {
	return model.StateResponse{
		ID:        s.ID,
		UserID:    s.UserID,
		Text:      s.Text,
		CreatedAt: s.CreatedAt.Unix(),
		ExpiresAt: s.ExpiresAt.Unix(),
	}
}
