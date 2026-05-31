package state_test

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
	"statesu.com/internal/state"
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

// newTestServer wires the full stack: auth + state handlers on a single mux.
func newTestServer(t *testing.T) (*httptest.Server, *crypto.JWTIssuer) {
	t.Helper()
	db := testDB(t)
	t.Cleanup(func() { db.Close() })

	cipher := testCipher(t)
	jwt := testJWT(t)

	authRepo := auth.NewRepository(db)
	authSvc := auth.NewService(authRepo, cipher)
	authHandler := auth.NewHandler(authSvc, jwt)

	stateRepo := state.NewRepository(db)
	stateSvc := state.NewService(stateRepo, authRepo, cipher)
	stateHandler := state.NewHandler(stateSvc, jwt)

	mux := http.NewServeMux()
	authHandler.Mount(mux)
	stateHandler.Mount(mux)

	return httptest.NewServer(mux), jwt
}

// registerAndGetToken creates a user via the auth API and returns the bearer token.
func registerAndGetToken(t *testing.T, srvURL, email, password string) string {
	t.Helper()
	body := fmt.Sprintf(`{"email":"%s","password":"%s"}`, email, password)
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

func authPostJSON(t *testing.T, url, token, body string) *http.Response {
	t.Helper()
	req, err := http.NewRequest(http.MethodPost, url, strings.NewReader(body))
	if err != nil {
		t.Fatalf("new request: %v", err)
	}
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("Authorization", token)
	resp, err := http.DefaultClient.Do(req)
	if err != nil {
		t.Fatalf("do request: %v", err)
	}
	return resp
}

func authDelete(t *testing.T, url, token string) *http.Response {
	t.Helper()
	req, err := http.NewRequest(http.MethodDelete, url, nil)
	if err != nil {
		t.Fatalf("new request: %v", err)
	}
	req.Header.Set("Authorization", token)
	resp, err := http.DefaultClient.Do(req)
	if err != nil {
		t.Fatalf("do request: %v", err)
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

func futureExpiry(t *testing.T) int64 {
	t.Helper()
	return time.Now().Add(24 * time.Hour).Unix()
}

// ---------------------------------------------------------------------------
// Create – success
// ---------------------------------------------------------------------------

func TestCreate_Success(t *testing.T) {
	srv, _ := newTestServer(t)
	defer srv.Close()

	token := registerAndGetToken(t, srv.URL, "create@example.com", "mypassword123")
	exp := futureExpiry(t)

	resp := authPostJSON(t, srv.URL+"/state", token,
		fmt.Sprintf(`{"text":"hello world","expires_at":%d}`, exp))

	if resp.StatusCode != http.StatusCreated {
		t.Fatalf("expected 201, got %d", resp.StatusCode)
	}

	var sr model.StateResponse
	decodeBody(t, resp, &sr)

	if sr.ID == "" {
		t.Error("expected non-empty state_id")
	}
	if sr.UserID == "" {
		t.Error("expected non-empty user_id")
	}
	if sr.Text != "hello world" {
		t.Errorf("expected text 'hello world', got %q", sr.Text)
	}
	if sr.CreatedAt == 0 {
		t.Error("expected non-zero created_at")
	}
	if sr.ExpiresAt != exp {
		t.Errorf("expected expires_at %d, got %d", exp, sr.ExpiresAt)
	}
}

// ---------------------------------------------------------------------------
// Create – missing / invalid auth
// ---------------------------------------------------------------------------

func TestCreate_MissingAuth(t *testing.T) {
	srv, _ := newTestServer(t)
	defer srv.Close()

	resp, err := http.Post(srv.URL+"/state", "application/json",
		strings.NewReader(`{"text":"hi","expires_at":9999999999}`))
	if err != nil {
		t.Fatalf("post: %v", err)
	}

	if resp.StatusCode != http.StatusUnauthorized {
		t.Errorf("expected 401, got %d", resp.StatusCode)
	}
	resp.Body.Close()
}

func TestCreate_InvalidAuth(t *testing.T) {
	srv, _ := newTestServer(t)
	defer srv.Close()

	resp := authPostJSON(t, srv.URL+"/state", "Bearer garbage",
		`{"text":"hi","expires_at":9999999999}`)

	if resp.StatusCode != http.StatusUnauthorized {
		t.Errorf("expected 401, got %d", resp.StatusCode)
	}
	resp.Body.Close()
}

// ---------------------------------------------------------------------------
// Create – invalid input
// ---------------------------------------------------------------------------

func TestCreate_InvalidInput(t *testing.T) {
	srv, _ := newTestServer(t)
	defer srv.Close()

	token := registerAndGetToken(t, srv.URL, "input@example.com", "mypassword123")
	exp := futureExpiry(t)

	tests := []struct {
		name string
		body string
	}{
		{"empty text", fmt.Sprintf(`{"text":"","expires_at":%d}`, exp)},
		{"whitespace only", fmt.Sprintf(`{"text":"   ","expires_at":%d}`, exp)},
		{"text too long", fmt.Sprintf(`{"text":"%s","expires_at":%d}`, strings.Repeat("x", 4097), exp)},
		{"expiry in past", `{"text":"hello","expires_at":1}`},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			resp := authPostJSON(t, srv.URL+"/state", token, tt.body)
			if resp.StatusCode != http.StatusBadRequest {
				t.Errorf("expected 400, got %d", resp.StatusCode)
			}
			resp.Body.Close()
		})
	}
}

func TestCreate_InvalidJSON(t *testing.T) {
	srv, _ := newTestServer(t)
	defer srv.Close()

	token := registerAndGetToken(t, srv.URL, "badjson@example.com", "mypassword123")

	resp := authPostJSON(t, srv.URL+"/state", token, `not json`)

	if resp.StatusCode != http.StatusBadRequest {
		t.Errorf("expected 400, got %d", resp.StatusCode)
	}
	resp.Body.Close()
}

// ---------------------------------------------------------------------------
// List – success
// ---------------------------------------------------------------------------

func TestList_Success(t *testing.T) {
	srv, _ := newTestServer(t)
	defer srv.Close()

	token := registerAndGetToken(t, srv.URL, "list@example.com", "mypassword123")
	exp := futureExpiry(t)

	// Create two states.
	authPostJSON(t, srv.URL+"/state", token,
		fmt.Sprintf(`{"text":"state one","expires_at":%d}`, exp))
	authPostJSON(t, srv.URL+"/state", token,
		fmt.Sprintf(`{"text":"state two","expires_at":%d}`, exp))

	// List them (no auth needed on GET /state).
	resp, err := http.Get(srv.URL + "/state?email=nobody@example.com")
	if err != nil {
		t.Fatalf("get: %v", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		t.Fatalf("expected 200, got %d", resp.StatusCode)
	}

	var page model.PaginatedStatesResponse
	if err := json.NewDecoder(resp.Body).Decode(&page); err != nil {
		t.Fatalf("decode: %v", err)
	}
	if len(page.Items) != 0 {
		t.Errorf("expected 0 items for unknown user, got %d", len(page.Items))
	}
	if page.Total != 0 {
		t.Errorf("expected total 0, got %d", page.Total)
	}
}

func TestList_ReturnsStates(t *testing.T) {
	srv, _ := newTestServer(t)
	defer srv.Close()

	email := "list2@example.com"
	token := registerAndGetToken(t, srv.URL, email, "mypassword123")
	exp := futureExpiry(t)

	authPostJSON(t, srv.URL+"/state", token,
		fmt.Sprintf(`{"text":"first","expires_at":%d}`, exp))

	// Create a second state.
	time.Sleep(10 * time.Millisecond)
	authPostJSON(t, srv.URL+"/state", token,
		fmt.Sprintf(`{"text":"second","expires_at":%d}`, exp))

	// List by email.
	resp, err := http.Get(srv.URL + "/state?email=" + email)
	if err != nil {
		t.Fatalf("get: %v", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		t.Fatalf("expected 200, got %d", resp.StatusCode)
	}

	var page model.PaginatedStatesResponse
	if err := json.NewDecoder(resp.Body).Decode(&page); err != nil {
		t.Fatalf("decode: %v", err)
	}
	if page.Total != 2 {
		t.Fatalf("expected total 2, got %d", page.Total)
	}
	if len(page.Items) != 2 {
		t.Fatalf("expected 2 items, got %d", len(page.Items))
	}
	// Items are ordered newest first.
	if page.Items[0].Text != "second" {
		t.Errorf("expected first item text 'second', got %q", page.Items[0].Text)
	}
	if page.Items[1].Text != "first" {
		t.Errorf("expected second item text 'first', got %q", page.Items[1].Text)
	}
}

func TestList_Paginates(t *testing.T) {
	srv, _ := newTestServer(t)
	defer srv.Close()

	email := "paginate@example.com"
	token := registerAndGetToken(t, srv.URL, email, "mypassword123")
	exp := futureExpiry(t)

	// Create 5 states. Sleep between them so created_at ordering is stable.
	for i := 0; i < 5; i++ {
		authPostJSON(t, srv.URL+"/state", token,
			fmt.Sprintf(`{"text":"s%d","expires_at":%d}`, i, exp))
		time.Sleep(10 * time.Millisecond)
	}

	// Page 1, size 2 → newest two: s4, s3.
	resp, err := http.Get(srv.URL + "/state?email=" + email + "&page=1&size=2")
	if err != nil {
		t.Fatalf("get: %v", err)
	}
	defer resp.Body.Close()

	var p1 model.PaginatedStatesResponse
	if err := json.NewDecoder(resp.Body).Decode(&p1); err != nil {
		t.Fatalf("decode: %v", err)
	}
	if p1.Total != 5 {
		t.Errorf("expected total 5, got %d", p1.Total)
	}
	if p1.Page != 1 || p1.Size != 2 {
		t.Errorf("expected page=1 size=2, got page=%d size=%d", p1.Page, p1.Size)
	}
	if len(p1.Items) != 2 || p1.Items[0].Text != "s4" || p1.Items[1].Text != "s3" {
		t.Errorf("unexpected page 1 items: %+v", p1.Items)
	}

	// Page 3, size 2 → only one item left: s0.
	resp2, err := http.Get(srv.URL + "/state?email=" + email + "&page=3&size=2")
	if err != nil {
		t.Fatalf("get: %v", err)
	}
	defer resp2.Body.Close()

	var p3 model.PaginatedStatesResponse
	if err := json.NewDecoder(resp2.Body).Decode(&p3); err != nil {
		t.Fatalf("decode: %v", err)
	}
	if len(p3.Items) != 1 || p3.Items[0].Text != "s0" {
		t.Errorf("unexpected page 3 items: %+v", p3.Items)
	}
}

func TestList_MissingEmail(t *testing.T) {
	srv, _ := newTestServer(t)
	defer srv.Close()

	resp, err := http.Get(srv.URL + "/state")
	if err != nil {
		t.Fatalf("get: %v", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusBadRequest {
		t.Errorf("expected 400, got %d", resp.StatusCode)
	}
}

// ---------------------------------------------------------------------------
// Latest
// ---------------------------------------------------------------------------

func TestLatest_NotFound(t *testing.T) {
	srv, _ := newTestServer(t)
	defer srv.Close()

	resp, err := http.Get(srv.URL + "/state/latest?email=nobody@example.com")
	if err != nil {
		t.Fatalf("get: %v", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusNotFound {
		t.Errorf("expected 404, got %d", resp.StatusCode)
	}
}

// TestLatest_NoEmailReturnsGlobalLatest verifies that omitting the email filter
// returns the most recent state across all users.
func TestLatest_NoEmailReturnsGlobalLatest(t *testing.T) {
	srv, _ := newTestServer(t)
	defer srv.Close()

	exp := futureExpiry(t)

	tokenA := registerAndGetToken(t, srv.URL, "alice@example.com", "mypassword123")
	authPostJSON(t, srv.URL+"/state", tokenA,
		fmt.Sprintf(`{"text":"alice first","expires_at":%d}`, exp))

	time.Sleep(10 * time.Millisecond)

	bobEmail := "bob@example.com"
	tokenB := registerAndGetToken(t, srv.URL, bobEmail, "mypassword123")
	authPostJSON(t, srv.URL+"/state", tokenB,
		fmt.Sprintf(`{"text":"bob latest","expires_at":%d}`, exp))

	resp, err := http.Get(srv.URL + "/state/latest")
	if err != nil {
		t.Fatalf("get: %v", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		t.Fatalf("expected 200, got %d", resp.StatusCode)
	}

	var got model.LatestStateResponse
	if err := json.NewDecoder(resp.Body).Decode(&got); err != nil {
		t.Fatalf("decode: %v", err)
	}
	if got.Email != bobEmail {
		t.Errorf("expected globally newest from %q, got %q", bobEmail, got.Email)
	}
	if got.Text != "bob latest" {
		t.Errorf("expected text 'bob latest', got %q", got.Text)
	}
}

func TestLatest_Success(t *testing.T) {
	srv, _ := newTestServer(t)
	defer srv.Close()

	email := "latest@example.com"
	token := registerAndGetToken(t, srv.URL, email, "mypassword123")
	exp := futureExpiry(t)

	resp1 := authPostJSON(t, srv.URL+"/state", token,
		fmt.Sprintf(`{"text":"only state","expires_at":%d}`, exp))
	var sr model.StateResponse
	decodeBody(t, resp1, &sr)

	resp, err := http.Get(srv.URL + "/state/latest?email=" + email)
	if err != nil {
		t.Fatalf("get: %v", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		t.Fatalf("expected 200, got %d", resp.StatusCode)
	}

	var got model.LatestStateResponse
	if err := json.NewDecoder(resp.Body).Decode(&got); err != nil {
		t.Fatalf("decode: %v", err)
	}
	if got.UserID != sr.UserID {
		t.Errorf("expected user_id %q, got %q", sr.UserID, got.UserID)
	}
	if got.Email != email {
		t.Errorf("expected email %q, got %q", email, got.Email)
	}
	if got.Text != "only state" {
		t.Errorf("expected text 'only state', got %q", got.Text)
	}
	if got.CreatedAt == 0 {
		t.Error("expected non-zero created_at")
	}
	if got.ExpiresAt != exp {
		t.Errorf("expected expires_at %d, got %d", exp, got.ExpiresAt)
	}
}

// TestLatest_ScopedToQueriedUser verifies that /state/latest returns the
// queried user's most recent state, not whichever record was written last
// across the whole table.
func TestLatest_ScopedToQueriedUser(t *testing.T) {
	srv, _ := newTestServer(t)
	defer srv.Close()

	exp := futureExpiry(t)

	aliceEmail := "alice@example.com"
	tokenA := registerAndGetToken(t, srv.URL, aliceEmail, "mypassword123")
	respA := authPostJSON(t, srv.URL+"/state", tokenA,
		fmt.Sprintf(`{"text":"alice latest","expires_at":%d}`, exp))
	var aliceState model.StateResponse
	decodeBody(t, respA, &aliceState)

	// Ensure DATETIME timestamps differ (sqlite CURRENT_TIMESTAMP has 1s granularity,
	// but our writes use Go time.Now which has finer resolution).
	time.Sleep(10 * time.Millisecond)

	// Bob writes the globally most recent state.
	tokenB := registerAndGetToken(t, srv.URL, "bob@example.com", "mypassword123")
	authPostJSON(t, srv.URL+"/state", tokenB,
		fmt.Sprintf(`{"text":"bob latest","expires_at":%d}`, exp))

	// Querying alice must still return alice's latest, not bob's.
	resp, err := http.Get(srv.URL + "/state/latest?email=" + aliceEmail)
	if err != nil {
		t.Fatalf("get: %v", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		t.Fatalf("expected 200, got %d", resp.StatusCode)
	}

	var got model.LatestStateResponse
	if err := json.NewDecoder(resp.Body).Decode(&got); err != nil {
		t.Fatalf("decode: %v", err)
	}
	if got.UserID != aliceState.UserID {
		t.Errorf("expected alice's user_id %q, got %q", aliceState.UserID, got.UserID)
	}
	if got.Email != aliceEmail {
		t.Errorf("expected email %q, got %q", aliceEmail, got.Email)
	}
	if got.Text != "alice latest" {
		t.Errorf("expected text 'alice latest', got %q", got.Text)
	}
}

// ---------------------------------------------------------------------------
// Delete – success
// ---------------------------------------------------------------------------

func TestDelete_Success(t *testing.T) {
	srv, _ := newTestServer(t)
	defer srv.Close()

	email := "del@example.com"
	token := registerAndGetToken(t, srv.URL, email, "mypassword123")
	exp := futureExpiry(t)

	resp1 := authPostJSON(t, srv.URL+"/state", token,
		fmt.Sprintf(`{"text":"to delete","expires_at":%d}`, exp))
	var sr model.StateResponse
	decodeBody(t, resp1, &sr)

	resp2 := authDelete(t, srv.URL+"/state/"+sr.ID, token)
	if resp2.StatusCode != http.StatusNoContent {
		t.Errorf("expected 204, got %d", resp2.StatusCode)
	}
	resp2.Body.Close()

	resp3, err := http.Get(srv.URL + "/state?email=" + email)
	if err != nil {
		t.Fatalf("get: %v", err)
	}
	defer resp3.Body.Close()

	var page model.PaginatedStatesResponse
	json.NewDecoder(resp3.Body).Decode(&page)
	if len(page.Items) != 0 {
		t.Errorf("expected 0 items after delete, got %d", len(page.Items))
	}
}

// ---------------------------------------------------------------------------
// Delete – not found
// ---------------------------------------------------------------------------

func TestDelete_NotFound(t *testing.T) {
	srv, _ := newTestServer(t)
	defer srv.Close()

	token := registerAndGetToken(t, srv.URL, "nodel@example.com", "mypassword123")

	resp := authDelete(t, srv.URL+"/state/00000000-0000-0000-0000-000000000000", token)
	if resp.StatusCode != http.StatusNotFound {
		t.Errorf("expected 404, got %d", resp.StatusCode)
	}
	resp.Body.Close()
}

// ---------------------------------------------------------------------------
// Delete – missing auth
// ---------------------------------------------------------------------------

func TestDelete_MissingAuth(t *testing.T) {
	srv, _ := newTestServer(t)
	defer srv.Close()

	req, err := http.NewRequest(http.MethodDelete, srv.URL+"/state/some-id", nil)
	if err != nil {
		t.Fatalf("new request: %v", err)
	}
	resp, err := http.DefaultClient.Do(req)
	if err != nil {
		t.Fatalf("do request: %v", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusUnauthorized {
		t.Errorf("expected 401, got %d", resp.StatusCode)
	}
}

// ---------------------------------------------------------------------------
// E2E – full round-trip: register → create → list → delete → list empty
// ---------------------------------------------------------------------------

func TestE2E_FullStateFlow(t *testing.T) {
	srv, jwt := newTestServer(t)
	defer srv.Close()

	email := "e2estate@example.com"
	token := registerAndGetToken(t, srv.URL, email, "mypassword123")

	exp := futureExpiry(t)
	resp1 := authPostJSON(t, srv.URL+"/state", token,
		fmt.Sprintf(`{"text":"e2e state","expires_at":%d}`, exp))
	if resp1.StatusCode != http.StatusCreated {
		t.Fatalf("create: expected 201, got %d", resp1.StatusCode)
	}
	var sr model.StateResponse
	decodeBody(t, resp1, &sr)

	resp2, err := http.Get(srv.URL + "/state?email=" + email)
	if err != nil {
		t.Fatalf("list: %v", err)
	}
	var page model.PaginatedStatesResponse
	decodeBody(t, resp2, &page)
	if len(page.Items) != 1 {
		t.Fatalf("expected 1 item, got %d", len(page.Items))
	}
	if page.Items[0].Text != "e2e state" {
		t.Errorf("expected 'e2e state', got %q", page.Items[0].Text)
	}

	if _, err := jwt.Verify(strings.TrimPrefix(token, "Bearer ")); err != nil {
		t.Fatalf("token should still be valid: %v", err)
	}

	resp3 := authDelete(t, srv.URL+"/state/"+sr.ID, token)
	if resp3.StatusCode != http.StatusNoContent {
		t.Fatalf("delete: expected 204, got %d", resp3.StatusCode)
	}
	resp3.Body.Close()

	resp4, err := http.Get(srv.URL + "/state?email=" + email)
	if err != nil {
		t.Fatalf("list after delete: %v", err)
	}
	var empty model.PaginatedStatesResponse
	decodeBody(t, resp4, &empty)
	if len(empty.Items) != 0 {
		t.Errorf("expected 0 items after delete, got %d", len(empty.Items))
	}
}
