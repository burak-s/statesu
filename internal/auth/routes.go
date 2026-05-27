package auth

import "net/http"

func (h *Handler) Mount(mux *http.ServeMux) {
	mux.HandleFunc("POST /auth/register", h.Register)
	mux.HandleFunc("POST /auth/login", h.Login)
}
