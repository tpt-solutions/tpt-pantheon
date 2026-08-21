package api

import (
	"context"
	"crypto/hmac"
	"crypto/sha256"
	"crypto/subtle"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"net/http"
	"net/url"
	"strings"
	"time"

	"github.com/PhillipC05/tpt-identity/internal/authn"
	"github.com/PhillipC05/tpt-identity/internal/bridge"
	bridgeproviders "github.com/PhillipC05/tpt-identity/internal/bridge/providers"
	"github.com/PhillipC05/tpt-identity/pkg/vc"
)

// ─── State token helpers ────────────────────────────────────────────────────
// A signed state token carries OIDC flow params and (for SAML) the AuthnRequest
// ID across the external provider round-trip, without a server-side state table.

type bridgeState struct {
	ClientID            string `json:"client_id"`
	RedirectURI         string `json:"redirect_uri"`
	Scope               string `json:"scope"`
	Nonce               string `json:"nonce"`
	OrigState           string `json:"orig_state,omitempty"`
	CodeChallenge       string `json:"code_challenge,omitempty"`
	CodeChallengeMethod string `json:"code_challenge_method,omitempty"`
	LinkFor             string `json:"link_for,omitempty"`  // non-empty = link to existing DID
	AuthnRequestID      string `json:"authn_request_id,omitempty"` // SAML: anti-replay
	CreatedAt           int64  `json:"created_at"`
}

func (s *Server) signBridgeState(st bridgeState) (string, error) {
	b, err := json.Marshal(st)
	if err != nil {
		return "", err
	}
	payload := base64.RawURLEncoding.EncodeToString(b)
	mac := hmac.New(sha256.New, []byte(s.apiKey))
	mac.Write([]byte(payload))
	sig := hex.EncodeToString(mac.Sum(nil))
	return payload + "." + sig, nil
}

func (s *Server) verifyBridgeState(token string) (*bridgeState, error) {
	parts := strings.SplitN(token, ".", 2)
	if len(parts) != 2 {
		return nil, fmt.Errorf("invalid state token")
	}
	mac := hmac.New(sha256.New, []byte(s.apiKey))
	mac.Write([]byte(parts[0]))
	expectedSig := hex.EncodeToString(mac.Sum(nil))
	if subtle.ConstantTimeCompare([]byte(parts[1]), []byte(expectedSig)) != 1 {
		return nil, fmt.Errorf("invalid state token signature")
	}
	raw, err := base64.RawURLEncoding.DecodeString(parts[0])
	if err != nil {
		return nil, err
	}
	var st bridgeState
	if err := json.Unmarshal(raw, &st); err != nil {
		return nil, err
	}
	if time.Now().Unix()-st.CreatedAt > 600 { // 10-min expiry
		return nil, fmt.Errorf("state token expired")
	}
	return &st, nil
}

// ─── Bridge start ────────────────────────────────────────────────────────────

// handleBridgeStart dispatches to the appropriate flow based on what interface
// the registered bridge implements.
// GET /auth/{provider}?client_id=...&redirect_uri=...&scope=...&nonce=...&state=...
func (s *Server) handleBridgeStart(w http.ResponseWriter, r *http.Request) {
	providerName := r.PathValue("provider")
	b, ok := s.bridges.Get(providerName)
	if !ok {
		http.Error(w, "unknown provider", http.StatusNotFound)
		return
	}
	if rb, ok := b.(bridge.RedirectBridge); ok {
		s.startRedirectBridge(w, r, rb)
		return
	}
	if sb, ok := b.(bridge.SAMLBridgeHandler); ok {
		s.startSAMLBridge(w, r, sb)
		return
	}
	http.Error(w, "provider does not support browser-based authentication", http.StatusBadRequest)
}

// startRedirectBridge handles the OIDC / OAuth2 authorization redirect.
func (s *Server) startRedirectBridge(w http.ResponseWriter, r *http.Request, rb bridge.RedirectBridge) {
	q := r.URL.Query()
	st := bridgeState{
		ClientID:            q.Get("client_id"),
		RedirectURI:         q.Get("redirect_uri"),
		Scope:               q.Get("scope"),
		Nonce:               q.Get("nonce"),
		OrigState:           q.Get("state"),
		CodeChallenge:       q.Get("code_challenge"),
		CodeChallengeMethod: q.Get("code_challenge_method"),
		CreatedAt:           time.Now().Unix(),
	}
	if subjectDID, err := s.oidc.SubjectFromBearer(r.Header.Get("Authorization")); err == nil {
		if q.Get("link") == "true" {
			st.LinkFor = subjectDID
		}
	}
	stateToken, err := s.signBridgeState(st)
	if err != nil {
		http.Error(w, "internal error", http.StatusInternalServerError)
		return
	}
	authURL, err := rb.AuthorizationURL(r.Context(), stateToken)
	if err != nil {
		http.Error(w, "upstream error: "+err.Error(), http.StatusBadGateway)
		return
	}
	http.Redirect(w, r, authURL, http.StatusFound)
}

// startSAMLBridge generates a SAML AuthnRequest and redirects to the IdP.
// The AuthnRequest ID is embedded in the signed RelayState for anti-replay.
func (s *Server) startSAMLBridge(w http.ResponseWriter, r *http.Request, sb bridge.SAMLBridgeHandler) {
	idpURL, requestID, err := sb.PrepareAuth(r.Context())
	if err != nil {
		http.Error(w, "saml error: "+err.Error(), http.StatusBadGateway)
		return
	}

	q := r.URL.Query()
	st := bridgeState{
		ClientID:       q.Get("client_id"),
		RedirectURI:    q.Get("redirect_uri"),
		Scope:          q.Get("scope"),
		Nonce:          q.Get("nonce"),
		OrigState:      q.Get("state"),
		AuthnRequestID: requestID,
		CreatedAt:      time.Now().Unix(),
	}
	relayState, err := s.signBridgeState(st)
	if err != nil {
		http.Error(w, "internal error", http.StatusInternalServerError)
		return
	}

	// Append RelayState to the URL returned by PrepareAuth.
	sep := "?"
	if strings.Contains(idpURL, "?") {
		sep = "&"
	}
	http.Redirect(w, r, idpURL+sep+"RelayState="+url.QueryEscape(relayState), http.StatusFound)
}

// ─── OIDC / OAuth2 callback ─────────────────────────────────────────────────

// handleBridgeCallback receives the callback from a redirect-based bridge.
// GET /auth/{provider}/callback?code=...&state=...
func (s *Server) handleBridgeCallback(w http.ResponseWriter, r *http.Request) {
	providerName := r.PathValue("provider")
	b, ok := s.bridges.Get(providerName)
	if !ok {
		http.Error(w, "unknown provider", http.StatusNotFound)
		return
	}
	rb, ok := b.(bridge.RedirectBridge)
	if !ok {
		http.Error(w, "provider does not support callback flow", http.StatusBadRequest)
		return
	}

	code := r.URL.Query().Get("code")
	rawState := r.URL.Query().Get("state")
	if code == "" || rawState == "" {
		http.Error(w, "missing code or state", http.StatusBadRequest)
		return
	}
	st, err := s.verifyBridgeState(rawState)
	if err != nil {
		http.Error(w, "invalid state: "+err.Error(), http.StatusBadRequest)
		return
	}
	ext, err := rb.ExchangeCode(r.Context(), code)
	if err != nil {
		http.Error(w, "upstream auth failed: "+err.Error(), http.StatusUnauthorized)
		return
	}
	s.finishBridgeAuth(w, r, ext, st, []string{"fed"})
}

// ─── SAML metadata and ACS ───────────────────────────────────────────────────

// handleSAMLMetadata serves the SP's SAML metadata XML.
// GET /auth/{provider}/metadata
func (s *Server) handleSAMLMetadata(w http.ResponseWriter, r *http.Request) {
	providerName := r.PathValue("provider")
	b, ok := s.bridges.Get(providerName)
	if !ok {
		http.Error(w, "unknown provider", http.StatusNotFound)
		return
	}
	sh, ok := b.(bridge.SAMLBridgeHandler)
	if !ok {
		http.Error(w, "provider is not a SAML provider", http.StatusBadRequest)
		return
	}
	sh.MetadataHandler(w, r)
}

// handleSAMLACS processes the SAMLResponse POST from the IdP.
// POST /auth/{provider}/acs
func (s *Server) handleSAMLACS(w http.ResponseWriter, r *http.Request) {
	providerName := r.PathValue("provider")
	b, ok := s.bridges.Get(providerName)
	if !ok {
		http.Error(w, "unknown provider", http.StatusNotFound)
		return
	}
	sh, ok := b.(bridge.SAMLBridgeHandler)
	if !ok {
		http.Error(w, "provider is not a SAML provider", http.StatusBadRequest)
		return
	}

	// ParseForm now so RelayState is in r.PostForm before ProcessACS reads SAMLResponse.
	if err := r.ParseForm(); err != nil {
		http.Error(w, "malformed form data", http.StatusBadRequest)
		return
	}

	// Recover the signed RelayState to get the AuthnRequest ID and OIDC params.
	var st *bridgeState
	var requestIDs []string
	if relayState := r.FormValue("RelayState"); relayState != "" {
		if parsed, err := s.verifyBridgeState(relayState); err == nil {
			st = parsed
			if st.AuthnRequestID != "" {
				requestIDs = []string{st.AuthnRequestID}
			}
		}
	}
	if st == nil {
		st = &bridgeState{}
	}

	ext, err := sh.ProcessACS(r, requestIDs)
	if err != nil {
		http.Error(w, "SAML authentication failed: "+err.Error(), http.StatusUnauthorized)
		return
	}
	s.finishBridgeAuth(w, r, ext, st, []string{"saml"})
}

// ─── Password bridge ────────────────────────────────────────────────────────

// handlePasswordAuth authenticates with an identifier and password.
// POST /auth/password
func (s *Server) handlePasswordAuth(w http.ResponseWriter, r *http.Request) {
	var req struct {
		Identifier          string `json:"identifier"`
		Password            string `json:"password"`
		ClientID            string `json:"client_id"`
		RedirectURI         string `json:"redirect_uri"`
		Scope               string `json:"scope"`
		Nonce               string `json:"nonce"`
		State               string `json:"state"`
		CodeChallenge       string `json:"code_challenge"`
		CodeChallengeMethod string `json:"code_challenge_method"`
	}
	if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
		http.Error(w, "invalid request", http.StatusBadRequest)
		return
	}
	if req.Identifier == "" || req.Password == "" {
		http.Error(w, "identifier and password required", http.StatusBadRequest)
		return
	}

	b, ok := s.bridges.Get("password")
	if !ok {
		http.Error(w, "password authentication not configured", http.StatusNotFound)
		return
	}
	pwBridge, ok := b.(*bridgeproviders.PasswordBridge)
	if !ok {
		http.Error(w, "password provider misconfigured", http.StatusInternalServerError)
		return
	}

	ext, err := pwBridge.VerifyCredentials(r.Context(), req.Identifier, req.Password)
	if err != nil {
		s.lockout.RecordFailure(r.Context(), req.Identifier)
		http.Error(w, "invalid credentials", http.StatusUnauthorized)
		return
	}
	s.lockout.RecordSuccess(r.Context(), req.Identifier)

	// Duress check: resolve the platform DID then verify whether the supplied
	// password also matches the enrolled duress passphrase.
	if subjectDID, _, findErr := s.mapper.FindOrCreate(r.Context(), ext); findErr == nil {
		duressManager := authn.NewDuressManager(s.store)
		if isDuress, _ := duressManager.CheckDuress(r.Context(), subjectDID, req.Password); isDuress {
			ext.Duress = true
		}
	}

	st := bridgeState{
		ClientID:            req.ClientID,
		RedirectURI:         req.RedirectURI,
		Scope:               req.Scope,
		Nonce:               req.Nonce,
		OrigState:           req.State,
		CodeChallenge:       req.CodeChallenge,
		CodeChallengeMethod: req.CodeChallengeMethod,
		CreatedAt:           time.Now().Unix(),
	}
	s.finishBridgeAuth(w, r, ext, &st, []string{"pwd"})
}

// ─── Magic Link bridge ───────────────────────────────────────────────────────

type magicLinkRequest struct {
	Email       string `json:"email"`
	ClientID    string `json:"client_id"`
	RedirectURI string `json:"redirect_uri"`
	Scope       string `json:"scope"`
	Nonce       string `json:"nonce"`
}

// handleMagicLinkRequest generates and "sends" a magic link token.
// POST /auth/magiclink/request
func (s *Server) handleMagicLinkRequest(w http.ResponseWriter, r *http.Request) {
	var req magicLinkRequest
	if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
		http.Error(w, "invalid request", http.StatusBadRequest)
		return
	}
	if req.Email == "" {
		http.Error(w, "email required", http.StatusBadRequest)
		return
	}

	ml := bridgeproviders.NewMagicLink(s.store)
	rawToken, expiresAt, err := ml.GenerateToken(r.Context(), req.Email)
	if err != nil {
		http.Error(w, "internal error", http.StatusInternalServerError)
		return
	}

	verifyURL := s.issuer + "/auth/magiclink/verify?token=" + rawToken
	if req.ClientID != "" {
		st := bridgeState{
			ClientID:    req.ClientID,
			RedirectURI: req.RedirectURI,
			Scope:       req.Scope,
			Nonce:       req.Nonce,
			CreatedAt:   time.Now().Unix(),
		}
		if stateToken, err := s.signBridgeState(st); err == nil {
			verifyURL += "&state=" + stateToken
		}
	}

	// TODO: integrate with tpt-email to deliver the magic link. For now return it
	// in the response (dev mode). In production this endpoint returns 202 with no URL.
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(http.StatusAccepted)
	json.NewEncoder(w).Encode(map[string]any{
		"expires_at": expiresAt.UTC().Format(time.RFC3339),
		"verify_url": verifyURL, // remove in production
	})
}

// handleMagicLinkVerify verifies a magic link token and issues a session.
// GET /auth/magiclink/verify?token=...&state=...
func (s *Server) handleMagicLinkVerify(w http.ResponseWriter, r *http.Request) {
	rawToken := r.URL.Query().Get("token")
	rawState := r.URL.Query().Get("state")
	if rawToken == "" {
		http.Error(w, "missing token", http.StatusBadRequest)
		return
	}

	ml := bridgeproviders.NewMagicLink(s.store)
	ext, err := ml.VerifyToken(r.Context(), rawToken)
	if err != nil {
		http.Error(w, "invalid or expired token", http.StatusUnauthorized)
		return
	}

	var st *bridgeState
	if rawState != "" {
		st, _ = s.verifyBridgeState(rawState)
	}
	if st == nil {
		st = &bridgeState{}
	}
	s.finishBridgeAuth(w, r, ext, st, []string{"email"})
}

// ─── Account linking ─────────────────────────────────────────────────────────

// handleListLinks lists all external providers linked to the authenticated DID.
// GET /api/v1/me/links
func (s *Server) handleListLinks(w http.ResponseWriter, r *http.Request) {
	subjectDID, err := s.oidc.SubjectFromBearer(r.Header.Get("Authorization"))
	if err != nil {
		http.Error(w, "unauthorized", http.StatusUnauthorized)
		return
	}
	links, err := s.store.ListExternalLinks(r.Context(), subjectDID)
	if err != nil {
		http.Error(w, "internal error", http.StatusInternalServerError)
		return
	}
	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(links)
}

// bootstrapCredentials is the bridge.BootstrapFunc called when a new platform
// identity is created from an external provider. It auto-issues VCs from any
// standard claims present in the external identity. Errors are logged only —
// a bootstrap failure never blocks authentication.
// This runs asynchronously; context.Background() is used so cancellation of
// the HTTP request doesn't abort in-flight issuance.
func (s *Server) bootstrapCredentials(_ context.Context, subjectDID string, ext *bridge.ExternalIdentity) {
	go func() {
		ctx := context.Background()
		s.maybeIssueBootstrapVC(ctx, subjectDID, "social.verified-contacts", func() map[string]string {
			email := ext.Claims["email"]
			if email == "" {
				return nil
			}
			return map[string]string{
				"contactType":  "email",
				"value":        email,
				"verifiedDate": time.Now().UTC().Format("2006-01-02"),
			}
		})
		s.maybeIssueBootstrapVC(ctx, subjectDID, "identity.legal-name", func() map[string]string {
			given := ext.Claims["given_name"]
			family := ext.Claims["family_name"]
			if given == "" || family == "" {
				return nil
			}
			return map[string]string{
				"givenNames": given,
				"familyName": family,
			}
		})
	}()
}

// maybeIssueBootstrapVC issues a VC for schemaID if claimsFn returns non-nil claims
// and the subject does not already hold a VC for that schema.
func (s *Server) maybeIssueBootstrapVC(ctx context.Context, subjectDID, schemaID string, claimsFn func() map[string]string) {
	claims := claimsFn()
	if claims == nil {
		return
	}
	existing, err := s.store.ListCredentials(ctx, subjectDID)
	if err == nil {
		for _, c := range existing {
			if c.CredentialSchema != nil && c.CredentialSchema.ID == schemaID {
				return // already issued
			}
		}
	}
	cred, err := vc.Issue(vc.IssueOptions{
		IssuerDID:            s.issuer,
		IssuerKey:            s.signingKey,
		VerificationMethodID: s.signingKeyID,
		SubjectDID:           subjectDID,
		SchemaID:             schemaID,
		Claims:               claims,
		ValidFor:             8760 * time.Hour, // 1 year
	})
	if err != nil {
		s.logger.Warn("bootstrap: issue vc failed", "schema", schemaID, "subject", subjectDID, "err", err)
		return
	}
	if err := s.store.SaveCredential(ctx, cred); err != nil {
		s.logger.Warn("bootstrap: save vc failed", "schema", schemaID, "subject", subjectDID, "err", err)
		return
	}
	s.events.Publish(ctx, "credential.issued", map[string]string{
		"id":         cred.ID,
		"subject":    subjectDID,
		"schema_id":  schemaID,
		"issuer_did": s.issuer,
		"source":     "bootstrap",
	})
}

// handleUnlink removes an external provider link.
// DELETE /api/v1/me/links/{provider}
func (s *Server) handleUnlink(w http.ResponseWriter, r *http.Request) {
	subjectDID, err := s.oidc.SubjectFromBearer(r.Header.Get("Authorization"))
	if err != nil {
		http.Error(w, "unauthorized", http.StatusUnauthorized)
		return
	}
	providerName := r.PathValue("provider")
	externalID := r.URL.Query().Get("external_id")
	if externalID == "" {
		http.Error(w, "external_id query param required", http.StatusBadRequest)
		return
	}
	if err := s.mapper.UnlinkIdentity(r.Context(), subjectDID, providerName, externalID); err != nil {
		http.Error(w, err.Error(), http.StatusBadRequest)
		return
	}
	w.WriteHeader(http.StatusNoContent)
}

// ─── Shared finish helper ────────────────────────────────────────────────────

func (s *Server) finishBridgeAuth(w http.ResponseWriter, r *http.Request, ext *bridge.ExternalIdentity, st *bridgeState, amr []string) {
	// Link mode: attach the new provider to an existing authenticated DID.
	if st.LinkFor != "" {
		if err := s.mapper.LinkIdentity(r.Context(), st.LinkFor, ext); err != nil {
			http.Error(w, "link failed: "+err.Error(), http.StatusConflict)
			return
		}
		w.Header().Set("Content-Type", "application/json")
		json.NewEncoder(w).Encode(map[string]string{"status": "linked", "provider": ext.Provider})
		return
	}

	// Find or create the platform DID for this external identity.
	subjectDID, _, err := s.mapper.FindOrCreate(r.Context(), ext)
	if err != nil {
		http.Error(w, "identity resolution failed: "+err.Error(), http.StatusInternalServerError)
		return
	}

	// If the user authenticated under duress, fire a silent security alert.
	if ext.Duress {
		s.events.Publish(r.Context(), "session.duress", map[string]string{
			"subject_did": subjectDID,
			"provider":    ext.Provider,
		})
	}

	// If OIDC flow params are present, issue an authorization code and redirect.
	if st.ClientID != "" && st.RedirectURI != "" {
		authCode, err := s.oidc.IssueSessionForDIDCtx(r, subjectDID, st.ClientID, st.RedirectURI, st.Scope, st.Nonce, st.CodeChallenge, st.CodeChallengeMethod, amr)
		if err != nil {
			http.Error(w, "session error: "+err.Error(), http.StatusInternalServerError)
			return
		}
		redirect := st.RedirectURI + "?code=" + authCode
		if st.OrigState != "" {
			redirect += "&state=" + st.OrigState
		}
		http.Redirect(w, r, redirect, http.StatusFound)
		return
	}

	// No OIDC flow — return the DID directly (API/CLI use case).
	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(map[string]string{"did": subjectDID, "provider": ext.Provider})
}
