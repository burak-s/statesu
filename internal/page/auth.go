package page

import (
	"context"
	"errors"
	"net/http"

	cerr "statesu.com/internal/error"
	"statesu.com/internal/model"
)

type authFormData struct {
	Title       string
	Action      string
	Email       string
	Error       string
	AltText     string
	AltLink     string
	AltLinkText string
}

func loginForm(email, errMsg string) authFormData {
	return authFormData{
		Title:       "Login",
		Action:      "/login",
		Email:       email,
		Error:       errMsg,
		AltText:     "Don't have an account?",
		AltLink:     "/register",
		AltLinkText: "Register",
	}
}

func registerForm(email, errMsg string) authFormData {
	return authFormData{
		Title:       "Register",
		Action:      "/register",
		Email:       email,
		Error:       errMsg,
		AltText:     "Already have an account?",
		AltLink:     "/login",
		AltLinkText: "Login",
	}
}

func (h *Handler) LoginPage(w http.ResponseWriter, r *http.Request) {
	h.view.Page(w, "login", loginForm("", ""))
}

func (h *Handler) RegisterPage(w http.ResponseWriter, r *http.Request) {
	h.view.Page(w, "register", registerForm("", ""))
}

func (h *Handler) LoginForm(w http.ResponseWriter, r *http.Request) {
	if err := r.ParseForm(); err != nil {
		h.view.Auto(w, r, "login", "auth-form", loginForm("", "invalid form data"))
		return
	}

	email := r.FormValue("email")
	password := r.FormValue("password")

	u, err := h.auth.Login(context.Background(), email, password)
	if err != nil {
		msg := "internal error"
		if errors.Is(err, cerr.ErrInvalidCredentials) {
			msg = "invalid email or password"
		}
		h.view.Auto(w, r, "login", "auth-form", loginForm(email, msg))
		return
	}

	h.setAuthCookieAndRedirect(w, r, u)
}

func (h *Handler) RegisterForm(w http.ResponseWriter, r *http.Request) {
	if err := r.ParseForm(); err != nil {
		h.view.Auto(w, r, "register", "auth-form", registerForm("", "invalid form data"))
		return
	}

	email := r.FormValue("email")
	password := r.FormValue("password")

	u, err := h.auth.Register(context.Background(), email, password)
	if err != nil {
		msg := "internal error"
		switch {
		case errors.Is(err, cerr.ErrInvalidEmail):
			msg = "invalid email address"
		case errors.Is(err, cerr.ErrInvalidPassword):
			msg = "password must be 8-72 chars with at least one letter and one digit"
		case errors.Is(err, cerr.ErrUserExists):
			msg = "user already exists"
		}
		h.view.Auto(w, r, "register", "auth-form", registerForm(email, msg))
		return
	}

	h.setAuthCookieAndRedirect(w, r, u)
}

func (h *Handler) setAuthCookieAndRedirect(w http.ResponseWriter, r *http.Request, u model.User) {
	token, exp, err := h.tokens.Issue(u.ID)
	if err != nil {
		if isHTMX(r) {
			w.Header().Set("HX-Redirect", "/login")
			return
		}
		http.Redirect(w, r, "/login", http.StatusSeeOther)
		return
	}

	http.SetCookie(w, &http.Cookie{
		Name:     "token",
		Value:    token,
		Expires:  exp,
		HttpOnly: true,
		SameSite: http.SameSiteStrictMode,
		Path:     "/",
	})

	if isHTMX(r) {
		w.Header().Set("HX-Redirect", "/")
		return
	}
	http.Redirect(w, r, "/", http.StatusSeeOther)
}

func isHTMX(r *http.Request) bool {
	return r.Header.Get("HX-Request") == "true"
}
