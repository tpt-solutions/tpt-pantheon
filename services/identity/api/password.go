package api

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"net/http"
	"strings"
	"time"

	"github.com/PhillipC05/tpt-identity/internal/store"
)

// passwordBridge is the interface subset used by the password change/reset handlers.
// The concrete type is *providers.PasswordBridge.
type passwordBridge interface {
	VerifyCredentials(ctx context.Context, identifier, password string) (any, error)
	SetPassword(ctx context.Context, identifier, password string) error
}

// handlePasswordChange handles POST /auth/password/change.
//
// Requires a valid OIDC bearer token (proves identity) plus the current password.
// Body (JSON): { "current_password": "...", "new_password": "..." }
func (s *Server) handlePasswordChange(w http.ResponseWriter, r *http.Request) {
	subjectDID, err := s.oidc.SubjectFromBearer(r.Header.Get("Authorization"))
	if err != nil {
		writeJSONError(w, http.StatusUnauthorized, "unauthorized", err.Error())
		return
	}

	var body struct {
		CurrentPassword string `json:"current_password"`
		NewPassword     string `json:"new_password"`
	}
	if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
		writeJSONError(w, http.StatusBadRequest, "invalid_request", "invalid JSON body")
		return
	}
	if body.CurrentPassword == "" || body.NewPassword == "" {
		writeJSONError(w, http.StatusBadRequest, "invalid_request", "current_password and new_password required")
		return
	}

	raw, ok := s.bridges.Get("password")
	if !ok {
		writeJSONError(w, http.StatusBadRequest, "not_supported", "password bridge not enabled")
		return
	}
	pwdBridge, ok := raw.(passwordBridge)
	if !ok {
		writeJSONError(w, http.StatusInternalServerError, "internal_error", "password bridge unavailable")
		return
	}

	if _, err := pwdBridge.VerifyCredentials(r.Context(), subjectDID, body.CurrentPassword); err != nil {
		_ = s.lockout.RecordFailure(r.Context(), subjectDID)
		writeJSONError(w, http.StatusUnauthorized, "invalid_credentials", "current password is incorrect")
		return
	}
	s.lockout.RecordSuccess(r.Context(), subjectDID)

	if err := pwdBridge.SetPassword(r.Context(), subjectDID, body.NewPassword); err != nil {
		writeJSONError(w, http.StatusBadRequest, "invalid_request", err.Error())
		return
	}

	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(map[string]string{"status": "ok"})
}

// handlePasswordResetRequest handles POST /auth/password/reset/request.
//
// Body (JSON): { "identifier": "user@example.com" }
// Always returns 200 to avoid identifier enumeration.
func (s *Server) handlePasswordResetRequest(w http.ResponseWriter, r *http.Request) {
	var body struct {
		Identifier string `json:"identifier"`
	}
	if err := json.NewDecoder(r.Body).Decode(&body); err != nil || body.Identifier == "" {
		writeJSONError(w, http.StatusBadRequest, "invalid_request", "identifier required")
		return
	}

	identifier := strings.ToLower(strings.TrimSpace(body.Identifier))

	// Rate-limit: 3 reset attempts per identifier per lockout window.
	count, _, _ := s.store.GetAuthFailures(r.Context(), "reset:"+identifier)
	if count < 3 {
		rawToken, err := randomHex(32)
		if err == nil {
			h := sha256.Sum256([]byte(rawToken))
			hash := hex.EncodeToString(h[:])
			tok := &store.PasswordResetToken{
				Hash:       hash,
				Identifier: identifier,
				ExpiresAt:  time.Now().Add(15 * time.Minute),
				CreatedAt:  time.Now(),
			}
			if saveErr := s.store.SavePasswordResetToken(r.Context(), tok); saveErr == nil {
				// Publish event — email notification subscriber delivers the reset link.
				s.events.Publish(r.Context(), "password.reset_requested", map[string]string{
					"identifier": identifier,
					"token":      rawToken,
					"expires_at": tok.ExpiresAt.Format(time.RFC3339),
				})
			}
		}
		_ = s.store.RecordAuthFailure(r.Context(), "reset:"+identifier)
	}

	// Always return 200 — do not reveal whether identifier exists.
	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(map[string]string{
		"status":  "ok",
		"message": "If this identifier is registered, a reset link has been sent.",
	})
}

// handlePasswordResetConfirm handles POST /auth/password/reset/confirm.
//
// Body (JSON): { "token": "...", "new_password": "..." }
func (s *Server) handlePasswordResetConfirm(w http.ResponseWriter, r *http.Request) {
	var body struct {
		Token       string `json:"token"`
		NewPassword string `json:"new_password"`
	}
	if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
		writeJSONError(w, http.StatusBadRequest, "invalid_request", "invalid JSON body")
		return
	}
	if body.Token == "" || body.NewPassword == "" {
		writeJSONError(w, http.StatusBadRequest, "invalid_request", "token and new_password required")
		return
	}

	h := sha256.Sum256([]byte(body.Token))
	hash := hex.EncodeToString(h[:])

	tok, err := s.store.GetPasswordResetToken(r.Context(), hash)
	if err != nil {
		writeJSONError(w, http.StatusBadRequest, "invalid_token", "token not found or already used")
		return
	}
	if time.Now().After(tok.ExpiresAt) {
		_ = s.store.DeletePasswordResetToken(r.Context(), hash)
		writeJSONError(w, http.StatusBadRequest, "invalid_token", "token expired")
		return
	}

	raw, ok := s.bridges.Get("password")
	if !ok {
		writeJSONError(w, http.StatusBadRequest, "not_supported", "password bridge not enabled")
		return
	}
	pwdBridge, ok := raw.(passwordBridge)
	if !ok {
		writeJSONError(w, http.StatusInternalServerError, "internal_error", "password bridge unavailable")
		return
	}

	if err := pwdBridge.SetPassword(r.Context(), tok.Identifier, body.NewPassword); err != nil {
		writeJSONError(w, http.StatusBadRequest, "invalid_request", err.Error())
		return
	}

	_ = s.store.DeletePasswordResetToken(r.Context(), hash)
	_ = s.store.ClearAuthFailures(r.Context(), tok.Identifier)
	_ = s.store.ClearAuthFailures(r.Context(), "reset:"+tok.Identifier)

	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(map[string]string{"status": "ok"})
}
