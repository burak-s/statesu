package auth_test

import (
	"database/sql"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"statesu.com/internal/auth"
	"statesu.com/internal/config"
	"statesu.com/internal/crypto"
	"statesu.com/internal/model"
)

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

func testCipher(t *testing.T) *crypto.EmailCipher {
	t.Helper()
	c, err := crypto.NewEmailCipher(
		[]byte("01234567890123456789012345678901"), // 32 bytes
		[]byte("hmac-key-for-testing-purposes-ok"),
	)
	if err != nil {
		t.Fatalf("new email cipher: %v", err)
	}
	return c
}

func testJWT(t *testing.T) *crypto.JWTIssuer {
	t.Helper()
	j, err := crypto.NewJWTIssuer(
		[]byte("this-is-a-test-jwt-secret-key!!!"),
	)
	if err != nil {
		t.Fatalf("new jwt issuer: %v", err)
	}
	return j
}

func testDB(t *testing.T) *sql.DB {
	t.Helper()
	db, err := sql.Open("sqlite", ":memory:?_pragma=foreign_keys(1)")
	if err != nil {
		t.Fatalf("open db: %v", err)
	}
	db.SetMaxOpenConns(1)
	if _, err := db.Exec(config.Schema); err != nil {
		t.Fatalf("create tables: %v", err)
	}
	return db
}

// newTestServer wires the full auth stack (real db, real crypto, real http)
// and returns a running httptest.Server.
func newTestServer(t *testing.T) *httptest.Server {
	t.Helper()
	db := testDB(t)
	t.Cleanup(func() { db.Close() })

	cipher := testCipher(t)
	jwt := testJWT(t)

	repo := auth.NewRepository(db)
	svc := auth.NewService(repo, cipher)
	handler := auth.NewHandler(svc, jwt)

	mux := http.NewServeMux()
	handler.Mount(mux)

	return httptest.NewServer(mux)
}

func postJSON(t *testing.T, url, body string) *http.Response {
	t.Helper()
	resp, err := http.Post(url, "application/json", strings.NewReader(body))
	if err != nil {
		t.Fatalf("post: %v", err)
	}
	return resp
}

func decodeBody(t *testing.T, resp *http.Response, v any) {
	t.Helper()
	defer resp.Body.Close()
	if err := json.NewDecoder(resp.Body).Decode(v); err != nil {
		t.Fatalf("decode: %v", err)
	}
}

func authCookie(t *testing.T, resp *http.Response) *http.Cookie {
	t.Helper()
	for _, c := range resp.Cookies() {
		if c.Name == "token" {
			return c
		}
	}
	t.Fatal("expected token cookie to be set")
	return nil
}

// ---------------------------------------------------------------------------
// Register – success
// ---------------------------------------------------------------------------

func TestRegister_Success(t *testing.T) {
	srv := newTestServer(t)
	defer srv.Close()

	resp := postJSON(t, srv.URL+"/auth/register",
		`{"email":"test@example.com","password":"mypassword123"}`)

	if resp.StatusCode != http.StatusCreated {
		t.Fatalf("expected 201, got %d", resp.StatusCode)
	}

	var ar model.AuthResponse
	decodeBody(t, resp, &ar)

	if ar.ID == "" {
		t.Error("expected non-empty id")
	}
	if ar.Email != "test@example.com" {
		t.Errorf("expected email test@example.com, got %q", ar.Email)
	}
	if ar.Token == "" {
		t.Error("expected non-empty token")
	}
	if ar.ExpiresAt == 0 {
		t.Error("expected non-zero expires_at")
	}
}

// ---------------------------------------------------------------------------
// Register – invalid email (table-driven)
// ---------------------------------------------------------------------------

func TestRegister_InvalidEmail(t *testing.T) {
	srv := newTestServer(t)
	defer srv.Close()

	tests := []struct {
		name  string
		email string
	}{
		{"empty", ""},
		{"no at", "notanemail"},
		{"too long", strings.Repeat("a", 249) + "@x.com"}, // 255 chars total, > 254
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			body := `{"email":"` + tt.email + `","password":"validpass1"}`
			resp := postJSON(t, srv.URL+"/auth/register", body)

			if resp.StatusCode != http.StatusBadRequest {
				t.Errorf("expected 400, got %d", resp.StatusCode)
			}
			var er model.ErrorResponse
			decodeBody(t, resp, &er)
			if er.Error == "" {
				t.Error("expected error message")
			}
		})
	}
}

// ---------------------------------------------------------------------------
// Register – invalid password (table-driven)
// ---------------------------------------------------------------------------

func TestRegister_InvalidPassword(t *testing.T) {
	srv := newTestServer(t)
	defer srv.Close()

	tests := []struct {
		name string
		pass string
	}{
		{"empty", ""},
		{"too short (7 chars)", "Abcde1f"},
		{"no letter", "12345678"},
		{"no digit", "abcdefgh"},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			body := `{"email":"test@example.com","password":"` + tt.pass + `"}`
			resp := postJSON(t, srv.URL+"/auth/register", body)

			if resp.StatusCode != http.StatusBadRequest {
				t.Errorf("expected 400, got %d", resp.StatusCode)
			}
			var er model.ErrorResponse
			decodeBody(t, resp, &er)
			if er.Error == "" {
				t.Error("expected error message")
			}
		})
	}
}

// ---------------------------------------------------------------------------
// Register – duplicate email
// ---------------------------------------------------------------------------

func TestRegister_Duplicate(t *testing.T) {
	srv := newTestServer(t)
	defer srv.Close()

	body := `{"email":"dup@example.com","password":"mypassword123"}`

	// First registration should succeed.
	resp1 := postJSON(t, srv.URL+"/auth/register", body)
	if resp1.StatusCode != http.StatusCreated {
		t.Fatalf("first register: expected 201, got %d", resp1.StatusCode)
	}
	resp1.Body.Close()

	// Second registration with the same email should conflict.
	resp2 := postJSON(t, srv.URL+"/auth/register", body)
	if resp2.StatusCode != http.StatusConflict {
		t.Errorf("expected 409, got %d", resp2.StatusCode)
	}
	var er model.ErrorResponse
	decodeBody(t, resp2, &er)
	if er.Error == "" {
		t.Error("expected error message")
	}
}

// ---------------------------------------------------------------------------
// Register – invalid JSON body
// ---------------------------------------------------------------------------

func TestRegister_InvalidJSON(t *testing.T) {
	srv := newTestServer(t)
	defer srv.Close()

	resp := postJSON(t, srv.URL+"/auth/register", `not json`)

	if resp.StatusCode != http.StatusBadRequest {
		t.Errorf("expected 400, got %d", resp.StatusCode)
	}
}

// ---------------------------------------------------------------------------
// Register – email normalization (case-insensitive)
// ---------------------------------------------------------------------------

func TestRegister_EmailNormalization(t *testing.T) {
	srv := newTestServer(t)
	defer srv.Close()

	// Register with uppercase email.
	resp1 := postJSON(t, srv.URL+"/auth/register",
		`{"email":"UPPER@Example.COM","password":"mypassword123"}`)
	if resp1.StatusCode != http.StatusCreated {
		t.Fatalf("first register: expected 201, got %d", resp1.StatusCode)
	}
	var ar1 model.AuthResponse
	decodeBody(t, resp1, &ar1)
	if ar1.Email != "upper@example.com" {
		t.Errorf("expected normalized email, got %q", ar1.Email)
	}

	// Register with lowercase version should conflict.
	resp2 := postJSON(t, srv.URL+"/auth/register",
		`{"email":"upper@example.com","password":"mypassword123"}`)
	if resp2.StatusCode != http.StatusConflict {
		t.Errorf("expected 409 for normalized duplicate, got %d", resp2.StatusCode)
	}
	resp2.Body.Close()
}

// ---------------------------------------------------------------------------
// Login – success
// ---------------------------------------------------------------------------

func TestLogin_Success(t *testing.T) {
	srv := newTestServer(t)
	defer srv.Close()

	// Register first.
	resp1 := postJSON(t, srv.URL+"/auth/register",
		`{"email":"login@example.com","password":"mypassword123"}`)
	if resp1.StatusCode != http.StatusCreated {
		t.Fatalf("register: expected 201, got %d", resp1.StatusCode)
	}
	resp1.Body.Close()

	// Login with same credentials.
	resp2 := postJSON(t, srv.URL+"/auth/login",
		`{"email":"login@example.com","password":"mypassword123"}`)
	if resp2.StatusCode != http.StatusOK {
		t.Fatalf("expected 200, got %d", resp2.StatusCode)
	}

	var ar model.AuthResponse
	decodeBody(t, resp2, &ar)

	if ar.ID == "" {
		t.Error("expected non-empty id")
	}
	if ar.Email != "login@example.com" {
		t.Errorf("expected email login@example.com, got %q", ar.Email)
	}
	if ar.Token == "" {
		t.Error("expected non-empty token")
	}
	if ar.ExpiresAt == 0 {
		t.Error("expected non-zero expires_at")
	}
}

// ---------------------------------------------------------------------------
// Login – wrong password
// ---------------------------------------------------------------------------

func TestLogin_WrongPassword(t *testing.T) {
	srv := newTestServer(t)
	defer srv.Close()

	postJSON(t, srv.URL+"/auth/register",
		`{"email":"wp@example.com","password":"correctpass1"}`)

	resp := postJSON(t, srv.URL+"/auth/login",
		`{"email":"wp@example.com","password":"wrongpassword1"}`)

	if resp.StatusCode != http.StatusUnauthorized {
		t.Errorf("expected 401, got %d", resp.StatusCode)
	}
	var er model.ErrorResponse
	decodeBody(t, resp, &er)
	if er.Error == "" {
		t.Error("expected error message")
	}
}

// ---------------------------------------------------------------------------
// Login – unknown email
// ---------------------------------------------------------------------------

func TestLogin_UnknownEmail(t *testing.T) {
	srv := newTestServer(t)
	defer srv.Close()

	resp := postJSON(t, srv.URL+"/auth/login",
		`{"email":"noone@example.com","password":"mypassword123"}`)

	if resp.StatusCode != http.StatusUnauthorized {
		t.Errorf("expected 401, got %d", resp.StatusCode)
	}
	var er model.ErrorResponse
	decodeBody(t, resp, &er)
	if er.Error == "" {
		t.Error("expected error message")
	}
}

// ---------------------------------------------------------------------------
// Login – empty credentials
// ---------------------------------------------------------------------------

func TestLogin_EmptyCredentials(t *testing.T) {
	srv := newTestServer(t)
	defer srv.Close()

	tests := []struct {
		name  string
		email string
		pass  string
	}{
		{"empty email", "", "validpass1"},
		{"empty password", "test@example.com", ""},
		{"both empty", "", ""},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			body := `{"email":"` + tt.email + `","password":"` + tt.pass + `"}`
			resp := postJSON(t, srv.URL+"/auth/login", body)
			if resp.StatusCode != http.StatusUnauthorized {
				t.Errorf("expected 401, got %d", resp.StatusCode)
			}
			resp.Body.Close()
		})
	}
}

// ---------------------------------------------------------------------------
// JWT – verify tokens from register and login
// ---------------------------------------------------------------------------

func TestJWT_TokensAreValid(t *testing.T) {
	jwt := testJWT(t)
	srv := newTestServer(t)
	defer srv.Close()

	// Register.
	resp := postJSON(t, srv.URL+"/auth/register",
		`{"email":"jwt@example.com","password":"mypassword123"}`)
	if resp.StatusCode != http.StatusCreated {
		t.Fatalf("register: expected 201, got %d", resp.StatusCode)
	}
	var ar model.AuthResponse
	decodeBody(t, resp, &ar)

	// Verify the returned token is valid.
	userID, err := jwt.Verify(ar.Token)
	if err != nil {
		t.Fatalf("verify token: %v", err)
	}
	if userID != ar.ID {
		t.Errorf("token subject %q != response id %q", userID, ar.ID)
	}
}

func TestJWT_RegisterAndLoginTokensDiffer(t *testing.T) {
	srv := newTestServer(t)
	defer srv.Close()

	// Register.
	resp1 := postJSON(t, srv.URL+"/auth/register",
		`{"email":"diff@example.com","password":"mypassword123"}`)
	var ar1 model.AuthResponse
	decodeBody(t, resp1, &ar1)

	// Login.
	resp2 := postJSON(t, srv.URL+"/auth/login",
		`{"email":"diff@example.com","password":"mypassword123"}`)
	var ar2 model.AuthResponse
	decodeBody(t, resp2, &ar2)

	// Both should verify to the same user ID.
	jwt := testJWT(t)
	id1, err := jwt.Verify(ar1.Token)
	if err != nil {
		t.Fatalf("verify register token: %v", err)
	}
	id2, err := jwt.Verify(ar2.Token)
	if err != nil {
		t.Fatalf("verify login token: %v", err)
	}
	if id1 != ar1.ID || id2 != ar2.ID {
		t.Errorf("token subjects don't match: %q vs %q, %q vs %q", id1, ar1.ID, id2, ar2.ID)
	}
}

// ---------------------------------------------------------------------------
// E2E – full round-trip: register → login → verify both tokens
// ---------------------------------------------------------------------------

func TestE2E_FullAuthFlow(t *testing.T) {
	jwt := testJWT(t)
	srv := newTestServer(t)
	defer srv.Close()

	// 1. Register.
	regResp := postJSON(t, srv.URL+"/auth/register",
		`{"email":"e2e@example.com","password":"e2eflowpass1"}`)
	if regResp.StatusCode != http.StatusCreated {
		t.Fatalf("register: expected 201, got %d", regResp.StatusCode)
	}
	var reg model.AuthResponse
	decodeBody(t, regResp, &reg)

	if _, err := jwt.Verify(reg.Token); err != nil {
		t.Fatalf("register token invalid: %v", err)
	}

	// 2. Login.
	loginResp := postJSON(t, srv.URL+"/auth/login",
		`{"email":"e2e@example.com","password":"e2eflowpass1"}`)
	if loginResp.StatusCode != http.StatusOK {
		t.Fatalf("login: expected 200, got %d", loginResp.StatusCode)
	}
	var login model.AuthResponse
	decodeBody(t, loginResp, &login)

	if _, err := jwt.Verify(login.Token); err != nil {
		t.Fatalf("login token invalid: %v", err)
	}

	// 3. Both tokens belong to the same user.
	if reg.ID != login.ID {
		t.Errorf("user ids differ: register=%q login=%q", reg.ID, login.ID)
	}
	if reg.Email != "e2e@example.com" || login.Email != "e2e@example.com" {
		t.Errorf("unexpected emails: register=%q login=%q", reg.Email, login.Email)
	}
}

// ---------------------------------------------------------------------------
// SPA session: cookie auth, /auth/me and /auth/logout
// ---------------------------------------------------------------------------

func TestRegister_SetsHttpOnlyCookie(t *testing.T) {
	srv := newTestServer(t)
	defer srv.Close()

	resp := postJSON(t, srv.URL+"/auth/register",
		`{"email":"cookie@example.com","password":"mypassword123"}`)
	if resp.StatusCode != http.StatusCreated {
		t.Fatalf("expected 201, got %d", resp.StatusCode)
	}

	cookie := authCookie(t, resp)
	resp.Body.Close()

	if cookie.Value == "" {
		t.Error("expected a non-empty token cookie")
	}
	if !cookie.HttpOnly {
		t.Error("expected the token cookie to be HttpOnly")
	}
}

func TestMe_WithCookie(t *testing.T) {
	srv := newTestServer(t)
	defer srv.Close()

	resp := postJSON(t, srv.URL+"/auth/register",
		`{"email":"me@example.com","password":"mypassword123"}`)
	var registered model.AuthResponse
	decodeBody(t, resp, &registered)
	cookie := authCookie(t, resp)

	req, _ := http.NewRequest(http.MethodGet, srv.URL+"/auth/me", nil)
	req.AddCookie(cookie)
	meResp, err := http.DefaultClient.Do(req)
	if err != nil {
		t.Fatalf("get /auth/me: %v", err)
	}

	if meResp.StatusCode != http.StatusOK {
		t.Fatalf("expected 200, got %d", meResp.StatusCode)
	}

	var me struct {
		ID    string `json:"id"`
		Email string `json:"email"`
	}
	decodeBody(t, meResp, &me)
	if me.ID != registered.ID {
		t.Errorf("expected id %q, got %q", registered.ID, me.ID)
	}
	if me.Email != "me@example.com" {
		t.Errorf("expected email me@example.com, got %q", me.Email)
	}
}

func TestMe_WithBearerToken(t *testing.T) {
	srv := newTestServer(t)
	defer srv.Close()

	resp := postJSON(t, srv.URL+"/auth/register",
		`{"email":"bearer@example.com","password":"mypassword123"}`)
	var registered model.AuthResponse
	decodeBody(t, resp, &registered)

	req, _ := http.NewRequest(http.MethodGet, srv.URL+"/auth/me", nil)
	req.Header.Set("Authorization", "Bearer "+registered.Token)
	meResp, err := http.DefaultClient.Do(req)
	if err != nil {
		t.Fatalf("get /auth/me: %v", err)
	}
	defer meResp.Body.Close()

	if meResp.StatusCode != http.StatusOK {
		t.Fatalf("expected 200, got %d", meResp.StatusCode)
	}
}

func TestMe_Unauthenticated(t *testing.T) {
	srv := newTestServer(t)
	defer srv.Close()

	resp, err := http.Get(srv.URL + "/auth/me")
	if err != nil {
		t.Fatalf("get /auth/me: %v", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusUnauthorized {
		t.Errorf("expected 401, got %d", resp.StatusCode)
	}
}

func TestLogout_ClearsCookie(t *testing.T) {
	srv := newTestServer(t)
	defer srv.Close()

	resp := postJSON(t, srv.URL+"/auth/register",
		`{"email":"logout@example.com","password":"mypassword123"}`)
	cookie := authCookie(t, resp)
	resp.Body.Close()

	req, _ := http.NewRequest(http.MethodPost, srv.URL+"/auth/logout", nil)
	req.AddCookie(cookie)
	logoutResp, err := http.DefaultClient.Do(req)
	if err != nil {
		t.Fatalf("post /auth/logout: %v", err)
	}
	defer logoutResp.Body.Close()

	if logoutResp.StatusCode != http.StatusNoContent {
		t.Fatalf("expected 204, got %d", logoutResp.StatusCode)
	}
	if cleared := authCookie(t, logoutResp); cleared.Value != "" {
		t.Errorf("expected cleared cookie, got value %q", cleared.Value)
	}
	if raw := logoutResp.Header.Get("Set-Cookie"); !strings.Contains(raw, "Max-Age=0") {
		t.Errorf("expected Max-Age=0 in Set-Cookie, got %q", raw)
	}
}
