package oidc

import (
	"crypto/ed25519"
	"encoding/json"
	"net/http"
	"strings"
)

// handleTokenExchange implements RFC 8693 Token Exchange.
//
// Grant type: urn:ietf:params:oauth:grant-type:token-exchange
//
// Supports:
//   - Impersonation: caller presents an access token and receives a new token
//     for the same subject but scoped to the caller's audience.
//   - Delegation: caller narrows scope of an existing token on behalf of a subject.
//
// Required params:
//   - subject_token — the token being exchanged (must be a valid access token)
//   - subject_token_type — urn:ietf:params:oauth:token-type:access_token
//
// Optional params:
//   - actor_token / actor_token_type — the service making the request (recorded as `act`)
//   - scope — requested scope (must be subset of subject_token scope)
//   - audience — intended audience of the issued token
func (p *Provider) handleTokenExchange(w http.ResponseWriter, r *http.Request) {
	const ttAccessToken = "urn:ietf:params:oauth:token-type:access_token"

	subjectToken := r.FormValue("subject_token")
	subjectTokenType := r.FormValue("subject_token_type")
	requestedTokenType := r.FormValue("requested_token_type")
	requestedScope := r.FormValue("scope")
	audience := r.FormValue("audience")
	actorToken := r.FormValue("actor_token")

	if subjectToken == "" {
		writeError(w, "invalid_request", "subject_token required")
		return
	}
	if subjectTokenType != ttAccessToken {
		writeError(w, "invalid_request", "subject_token_type must be "+ttAccessToken)
		return
	}
	if requestedTokenType != "" && requestedTokenType != ttAccessToken {
		writeError(w, "invalid_request", "requested_token_type must be "+ttAccessToken)
		return
	}

	// Verify the subject token.
	claims, err := Verify(subjectToken, p.signingPub)
	if err != nil {
		writeError(w, "invalid_request", "subject_token invalid: "+err.Error())
		return
	}

	// Authenticate the calling client.
	callerClientID := ""
	if user, _, ok := r.BasicAuth(); ok {
		callerClientID = user
	} else if id := r.FormValue("client_id"); id != "" {
		callerClientID = id
	}
	if callerClientID != "" {
		if err := p.validateClientAuth(r, callerClientID); err != nil {
			writeError(w, "invalid_client", err.Error())
			return
		}
	}

	// Scope: use requested_scope if a subset, otherwise carry subject token scope.
	effectiveScope := claims.Scope
	if requestedScope != "" {
		if !scopeIsSubset(requestedScope, claims.Scope) {
			writeError(w, "invalid_scope", "requested scope exceeds subject_token scope")
			return
		}
		effectiveScope = requestedScope
	}

	if audience == "" {
		audience = p.issuer
	}

	// Build extra claims for the new token.
	extra := map[string]any{
		"aud": audience,
	}
	if effectiveScope != "" {
		extra["scope"] = effectiveScope
	}

	// Record the actor (the service making the exchange).
	if actorToken != "" {
		actClaims, actErr := Verify(actorToken, p.signingPub)
		if actErr == nil {
			extra["act"] = map[string]string{"sub": actClaims.Subject}
		}
	} else if callerClientID != "" {
		extra["act"] = map[string]string{"sub": callerClientID}
	}

	accessToken, err := issueJWTWithExtra(p.issuer, claims.Subject, audience, accessTokenTTL, ed25519.PrivateKey(p.signingKey), p.keyID, extra)
	if err != nil {
		http.Error(w, "internal error", http.StatusInternalServerError)
		return
	}

	w.Header().Set("Content-Type", "application/json")
	w.Header().Set("Cache-Control", "no-store")
	json.NewEncoder(w).Encode(map[string]any{
		"access_token":      accessToken,
		"issued_token_type": ttAccessToken,
		"token_type":        "Bearer",
		"expires_in":        int(accessTokenTTL.Seconds()),
		"scope":             effectiveScope,
	})
}

// scopeIsSubset returns true if every space-separated scope in requested is
// present in allowed.
func scopeIsSubset(requested, allowed string) bool {
	allowedSet := make(map[string]bool)
	for _, s := range strings.Fields(allowed) {
		allowedSet[s] = true
	}
	for _, s := range strings.Fields(requested) {
		if !allowedSet[s] {
			return false
		}
	}
	return true
}
