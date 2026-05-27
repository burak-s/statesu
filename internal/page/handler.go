package page

import (
	"context"
	"fmt"
	"net/http"
	"strings"
	"time"

	"statesu.com/internal/middleware"
	"statesu.com/internal/model"
	"statesu.com/internal/view"
)

type Tokens interface {
	Verify(token string) (string, error)
	Issue(subject string) (string, time.Time, error)
}

type AuthService interface {
	Login(ctx context.Context, email, password string) (model.User, error)
	Register(ctx context.Context, email, password string) (model.User, error)
}

type StateService interface {
	Create(ctx context.Context, userID, text string, expiresAt time.Time) (model.State, error)
	Latest(ctx context.Context) (model.State, string, error)
}

type Handler struct {
	tokens Tokens
	auth   AuthService
	state  StateService
	view   *view.Renderer
}

func NewHandler(tokens Tokens, auth AuthService, state StateService, view *view.Renderer) *Handler {
	return &Handler{tokens: tokens, auth: auth, state: state, view: view}
}

func (h *Handler) Mount(mux *http.ServeMux) {
	mux.HandleFunc("GET /", middleware.OptionalAuth(h.tokens, h.Index))
	mux.HandleFunc("GET /latest-feed", h.LatestFeed)
	mux.HandleFunc("POST /logout", h.Logout)

	mux.HandleFunc("GET /login", h.LoginPage)
	mux.HandleFunc("POST /login", h.LoginForm)
	mux.HandleFunc("GET /register", h.RegisterPage)
	mux.HandleFunc("POST /register", h.RegisterForm)

	mux.HandleFunc("GET /state/new", middleware.OptionalAuth(h.tokens, h.StatePage))
	mux.HandleFunc("POST /state/new", middleware.OptionalAuth(h.tokens, h.StateForm))
}

type latestStateData struct {
	Initial   string
	Email     string
	Text      string
	CreatedAt string
	Found     bool
}

type indexData struct {
	UserID      string
	StateForm   stateFormData
	LatestState latestStateData
}

func (h *Handler) Index(w http.ResponseWriter, r *http.Request) {
	userID := middleware.UserIDFromContext(r.Context())
	data := indexData{
		UserID:      userID,
		StateForm:   defaultStateForm(),
		LatestState: h.fetchLatest(),
	}
	h.view.Page(w, "index", data)
}

func (h *Handler) LatestFeed(w http.ResponseWriter, r *http.Request) {
	h.view.Partial(w, "latest-state", h.fetchLatest())
}

func (h *Handler) fetchLatest() latestStateData {
	st, email, err := h.state.Latest(context.Background())
	if err != nil {
		return latestStateData{}
	}

	initial := strings.ToUpper(strings.TrimSpace(email))[0:1]

	return latestStateData{
		Initial:   initial,
		Email:     email,
		Text:      st.Text,
		CreatedAt: formatRelativeTime(st.CreatedAt),
		Found:     true,
	}
}

func (h *Handler) Logout(w http.ResponseWriter, r *http.Request) {
	http.SetCookie(w, &http.Cookie{
		Name:     "token",
		Value:    "",
		MaxAge:   -1,
		HttpOnly: true,
		Path:     "/",
	})
	w.Header().Set("HX-Redirect", "/")
}

func formatRelativeTime(t time.Time) string {
	d := time.Since(t)
	switch {
	case d < time.Minute:
		return "just now"
	case d < time.Hour:
		m := int(d.Minutes())
		if m == 1 {
			return "1 minute ago"
		}
		return fmt.Sprintf("%d minutes ago", m)
	case d < 24*time.Hour:
		h := int(d.Hours())
		if h == 1 {
			return "1 hour ago"
		}
		return fmt.Sprintf("%d hours ago", h)
	case d < 48*time.Hour:
		return "yesterday"
	case d < 7*24*time.Hour:
		d := int(d.Hours() / 24)
		return fmt.Sprintf("%d days ago", d)
	default:
		return t.Format("Jan 2, 2006")
	}
}
