package oidc

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"net/http"
	"time"

	"github.com/PhillipC05/tpt-identity/internal/store"
)

// RegisterClientRequest is the RFC 7591 client registration request body.
type RegisterClientRequest struct {
	ClientName              string   `json:"client_name"`
	RedirectURIs            []string `json:"redirect_uris"`
	TokenEndpointAuthMethod string   `json:"token_endpoint_auth_method,omitempty"`
	GrantTypes              []string `json:"grant_types,omitempty"`
	ResponseTypes           []string `json:"response_types,omitempty"`
	Scope                   string   `json:"scope,omitempty"`
	BackchannelLogoutURI    string   `json:"backchannel_logout_uri,omitempty"`
}

// RegisterClientResponse is the RFC 7591 client registration response.
type RegisterClientResponse struct {
	ClientID                string   `json:"client_id"`
	ClientSecret            string   `json:"client_secret,omitempty"`
	ClientName              string   `json:"client_name"`
	RedirectURIs            []string `json:"redirect_uris"`
	TokenEndpointAuthMethod string   `json:"token_endpoint_auth_method"`
	GrantTypes              []string `json:"grant_types"`
	ResponseTypes           []string `json:"response_types"`
	BackchannelLogoutURI    string   `json:"backchannel_logout_uri,omitempty"`
	ClientIDIssuedAt        int64    `json:"client_id_issued_at"`
}

// RegisterClientHandler handles POST /oidc/register (RFC 7591).
func (p *Provider) RegisterClientHandler(w http.ResponseWriter, r *http.Request) {
	var req RegisterClientRequest
	if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
		writeError(w, "invalid_client_metadata", "invalid request body")
		return
	}
	if len(req.RedirectURIs) == 0 {
		writeError(w, "invalid_client_metadata", "redirect_uris required")
		return
	}
	if req.ClientName == "" {
		writeError(w, "invalid_client_metadata", "client_name required")
		return
	}

	authMethod := req.TokenEndpointAuthMethod
	if authMethod == "" {
		authMethod = "client_secret_basic"
	}
	grantTypes := req.GrantTypes
	if len(grantTypes) == 0 {
		grantTypes = []string{"authorization_code", "refresh_token"}
	}
	responseTypes := req.ResponseTypes
	if len(responseTypes) == 0 {
		responseTypes = []string{"code"}
	}

	clientID, err := randomHex(16)
	if err != nil {
		http.Error(w, "internal error", http.StatusInternalServerError)
		return
	}

	var clientSecret string
	var secretHash string
	if authMethod != "none" {
		clientSecret, err = randomHex(32)
		if err != nil {
			http.Error(w, "internal error", http.StatusInternalServerError)
			return
		}
		secretHash = hashSecret(clientSecret)
	}

	client := &store.OIDCClient{
		ClientID:                clientID,
		ClientSecretHash:        secretHash,
		ClientName:              req.ClientName,
		RedirectURIs:            req.RedirectURIs,
		TokenEndpointAuthMethod: authMethod,
		GrantTypes:              grantTypes,
		ResponseTypes:           responseTypes,
		Scope:                   req.Scope,
		BackchannelLogoutURI:    req.BackchannelLogoutURI,
		CreatedAt:               time.Now(),
	}
	if err := p.store.SaveClient(r.Context(), client); err != nil {
		http.Error(w, "internal error", http.StatusInternalServerError)
		return
	}

	resp := RegisterClientResponse{
		ClientID:                clientID,
		ClientName:              req.ClientName,
		RedirectURIs:            req.RedirectURIs,
		TokenEndpointAuthMethod: authMethod,
		GrantTypes:              grantTypes,
		ResponseTypes:           responseTypes,
		BackchannelLogoutURI:    req.BackchannelLogoutURI,
		ClientIDIssuedAt:        time.Now().Unix(),
	}
	if clientSecret != "" {
		resp.ClientSecret = clientSecret
	}
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(http.StatusCreated)
	json.NewEncoder(w).Encode(resp)
}

func hashSecret(s string) string {
	h := sha256.Sum256([]byte(s))
	return hex.EncodeToString(h[:])
}
