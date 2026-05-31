package page

import (
	"context"
	"encoding/json"
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
		h.stateFormError(w, r, stateFormData{}, "invalid form data")
		return
	}

	userID := middleware.UserIDFromContext(r.Context())
	text := r.FormValue("text")
	ttlRaw := r.FormValue("ttl")

	ttl, err := time.ParseDuration(ttlRaw)
	if err != nil || ttl <= 0 {
		h.stateFormError(w, r, stateFormData{Text: text, TTL: ttlRaw}, "invalid expiry")
		return
	}

	if _, err := h.state.Create(context.Background(), userID, text, time.Now().Add(ttl)); err != nil {
		msg := "internal error"
		if errors.Is(err, cerr.ErrInvalidInput) {
			msg = "text is required and expiry must be within 30 days"
		}
		h.stateFormError(w, r, stateFormData{Text: text, TTL: ttlRaw}, msg)
		return
	}

	// On success, send the user back to the home page with a toast flash.
	const dest = "/?toast=State+published"
	if isHTMX(r) {
		w.Header().Set("HX-Redirect", dest)
		return
	}
	http.Redirect(w, r, dest, http.StatusSeeOther)
}

// stateFormError reports a form failure. For htmx requests it re-renders the
// form (preserving the user's input) and fires a red toast via HX-Trigger; for
// plain requests it falls back to an inline error on the full page.
func (h *Handler) stateFormError(w http.ResponseWriter, r *http.Request, data stateFormData, msg string) {
	if isHTMX(r) {
		w.Header().Set("HX-Trigger", toastTrigger(msg, "error"))
		h.view.Partial(w, "state-form", data)
		return
	}
	data.Error = msg
	h.view.Auto(w, r, "state", "state-form", data)
}

// toastTrigger builds the HX-Trigger header value the base layout listens for.
func toastTrigger(message, kind string) string {
	b, _ := json.Marshal(map[string]map[string]string{
		"toast": {"message": message, "type": kind},
	})
	return string(b)
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
