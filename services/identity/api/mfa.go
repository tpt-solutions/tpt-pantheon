package api

import (
	"encoding/json"
	"net/http"

	"github.com/PhillipC05/tpt-identity/internal/authn"
)

// handleTOTPEnrol initiates TOTP enrolment for the authenticated subject.
// POST /api/v1/me/totp/enrol
func (s *Server) handleTOTPEnrol(w http.ResponseWriter, r *http.Request) {
	subjectDID, err := s.oidc.SubjectFromBearer(r.Header.Get("Authorization"))
	if err != nil {
		http.Error(w, "unauthorized", http.StatusUnauthorized)
		return
	}

	var req struct {
		AccountName string `json:"account_name"`
	}
	_ = json.NewDecoder(r.Body).Decode(&req)
	if req.AccountName == "" {
		req.AccountName = subjectDID
	}

	mgr := authn.NewTOTPManager(s.store, s.totpPassphrase, s.issuer)
	uri, err := mgr.Enrol(subjectDID, req.AccountName)
	if err != nil {
		http.Error(w, "enrolment failed: "+err.Error(), http.StatusInternalServerError)
		return
	}

	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(map[string]string{
		"otpauth_uri": uri,
		"message":     "Scan the URI with your authenticator app, then verify with POST /api/v1/me/totp/verify",
	})
}

// handleTOTPVerify verifies a TOTP code for the authenticated subject.
// POST /api/v1/me/totp/verify
func (s *Server) handleTOTPVerify(w http.ResponseWriter, r *http.Request) {
	subjectDID, err := s.oidc.SubjectFromBearer(r.Header.Get("Authorization"))
	if err != nil {
		http.Error(w, "unauthorized", http.StatusUnauthorized)
		return
	}

	var req struct {
		Code string `json:"code"`
	}
	if err := json.NewDecoder(r.Body).Decode(&req); err != nil || req.Code == "" {
		http.Error(w, "code required", http.StatusBadRequest)
		return
	}

	mgr := authn.NewTOTPManager(s.store, s.totpPassphrase, s.issuer)
	if err := mgr.Verify(subjectDID, req.Code); err != nil {
		s.lockout.RecordFailure(r.Context(), subjectDID)
		http.Error(w, "invalid code", http.StatusUnauthorized)
		return
	}

	s.lockout.RecordSuccess(r.Context(), subjectDID)
	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(map[string]string{"status": "verified"})
}

// handleTOTPUnenrol removes the TOTP credential for the authenticated subject.
// DELETE /api/v1/me/totp
func (s *Server) handleTOTPUnenrol(w http.ResponseWriter, r *http.Request) {
	subjectDID, err := s.oidc.SubjectFromBearer(r.Header.Get("Authorization"))
	if err != nil {
		http.Error(w, "unauthorized", http.StatusUnauthorized)
		return
	}

	mgr := authn.NewTOTPManager(s.store, s.totpPassphrase, s.issuer)
	if err := mgr.Unenrol(subjectDID); err != nil {
		http.Error(w, "unenrol failed: "+err.Error(), http.StatusInternalServerError)
		return
	}
	w.WriteHeader(http.StatusNoContent)
}
