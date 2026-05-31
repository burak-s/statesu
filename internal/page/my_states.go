package page

import (
	"context"
	"errors"
	"net/http"
	"strconv"
	"time"

	cerr "statesu.com/internal/error"
	"statesu.com/internal/middleware"
	"statesu.com/internal/model"
)

type stateItem struct {
	ID        string
	Text      string
	CreatedAt string
	ExpiresAt string
	Expired   bool
}

type myStatesData struct {
	UserID   string
	Items    []stateItem
	Page     int
	PrevPage int
	NextPage int
	HasPrev  bool
	HasNext  bool
}

func (h *Handler) MyStates(w http.ResponseWriter, r *http.Request) {
	userID := middleware.UserIDFromContext(r.Context())

	page, _ := strconv.Atoi(r.URL.Query().Get("page"))
	if page < 1 {
		page = 1
	}

	ctx, cancel := context.WithTimeout(r.Context(), 10*time.Second)
	defer cancel()

	result, err := h.state.List(ctx, model.StateFilter{UserID: userID}, page, 20)
	if err != nil {
		http.Error(w, "internal error", http.StatusInternalServerError)
		return
	}

	now := time.Now()
	items := make([]stateItem, 0, len(result.Items))
	for _, s := range result.Items {
		items = append(items, stateItem{
			ID:        s.ID,
			Text:      s.Text,
			CreatedAt: formatRelativeTime(s.CreatedAt),
			ExpiresAt: formatExpiresAt(s.ExpiresAt, now),
			Expired:   s.ExpiresAt.Before(now),
		})
	}

	hasNext := result.Page*result.Size < result.Total
	h.view.Page(w, "my-states", myStatesData{
		UserID:   userID,
		Items:    items,
		Page:     result.Page,
		PrevPage: result.Page - 1,
		NextPage: result.Page + 1,
		HasPrev:  result.Page > 1,
		HasNext:  hasNext,
	})
}

func (h *Handler) DeleteState(w http.ResponseWriter, r *http.Request) {
	userID := middleware.UserIDFromContext(r.Context())
	stateID := r.PathValue("stateID")

	ctx, cancel := context.WithTimeout(r.Context(), 5*time.Second)
	defer cancel()

	if err := h.state.Delete(ctx, stateID, userID); err != nil {
		if errors.Is(err, cerr.ErrStateNotFound) {
			http.Error(w, "not found", http.StatusNotFound)
			return
		}
		http.Error(w, "internal error", http.StatusInternalServerError)
		return
	}

	w.Header().Set("HX-Trigger", `{"toast":{"message":"State deleted","type":"success"}}`)
	w.WriteHeader(http.StatusOK)
}

func formatExpiresAt(t time.Time, now time.Time) string {
	if t.Before(now) {
		return "expired"
	}
	d := t.Sub(now)
	switch {
	case d < time.Hour:
		m := int(d.Minutes())
		if m <= 1 {
			return "expires in 1 minute"
		}
		return "expires in " + strconv.Itoa(m) + " minutes"
	case d < 24*time.Hour:
		h := int(d.Hours())
		if h == 1 {
			return "expires in 1 hour"
		}
		return "expires in " + strconv.Itoa(h) + " hours"
	case d < 48*time.Hour:
		return "expires tomorrow"
	default:
		return "expires " + t.Format("Jan 2, 2006")
	}
}
