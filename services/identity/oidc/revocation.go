package oidc

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"net/http"
	"sync"
	"time"
)

// RevokedAccessToken tracks revoked access tokens until their natural expiry.
// In production this should be stored in Redis or the DB; the in-memory store
// here is suitable for single-instance deployments.
type revokedToken struct {
	expiresAt time.Time
}

var (
	revokedMu     sync.RWMutex
	revokedTokens = map[string]revokedToken{}
)

// IsAccessTokenRevoked reports whether the given access token has been revoked.
func IsAccessTokenRevoked(token string) bool {
	h := hashTokenValue(token)
	revokedMu.RLock()
	e, ok := revokedTokens[h]
	revokedMu.RUnlock()
	if !ok {
		return false
	}
	if time.Now().After(e.expiresAt) {
		// Lazily clean up.
		revokedMu.Lock()
		delete(revokedTokens, h)
		revokedMu.Unlock()
		return false
	}
	return true
}

// RevokeHandler handles POST /oidc/revoke (RFC 7009).
// Accepts both access_token and refresh_token hint values.
func (p *Provider) RevokeHandler(w http.ResponseWriter, r *http.Request) {
	if err := r.ParseForm(); err != nil {
		http.Error(w, "bad request", http.StatusBadRequest)
		return
	}

	token := r.FormValue("token")
	if token == "" {
		writeError(w, "invalid_request", "token parameter is required")
		return
	}
	hint := r.FormValue("token_type_hint") // "access_token" or "refresh_token"

	// Authenticate the client before revoking.
	clientID := r.FormValue("client_id")
	if clientID == "" {
		if user, _, ok := r.BasicAuth(); ok {
			clientID = user
		}
	}
	if clientID != "" {
		if err := p.validateClientAuth(r, clientID); err != nil {
			writeError(w, "invalid_client", err.Error())
			return
		}
	}

	// Try refresh token first (or if hint says so), then access token.
	revoked := false
	if hint == "" || hint == "refresh_token" {
		hash := hashToken(token)
		rt, err := p.store.GetRefreshToken(r.Context(), hash)
		if err == nil {
			if clientID != "" && rt.ClientID != clientID {
				// RFC 7009 §2.1: ignore if owned by different client (still return 200).
			} else {
				_ = p.store.DeleteRefreshToken(r.Context(), hash)
				revoked = true
			}
		}
	}
	if !revoked && (hint == "" || hint == "access_token") {
		// Parse expiry from the JWT without full verification (token may already be expired).
		claims, err := ParseUnverified(token)
		if err == nil {
			expiry := time.Unix(claims.ExpiresAt, 0)
			if time.Now().Before(expiry) {
				h := hashTokenValue(token)
				revokedMu.Lock()
				revokedTokens[h] = revokedToken{expiresAt: expiry}
				revokedMu.Unlock()
			}
		}
	}

	// RFC 7009 §2.2: always return 200, even if token was unknown.
	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(map[string]string{})
}

func hashTokenValue(token string) string {
	h := sha256.Sum256([]byte(token))
	return hex.EncodeToString(h[:])
}
