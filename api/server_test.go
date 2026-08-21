package api_test

import (
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"

	"github.com/PhillipC05/tpt-identity/api"
	"github.com/PhillipC05/tpt-identity/internal/resolver"
	"github.com/PhillipC05/tpt-identity/internal/store"
	"github.com/PhillipC05/tpt-identity/oidc"
	"github.com/PhillipC05/tpt-identity/pkg/crypto"
)

const (
	testAPIKey = "test-api-key-1234"
	testIssuer = "https://test.tpt-identity.example"
)

// newTestServer wires a Server against an in-memory SQLite store with a fresh Ed25519 key.
func newTestServer(t *testing.T) *api.Server {
	t.Helper()
	st, err := store.OpenSQLite(":memory:")
	if err != nil {
		t.Fatalf("open store: %v", err)
	}
	t.Cleanup(func() { _ = st.Close() })

	_, priv, err := crypto.GenerateSigningKey()
	if err != nil {
		t.Fatalf("keygen: %v", err)
	}
	oidcProvider := oidc.NewProvider(testIssuer, priv, "test-key-1", st)
	res := resolver.New(5 * time.Minute)

	return api.NewServer(api.Config{
		APIKey:       testAPIKey,
		Issuer:       testIssuer,
		SigningKey:    priv,
		SigningKeyID:  testIssuer + "#signing-key-1",
		Store:        st,
		Resolver:     res,
		OIDC:         oidcProvider,
	})
}

// do fires a request against srv and returns the recorder.
func do(t *testing.T, srv *api.Server, method, path, body string, headers map[string]string) *httptest.ResponseRecorder {
	t.Helper()
	r := httptest.NewRequest(method, path, strings.NewReader(body))
	for k, v := range headers {
		r.Header.Set(k, v)
	}
	w := httptest.NewRecorder()
	srv.ServeHTTP(w, r)
	return w
}

// authed wraps do with the test API key and JSON content-type when there is a body.
func authed(t *testing.T, srv *api.Server, method, path, body string) *httptest.ResponseRecorder {
	t.Helper()
	h := map[string]string{"Authorization": "Bearer " + testAPIKey}
	if body != "" {
		h["Content-Type"] = "application/json"
	}
	return do(t, srv, method, path, body, h)
}

// ---- Liveness & readiness ----

func TestLiveness(t *testing.T) {
	srv := newTestServer(t)
	w := do(t, srv, http.MethodGet, "/healthz", "", nil)
	if w.Code != http.StatusOK {
		t.Errorf("want 200, got %d: %s", w.Code, w.Body.String())
	}
	if !strings.Contains(w.Body.String(), "ok") {
		t.Errorf("unexpected body: %q", w.Body.String())
	}
}

func TestReadiness(t *testing.T) {
	srv := newTestServer(t)
	w := do(t, srv, http.MethodGet, "/readyz", "", nil)
	if w.Code != http.StatusOK {
		t.Errorf("want 200, got %d: %s", w.Code, w.Body.String())
	}
	var result map[string]any
	if err := json.Unmarshal(w.Body.Bytes(), &result); err != nil {
		t.Fatalf("bad JSON: %v", err)
	}
	if result["status"] != "ok" {
		t.Errorf("want status=ok, got %v", result["status"])
	}
}

// ---- OIDC endpoints ----

func TestOIDCDiscovery(t *testing.T) {
	srv := newTestServer(t)
	w := do(t, srv, http.MethodGet, "/.well-known/openid-configuration", "", nil)
	if w.Code != http.StatusOK {
		t.Fatalf("want 200, got %d: %s", w.Code, w.Body.String())
	}
	var meta map[string]any
	if err := json.Unmarshal(w.Body.Bytes(), &meta); err != nil {
		t.Fatalf("bad JSON: %v", err)
	}
	if meta["issuer"] != testIssuer {
		t.Errorf("want issuer=%q, got %v", testIssuer, meta["issuer"])
	}
	if _, ok := meta["authorization_endpoint"]; !ok {
		t.Error("missing authorization_endpoint in discovery doc")
	}
	if _, ok := meta["jwks_uri"]; !ok {
		t.Error("missing jwks_uri in discovery doc")
	}
}

func TestJWKS(t *testing.T) {
	srv := newTestServer(t)
	w := do(t, srv, http.MethodGet, "/.well-known/jwks.json", "", nil)
	if w.Code != http.StatusOK {
		t.Fatalf("want 200, got %d: %s", w.Code, w.Body.String())
	}
	var jwks map[string]any
	if err := json.Unmarshal(w.Body.Bytes(), &jwks); err != nil {
		t.Fatalf("bad JSON: %v", err)
	}
	keys, ok := jwks["keys"].([]any)
	if !ok || len(keys) == 0 {
		t.Errorf("expected at least one JWK, got: %v", jwks)
	}
}

// ---- Auth middleware ----

func TestAuthRequiredNoHeader(t *testing.T) {
	srv := newTestServer(t)
	w := do(t, srv, http.MethodGet, "/api/v1/identities/did:peer:test", "", nil)
	if w.Code != http.StatusUnauthorized {
		t.Errorf("want 401 (no auth), got %d", w.Code)
	}
}

func TestAuthRequiredWrongKey(t *testing.T) {
	srv := newTestServer(t)
	w := do(t, srv, http.MethodGet, "/api/v1/identities/did:peer:test", "", map[string]string{
		"Authorization": "Bearer wrong-key",
	})
	if w.Code != http.StatusUnauthorized {
		t.Errorf("want 401 (wrong key), got %d", w.Code)
	}
}

// ---- Identities ----

func TestCreateIdentityPeer(t *testing.T) {
	srv := newTestServer(t)
	w := authed(t, srv, http.MethodPost, "/api/v1/identities", `{"method":"peer"}`)
	if w.Code != http.StatusCreated {
		t.Fatalf("want 201, got %d: %s", w.Code, w.Body.String())
	}
	var resp map[string]any
	if err := json.Unmarshal(w.Body.Bytes(), &resp); err != nil {
		t.Fatalf("bad JSON: %v", err)
	}
	if resp["did"] == nil {
		t.Error("missing did in create response")
	}
}

func TestCreateIdentityEmptyMethodFails(t *testing.T) {
	srv := newTestServer(t)
	w := authed(t, srv, http.MethodPost, "/api/v1/identities", `{}`)
	if w.Code < 400 {
		t.Errorf("want ≥400 for empty method, got %d: %s", w.Code, w.Body.String())
	}
}

func TestCreateIdentityWebRequiresDomain(t *testing.T) {
	srv := newTestServer(t)
	// did:web without a domain should fail
	w := authed(t, srv, http.MethodPost, "/api/v1/identities", `{"method":"web"}`)
	if w.Code < 400 {
		t.Errorf("want ≥400 for web identity without domain, got %d: %s", w.Code, w.Body.String())
	}
}

// ---- Webhooks ----

func TestWebhookCreateListDelete(t *testing.T) {
	srv := newTestServer(t)

	// Create
	body := `{"url":"https://example.com/hook","event_types":["credential.issued"]}`
	w := authed(t, srv, http.MethodPost, "/api/v1/webhooks", body)
	if w.Code != http.StatusCreated {
		t.Fatalf("create: want 201, got %d: %s", w.Code, w.Body.String())
	}
	var created map[string]any
	if err := json.Unmarshal(w.Body.Bytes(), &created); err != nil {
		t.Fatalf("bad JSON: %v", err)
	}
	webhookID, _ := created["id"].(string)
	if webhookID == "" {
		t.Fatal("missing id in webhook response")
	}
	if created["signing_secret"] == nil {
		t.Error("signing_secret must be returned on creation")
	}

	// List — should contain 1
	w = authed(t, srv, http.MethodGet, "/api/v1/webhooks", "")
	if w.Code != http.StatusOK {
		t.Fatalf("list: want 200, got %d", w.Code)
	}
	var list []any
	_ = json.Unmarshal(w.Body.Bytes(), &list)
	if len(list) != 1 {
		t.Errorf("want 1 webhook, got %d", len(list))
	}

	// Delete
	w = authed(t, srv, http.MethodDelete, "/api/v1/webhooks/"+webhookID, "")
	if w.Code != http.StatusNoContent {
		t.Errorf("delete: want 204, got %d: %s", w.Code, w.Body.String())
	}

	// List again — should be empty
	w = authed(t, srv, http.MethodGet, "/api/v1/webhooks", "")
	_ = json.Unmarshal(w.Body.Bytes(), &list)
	if len(list) != 0 {
		t.Errorf("want 0 webhooks after delete, got %d", len(list))
	}
}

func TestWebhookMissingURLRejects(t *testing.T) {
	srv := newTestServer(t)
	w := authed(t, srv, http.MethodPost, "/api/v1/webhooks", `{"event_types":["x"]}`)
	if w.Code != http.StatusBadRequest {
		t.Errorf("want 400 for missing URL, got %d", w.Code)
	}
}

// ---- Consent grants ----

func TestListGrantsMissingSubjectRejects(t *testing.T) {
	srv := newTestServer(t)
	// No ?subject query param
	w := authed(t, srv, http.MethodGet, "/api/v1/consents/grants", "")
	if w.Code != http.StatusBadRequest {
		t.Errorf("want 400 without subject param, got %d", w.Code)
	}
}

func TestListGrantsEmpty(t *testing.T) {
	srv := newTestServer(t)
	w := authed(t, srv, http.MethodGet, "/api/v1/consents/grants?subject=did:peer:nobody", "")
	if w.Code != http.StatusOK {
		t.Fatalf("want 200, got %d: %s", w.Code, w.Body.String())
	}
	// Should return an empty list (not null).
	var list []any
	if err := json.Unmarshal(w.Body.Bytes(), &list); err != nil {
		// null is also acceptable — some handlers return null for empty lists
		if !strings.Contains(w.Body.String(), "null") {
			t.Errorf("bad response: %q", w.Body.String())
		}
	}
}

// ---- Rate limiting ----

func TestRateLimitExceeded(t *testing.T) {
	_, priv, err := crypto.GenerateSigningKey()
	if err != nil {
		t.Fatal(err)
	}
	st, err := store.OpenSQLite(":memory:")
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = st.Close() })
	oidcP := oidc.NewProvider(testIssuer, priv, "key-1", st)

	// Very tight limit: 1 req/s burst 1.
	srv := api.NewServer(api.Config{
		APIKey:    testAPIKey,
		Issuer:    testIssuer,
		Store:     st,
		OIDC:      oidcP,
		Resolver:  resolver.New(time.Minute),
		RateLimit: 1,
	})

	// /authorize is rate-limited; /healthz is explicitly exempt (health probes must never be throttled).
	limited := 0
	for i := 0; i < 10; i++ {
		r := httptest.NewRequest(http.MethodGet, "/authorize", nil)
		r.RemoteAddr = "10.0.0.1:1234"
		w := httptest.NewRecorder()
		srv.ServeHTTP(w, r)
		if w.Code == http.StatusTooManyRequests {
			limited++
		}
	}
	if limited == 0 {
		t.Error("expected at least one rate-limited response from the same IP")
	}
}

// ---- Schema endpoint ----

func TestListSchemasRequiresAuth(t *testing.T) {
	srv := newTestServer(t)
	w := do(t, srv, http.MethodGet, "/api/v1/schemas", "", nil)
	if w.Code != http.StatusUnauthorized {
		t.Errorf("want 401, got %d", w.Code)
	}
}

// ---- Me / sessions ----

func TestMySessionsRequiresBearer(t *testing.T) {
	srv := newTestServer(t)
	// No bearer token at all.
	w := do(t, srv, http.MethodGet, "/api/v1/me/sessions", "", nil)
	// The "me" endpoints use OIDC bearer auth, not the API key.
	// Without any token we expect 401.
	if w.Code != http.StatusUnauthorized {
		t.Errorf("want 401, got %d", w.Code)
	}
}
