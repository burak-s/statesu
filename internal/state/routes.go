package state

import (
	"net/http"

	"statesu.com/internal/middleware"
)

func (h *Handler) Mount(mux *http.ServeMux) {
	auth := middleware.RequireAuth(h.tokens)
	
	mux.HandleFunc("POST /state", auth(h.Create))
	mux.HandleFunc("GET /state", h.List)
	mux.HandleFunc("GET /state/latest", h.Latest)
	mux.HandleFunc("DELETE /state/{stateID}", auth(h.Delete))
}
