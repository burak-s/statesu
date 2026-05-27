package page

import (
	"context"
	"errors"
	"net/http"
	"time"

	cerr "statesu.com/internal/error"
	"statesu.com/internal/middleware"
)

type stateFormData struct {
	Text  string
	TTL   string
	Error string
}

func defaultStateForm() stateFormData {
	return stateFormData{TTL: "24h"}
}

func (h *Handler) StatePage(w http.ResponseWriter, r *http.Request) {
	if !h.requireUser(w, r) {
		return
	}
	h.view.Page(w, "state", struct {
		UserID string
		stateFormData
	}{
		UserID:        middleware.UserIDFromContext(r.Context()),
		stateFormData: defaultStateForm(),
	})
}

func (h *Handler) StateForm(w http.ResponseWriter, r *http.Request) {
	if !h.requireUser(w, r) {
		return
	}

	if err := r.ParseForm(); err != nil {
		h.view.Auto(w, r, "state", "state-form", stateFormData{Error: "invalid form data"})
		return
	}

	userID := middleware.UserIDFromContext(r.Context())
	text := r.FormValue("text")
	ttlRaw := r.FormValue("ttl")

	ttl, err := time.ParseDuration(ttlRaw)
	if err != nil || ttl <= 0 {
		h.view.Auto(w, r, "state", "state-form", stateFormData{
			Text: text, TTL: ttlRaw, Error: "invalid expiry",
		})
		return
	}

	if _, err := h.state.Create(context.Background(), userID, text, time.Now().Add(ttl)); err != nil {
		msg := "internal error"
		if errors.Is(err, cerr.ErrInvalidInput) {
			msg = "text is required and expiry must be within 30 days"
		}
		h.view.Auto(w, r, "state", "state-form", stateFormData{
			Text: text, TTL: ttlRaw, Error: msg,
		})
		return
	}

	if isHTMX(r) {
		w.Header().Set("HX-Trigger", "state-updated")
		h.view.Partial(w, "state-form", defaultStateForm())
		return
	}
	http.Redirect(w, r, "/", http.StatusSeeOther)
}

func (h *Handler) requireUser(w http.ResponseWriter, r *http.Request) bool {
	if middleware.UserIDFromContext(r.Context()) != "" {
		return true
	}
	if isHTMX(r) {
		w.Header().Set("HX-Redirect", "/login")
	} else {
		http.Redirect(w, r, "/login", http.StatusSeeOther)
	}
	return false
}
