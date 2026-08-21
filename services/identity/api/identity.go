package api

import (
	"encoding/json"
	"net/http"
	"time"

	"github.com/PhillipC05/tpt-identity/internal/store"
	"github.com/PhillipC05/tpt-identity/pkg/did"
	"github.com/PhillipC05/tpt-identity/pkg/keystore"
)

type createIdentityRequest struct {
	Method     string `json:"method"`     // "web", "key", "peer"
	Domain     string `json:"domain"`     // required for did:web
	Passphrase string `json:"passphrase"` // for key encryption
}

func (s *Server) handleCreateIdentity(w http.ResponseWriter, r *http.Request) {
	var req createIdentityRequest
	if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
		http.Error(w, "bad request: "+err.Error(), http.StatusBadRequest)
		return
	}
	ks, err := keystore.Generate()
	if err != nil {
		http.Error(w, "keygen failed: "+err.Error(), http.StatusInternalServerError)
		return
	}
	id, doc, err := did.Create(req.Method, did.CreateOptions{
		Domain:     req.Domain,
		SigningPub:  ks.SigningPub,
		EncPub:     ks.EncPub[:],
	})
	if err != nil {
		http.Error(w, "create DID: "+err.Error(), http.StatusBadRequest)
		return
	}
	now := time.Now()
	identity := &store.Identity{
		DID:       id,
		Method:    req.Method,
		CreatedAt: now,
		UpdatedAt: now,
	}
	if err := s.store.SaveIdentity(r.Context(), identity); err != nil {
		http.Error(w, "save identity: "+err.Error(), http.StatusInternalServerError)
		return
	}
	if err := s.store.SaveDocument(r.Context(), doc); err != nil {
		http.Error(w, "save document: "+err.Error(), http.StatusInternalServerError)
		return
	}
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(http.StatusCreated)
	json.NewEncoder(w).Encode(map[string]any{
		"did":      id,
		"document": doc,
	})
}

func (s *Server) handleGetIdentity(w http.ResponseWriter, r *http.Request) {
	id := r.PathValue("did")
	doc, err := s.resolver.Resolve(id)
	if err != nil {
		http.Error(w, "resolve: "+err.Error(), http.StatusNotFound)
		return
	}
	w.Header().Set("Content-Type", "application/did+json")
	json.NewEncoder(w).Encode(doc)
}
