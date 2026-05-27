package model

type User struct {
	ID        string `json:"id"`
	Email     string `json:"email"`
	EmailHash string `json:"-"`
	Password  string `json:"-"`
}

type CredentialsRequest struct {
	Email    string `json:"email"`
	Password string `json:"password"`
}

type AuthResponse struct {
	ID        string `json:"id"`
	Email     string `json:"email"`
	Token     string `json:"token"`
	ExpiresAt int64  `json:"expires_at"`
}

type ErrorResponse struct {
	Error string `json:"error"`
}
