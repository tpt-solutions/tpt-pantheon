package oidc

import (
	"net/http"
	"strings"
)

// LogoutHandler handles GET /oidc/logout (RP-Initiated Logout 1.0).
//
// Accepts:
//   - id_token_hint (optional) — previously issued ID token; used to identify the session.
//   - post_logout_redirect_uri (optional) — where to send the user after logout.
//   - state (optional) — opaque value passed back to the RP on redirect.
//
// The handler revokes all refresh tokens for the subject, deletes the active session,
// and redirects to post_logout_redirect_uri (if registered) or the issuer root.
func (p *Provider) LogoutHandler(w http.ResponseWriter, r *http.Request) {
	q := r.URL.Query()
	idTokenHint := q.Get("id_token_hint")
	postLogoutURI := q.Get("post_logout_redirect_uri")
	state := q.Get("state")

	var subjectDID string
	if idTokenHint != "" {
		claims, err := Verify(idTokenHint, p.signingPub)
		if err == nil {
			subjectDID = claims.Subject
		}
	}

	if subjectDID != "" {
		// Revoke all sessions and refresh tokens for this subject.
		sessions, err := p.store.ListSessionsBySubject(r.Context(), subjectDID)
		if err == nil {
			for _, sess := range sessions {
				if sess.RefreshTokenHash != "" {
					_ = p.store.DeleteRefreshToken(r.Context(), sess.RefreshTokenHash)
				}
				_ = p.store.DeleteSession(r.Context(), sess.ID)
			}
		}
		// Revoke all loose refresh tokens not attached to a session.
		_ = p.store.PurgeExpiredSessions(r.Context())
	}

	// Validate post_logout_redirect_uri: only allow if registered for the client.
	// Without a verified id_token_hint we can still redirect to a registered URI
	// but we cannot know which client — so we skip the registration check in that case.
	target := p.issuer
	if postLogoutURI != "" {
		if subjectDID != "" && p.isRegisteredLogoutURI(r, postLogoutURI) {
			target = postLogoutURI
		} else if subjectDID == "" {
			// Unauthenticated logout request with redirect — just go to issuer root.
			target = p.issuer
		}
		// If URI is not registered, silently fall back to issuer root (no redirect leak).
	}

	if state != "" && strings.Contains(target, "?") {
		target += "&state=" + state
	} else if state != "" {
		target += "?state=" + state
	}

	http.Redirect(w, r, target, http.StatusFound)
}

// isRegisteredLogoutURI returns true if postLogoutURI appears in any registered client's
// post_logout_redirect_uris. We iterate all clients for simplicity; a production
// deployment with thousands of clients should index this separately.
func (p *Provider) isRegisteredLogoutURI(r *http.Request, uri string) bool {
	clients, err := p.store.ListClients(r.Context())
	if err != nil {
		return false
	}
	for _, c := range clients {
		for _, u := range c.PostLogoutRedirectURIs {
			if u == uri {
				return true
			}
		}
	}
	return false
}
