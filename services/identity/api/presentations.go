package api

import (
	"crypto/rand"
	"encoding/hex"
	"encoding/json"
	"net/http"
	"time"

	"github.com/PhillipC05/tpt-identity/pkg/pe"
	"github.com/PhillipC05/tpt-identity/pkg/vc"
)

type presentationRequestBody struct {
	Definition pe.PresentationDefinition `json:"presentation_definition"`
	Domain     string                    `json:"domain,omitempty"`
}

type presentationRequestResponse struct {
	RequestID  string                    `json:"request_id"`
	Nonce      string                    `json:"nonce"`
	Definition pe.PresentationDefinition `json:"presentation_definition"`
	ExpiresAt  string                    `json:"expires_at"`
}

// handleCreatePresentationRequest creates a presentation request from a verifier.
// POST /api/v1/presentations/request
func (s *Server) handleCreatePresentationRequest(w http.ResponseWriter, r *http.Request) {
	var body presentationRequestBody
	if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
		http.Error(w, "invalid request", http.StatusBadRequest)
		return
	}
	if body.Definition.ID == "" || len(body.Definition.InputDescriptors) == 0 {
		http.Error(w, "presentation_definition with at least one input_descriptor required", http.StatusBadRequest)
		return
	}

	nonce, err := randomNonce(16)
	if err != nil {
		http.Error(w, "internal error", http.StatusInternalServerError)
		return
	}

	req := pe.PresentationRequest{
		ID:         "preq_" + nonce[:16],
		Definition: body.Definition,
		Nonce:      nonce,
		Domain:     body.Domain,
	}

	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(http.StatusCreated)
	json.NewEncoder(w).Encode(presentationRequestResponse{
		RequestID:  req.ID,
		Nonce:      req.Nonce,
		Definition: req.Definition,
		ExpiresAt:  time.Now().Add(10 * time.Minute).UTC().Format(time.RFC3339),
	})
}

type submitPresentationBody struct {
	// Definition must be embedded in the submit body for now.
	// In production, definitions are stored server-side by request ID.
	Definition   pe.PresentationDefinition  `json:"presentation_definition"`
	Presentation vc.VerifiablePresentation  `json:"presentation"`
	Submission   pe.PresentationSubmission  `json:"submission"`
}

// handleSubmitPresentation evaluates a holder's VP against a presentation definition.
// POST /api/v1/presentations/submit
func (s *Server) handleSubmitPresentation(w http.ResponseWriter, r *http.Request) {
	var body submitPresentationBody
	if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
		http.Error(w, "invalid request", http.StatusBadRequest)
		return
	}
	if body.Definition.ID == "" {
		http.Error(w, "presentation_definition required", http.StatusBadRequest)
		return
	}

	result, err := pe.Evaluate(&body.Definition, &body.Submission, &body.Presentation)
	if err != nil {
		http.Error(w, "evaluation error: "+err.Error(), http.StatusBadRequest)
		return
	}

	status := http.StatusOK
	if !result.Satisfied {
		status = http.StatusUnprocessableEntity
	}

	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(status)
	json.NewEncoder(w).Encode(map[string]any{
		"satisfied":           result.Satisfied,
		"errors":              result.Errors,
		"matched_credentials": len(result.MatchedCredentials),
	})
}

func randomNonce(n int) (string, error) {
	b := make([]byte, n)
	if _, err := rand.Read(b); err != nil {
		return "", err
	}
	return hex.EncodeToString(b), nil
}
