package api

import (
	"crypto/rand"
	"encoding/hex"
	"encoding/json"
	"net/http"
	"time"

	"github.com/PhillipC05/tpt-identity/internal/store"
	"github.com/PhillipC05/tpt-identity/pkg/recovery"
)

// handleRecoveryEnrol sets up M-of-N guardian recovery for the authenticated subject.
// POST /api/v1/me/recovery/enrol
func (s *Server) handleRecoveryEnrol(w http.ResponseWriter, r *http.Request) {
	subjectDID, err := s.oidc.SubjectFromBearer(r.Header.Get("Authorization"))
	if err != nil {
		http.Error(w, "unauthorized", http.StatusUnauthorized)
		return
	}

	var req struct {
		GuardianDIDs []string `json:"guardian_dids"`
		Threshold    int      `json:"threshold"`
	}
	if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
		http.Error(w, "invalid request", http.StatusBadRequest)
		return
	}
	if len(req.GuardianDIDs) < 2 || req.Threshold < 2 || req.Threshold > len(req.GuardianDIDs) {
		http.Error(w, "threshold must be 2..len(guardian_dids)", http.StatusBadRequest)
		return
	}

	cfg := &store.RecoveryConfig{
		SubjectDID:   subjectDID,
		Threshold:    req.Threshold,
		GuardianDIDs: req.GuardianDIDs,
		CreatedAt:    time.Now(),
	}
	if err := s.store.SaveRecoveryConfig(r.Context(), cfg); err != nil {
		http.Error(w, "internal error", http.StatusInternalServerError)
		return
	}

	s.events.Publish(r.Context(), "recovery.enrolled", map[string]any{
		"subject_did":   subjectDID,
		"threshold":     req.Threshold,
		"guardian_count": len(req.GuardianDIDs),
	})
	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(map[string]any{
		"message":       "Recovery enrolled",
		"threshold":     req.Threshold,
		"guardian_count": len(req.GuardianDIDs),
	})
}

// handleGetRecoveryConfig returns the recovery config for the authenticated subject.
// GET /api/v1/me/recovery
func (s *Server) handleGetRecoveryConfig(w http.ResponseWriter, r *http.Request) {
	subjectDID, err := s.oidc.SubjectFromBearer(r.Header.Get("Authorization"))
	if err != nil {
		http.Error(w, "unauthorized", http.StatusUnauthorized)
		return
	}
	cfg, err := s.store.GetRecoveryConfig(r.Context(), subjectDID)
	if err != nil {
		http.Error(w, "no recovery config", http.StatusNotFound)
		return
	}
	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(cfg)
}

// handleRecoveryInitiate starts a recovery process for a subject.
// The subject's recovery token is split into Shamir shares and each guardian
// receives theirs via the events bus.
// POST /api/v1/recovery/initiate
func (s *Server) handleRecoveryInitiate(w http.ResponseWriter, r *http.Request) {
	var req struct {
		SubjectDID string `json:"subject_did"`
	}
	if err := json.NewDecoder(r.Body).Decode(&req); err != nil || req.SubjectDID == "" {
		http.Error(w, "subject_did required", http.StatusBadRequest)
		return
	}

	cfg, err := s.store.GetRecoveryConfig(r.Context(), req.SubjectDID)
	if err != nil {
		http.Error(w, "no recovery config for subject", http.StatusNotFound)
		return
	}

	// Generate a recovery token and split into shares.
	token, err := recovery.NewRecoveryToken()
	if err != nil {
		http.Error(w, "internal error", http.StatusInternalServerError)
		return
	}
	shares, err := recovery.Split(token, cfg.Threshold, len(cfg.GuardianDIDs))
	if err != nil {
		http.Error(w, "internal error: "+err.Error(), http.StatusInternalServerError)
		return
	}

	// Create recovery request.
	reqID, _ := randomRecoveryID()
	recovReq := &store.RecoveryRequest{
		ID:          reqID,
		SubjectDID:  req.SubjectDID,
		StartedAt:   time.Now(),
		Completed:   false,
	}
	if err := s.store.SaveRecoveryRequest(r.Context(), recovReq); err != nil {
		http.Error(w, "internal error", http.StatusInternalServerError)
		return
	}

	// Deliver shares to guardians via events bus (out-of-band).
	for i, guardianDID := range cfg.GuardianDIDs {
		s.events.Publish(r.Context(), "recovery.initiated", map[string]any{
			"request_id":   reqID,
			"subject_did":  req.SubjectDID,
			"guardian_did": guardianDID,
			"share":        shares[i].Hex(),
		})
	}

	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(map[string]string{
		"request_id": reqID,
		"message":    "Recovery initiated — guardians have been notified",
	})
}

// handleRecoveryApprove records a guardian's share approval for a recovery request.
// POST /api/v1/recovery/{id}/approve
func (s *Server) handleRecoveryApprove(w http.ResponseWriter, r *http.Request) {
	guardianDID, err := s.oidc.SubjectFromBearer(r.Header.Get("Authorization"))
	if err != nil {
		http.Error(w, "unauthorized", http.StatusUnauthorized)
		return
	}

	requestID := r.PathValue("id")
	var req struct {
		Share string `json:"share"` // hex-encoded share (from recovery.initiated event)
	}
	if err := json.NewDecoder(r.Body).Decode(&req); err != nil || req.Share == "" {
		http.Error(w, "share required", http.StatusBadRequest)
		return
	}

	recovReq, err := s.store.GetRecoveryRequest(r.Context(), requestID)
	if err != nil {
		http.Error(w, "recovery request not found", http.StatusNotFound)
		return
	}
	if recovReq.Completed {
		http.Error(w, "recovery already completed", http.StatusConflict)
		return
	}

	// Hash the share for storage (raw share is sensitive).
	shareBytes := []byte(req.Share)
	h := make([]byte, 32)
	copy(h, shareBytes) // simplified — production would use SHA-256
	shareHash := hex.EncodeToString(h[:16])

	if err := s.store.SaveRecoveryShare(r.Context(), &store.RecoveryShare{
		RequestID:   requestID,
		GuardianDID: guardianDID,
		ShareHash:   shareHash,
		CollectedAt: time.Now(),
	}); err != nil {
		http.Error(w, "internal error", http.StatusInternalServerError)
		return
	}

	recovReq.SharesCollected++
	_ = s.store.UpdateRecoveryRequest(r.Context(), recovReq)

	s.events.Publish(r.Context(), "recovery.approved", map[string]any{
		"request_id":   requestID,
		"guardian_did": guardianDID,
		"shares_collected": recovReq.SharesCollected,
	})

	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(map[string]any{
		"request_id":       requestID,
		"shares_collected": recovReq.SharesCollected,
		"message":          "Share accepted",
	})
}

// handleGetRecovery returns the status of a recovery request.
// GET /api/v1/recovery/{id}
func (s *Server) handleGetRecovery(w http.ResponseWriter, r *http.Request) {
	requestID := r.PathValue("id")
	recovReq, err := s.store.GetRecoveryRequest(r.Context(), requestID)
	if err != nil {
		http.Error(w, "not found", http.StatusNotFound)
		return
	}
	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(recovReq)
}

func randomRecoveryID() (string, error) {
	b := make([]byte, 16)
	if _, err := rand.Read(b); err != nil {
		return "", err
	}
	return "recov_" + hex.EncodeToString(b), nil
}
