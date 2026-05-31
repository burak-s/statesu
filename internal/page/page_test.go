package page_test

import (
	"database/sql"
	"encoding/json"
	"fmt"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"

	"statesu.com/internal/auth"
	"statesu.com/internal/config"
	"statesu.com/internal/crypto"
	"statesu.com/internal/model"
	"statesu.com/internal/page"
	"statesu.com/internal/state"
	"statesu.com/internal/view"
)

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

func testCipher(t *testing.T) *crypto.EmailCipher {
	t.Helper()
	c, err := crypto.NewEmailCipher(
		[]byte("01234567890123456789012345678901"),
		[]byte("hmac-key-for-testing-purposes-ok"),
	)
	if err != nil {
		t.Fatalf("new email cipher: %v", err)
	}
	return c
}

func testJWT(t *testing.T) *crypto.JWTIssuer {
	t.Helper()
	j, err := crypto.NewJWTIssuer([]byte("this-is-a-test-jwt-secret-key!!!"))
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
	t.Cleanup(func() { db.Close() })
	return db
}

// newTestServer wires the full page + state + auth stack.
func newTestServer(t *testing.T) *httptest.Server {
	t.Helper()

	db := testDB(t)
	cipher := testCipher(t)
	jwt := testJWT(t)

	renderer, err := view.New()
	if err != nil {
		t.Fatalf("view.New: %v", err)
	}

	authRepo := auth.NewRepository(db)
	authSvc := auth.NewService(authRepo, cipher)
	authHandler := auth.NewHandler(authSvc, jwt)

	stateRepo := state.NewRepository(db)
	stateSvc := state.NewService(stateRepo, authRepo, cipher)
	stateHandler := state.NewHandler(stateSvc, jwt)

	pageHandler := page.NewHandler(jwt, authSvc, stateSvc, renderer)

	mux := http.NewServeMux()
	authHandler.Mount(mux)
	stateHandler.Mount(mux)
	pageHandler.Mount(mux)

	srv := httptest.NewServer(mux)
	t.Cleanup(srv.Close)
	return srv
}

func registerAndGetToken(t *testing.T, srvURL, email, password string) string {
	t.Helper()
	body := fmt.Sprintf(`{"email":%q,"password":%q}`, email, password)
	resp, err := http.Post(srvURL+"/auth/register", "application/json", strings.NewReader(body))
	if err != nil {
		t.Fatalf("register: %v", err)
	}
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusCreated {
		t.Fatalf("register: expected 201, got %d", resp.StatusCode)
	}
	var ar struct {
		Token string `json:"token"`
	}
	if err := json.NewDecoder(resp.Body).Decode(&ar); err != nil {
		t.Fatalf("decode token: %v", err)
	}
	return "Bearer " + ar.Token
}

func createState(t *testing.T, srvURL, token, text string) model.StateResponse {
	t.Helper()
	exp := time.Now().Add(24 * time.Hour).Unix()
	body := fmt.Sprintf(`{"text":%q,"expires_at":%d}`, text, exp)
	req, _ := http.NewRequest(http.MethodPost, srvURL+"/state", strings.NewReader(body))
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("Authorization", token)
	resp, err := http.DefaultClient.Do(req)
	if err != nil {
		t.Fatalf("create state: %v", err)
	}
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusCreated {
		t.Fatalf("create state: expected 201, got %d", resp.StatusCode)
	}
	var sr model.StateResponse
	if err := json.NewDecoder(resp.Body).Decode(&sr); err != nil {
		t.Fatalf("decode state: %v", err)
	}
	return sr
}

func authDelete(t *testing.T, url, token string) *http.Response {
	t.Helper()
	req, err := http.NewRequest(http.MethodDelete, url, nil)
	if err != nil {
		t.Fatalf("new request: %v", err)
	}
	if token != "" {
		req.Header.Set("Authorization", token)
	}
	resp, err := http.DefaultClient.Do(req)
	if err != nil {
		t.Fatalf("do request: %v", err)
	}
	return resp
}

// ---------------------------------------------------------------------------
// DELETE /my-states/{stateID}
// ---------------------------------------------------------------------------

func TestDeleteState_Success(t *testing.T) {
	srv := newTestServer(t)
	token := registerAndGetToken(t, srv.URL, "del@example.com", "mypassword1")
	st := createState(t, srv.URL, token, "to be deleted")

	resp := authDelete(t, srv.URL+"/my-states/"+st.ID, token)
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		t.Fatalf("expected 200, got %d", resp.StatusCode)
	}

	trigger := resp.Header.Get("HX-Trigger")
	if trigger == "" {
		t.Fatal("expected HX-Trigger header, got none")
	}

	var payload struct {
		Toast struct {
			Message string `json:"message"`
			Type    string `json:"type"`
		} `json:"toast"`
	}
	if err := json.Unmarshal([]byte(trigger), &payload); err != nil {
		t.Fatalf("parse HX-Trigger: %v", err)
	}
	if payload.Toast.Message == "" {
		t.Error("expected non-empty toast message")
	}
}

func TestDeleteState_NotFound(t *testing.T) {
	srv := newTestServer(t)
	token := registerAndGetToken(t, srv.URL, "notfound@example.com", "mypassword1")

	resp := authDelete(t, srv.URL+"/my-states/00000000-0000-0000-0000-000000000000", token)
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusNotFound {
		t.Errorf("expected 404, got %d", resp.StatusCode)
	}
}

func TestDeleteState_Unauthorized(t *testing.T) {
	srv := newTestServer(t)
	token := registerAndGetToken(t, srv.URL, "owner@example.com", "mypassword1")
	st := createState(t, srv.URL, token, "owned state")

	resp := authDelete(t, srv.URL+"/my-states/"+st.ID, "")
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusUnauthorized {
		t.Errorf("expected 401, got %d", resp.StatusCode)
	}
}

func TestDeleteState_WrongUser(t *testing.T) {
	srv := newTestServer(t)

	ownerToken := registerAndGetToken(t, srv.URL, "owner2@example.com", "mypassword1")
	st := createState(t, srv.URL, ownerToken, "not yours")

	otherToken := registerAndGetToken(t, srv.URL, "other@example.com", "mypassword1")
	resp := authDelete(t, srv.URL+"/my-states/"+st.ID, otherToken)
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusNotFound {
		t.Errorf("expected 404 when deleting another user's state, got %d", resp.StatusCode)
	}
}

// ---------------------------------------------------------------------------
// GET /my-states
// ---------------------------------------------------------------------------

func TestMyStates_Success(t *testing.T) {
	srv := newTestServer(t)
	token := registerAndGetToken(t, srv.URL, "list@example.com", "mypassword1")
	createState(t, srv.URL, token, "my state")

	req, _ := http.NewRequest(http.MethodGet, srv.URL+"/my-states", nil)
	req.Header.Set("Authorization", token)
	resp, err := http.DefaultClient.Do(req)
	if err != nil {
		t.Fatalf("get /my-states: %v", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		t.Errorf("expected 200, got %d", resp.StatusCode)
	}
	ct := resp.Header.Get("Content-Type")
	if !strings.HasPrefix(ct, "text/html") {
		t.Errorf("expected text/html content type, got %q", ct)
	}
}

func TestMyStates_Unauthorized(t *testing.T) {
	srv := newTestServer(t)

	resp, err := http.Get(srv.URL + "/my-states")
	if err != nil {
		t.Fatalf("get /my-states: %v", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusUnauthorized {
		t.Errorf("expected 401, got %d", resp.StatusCode)
	}
}
