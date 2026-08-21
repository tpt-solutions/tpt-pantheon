package oidc

import (
	"crypto/ed25519"
	"encoding/base64"
	"encoding/json"
	"fmt"
	"strings"
	"time"
)

// Claims is the payload of an OIDC ID token or access token.
type Claims struct {
	Issuer    string   `json:"iss"`
	Subject   string   `json:"sub"` // the user's DID
	Audience  string   `json:"aud"`
	IssuedAt  int64    `json:"iat"`
	ExpiresAt int64    `json:"exp"`
	Nonce     string   `json:"nonce,omitempty"`
	DID       string   `json:"did"`                  // tpt-identity extension: the full DID
	AMR       []string `json:"amr,omitempty"`        // Authentication Methods References (RFC 8176)
	ACR       string   `json:"acr,omitempty"`        // Authentication Context Class Reference
	TokenType string   `json:"token_type,omitempty"` // "access" or "id" — internal discrimination
	Scope     string   `json:"scope,omitempty"`      // granted scope (for token exchange / introspection)
}

// IssueIDToken signs an OIDC ID token using Ed25519 (EdDSA).
func IssueIDToken(issuer, subjectDID, audience, nonce string, ttl time.Duration, key ed25519.PrivateKey, amr []string, keyID string) (string, error) {
	now := time.Now()
	claims := Claims{
		Issuer:    issuer,
		Subject:   subjectDID,
		Audience:  audience,
		IssuedAt:  now.Unix(),
		ExpiresAt: now.Add(ttl).Unix(),
		Nonce:     nonce,
		DID:       subjectDID,
		AMR:       amr,
		TokenType: "id",
	}
	return signJWT(claims, key, keyID)
}

// IssueAccessToken signs a short-lived access token.
func IssueAccessToken(issuer, subjectDID, audience string, ttl time.Duration, key ed25519.PrivateKey, keyID string) (string, error) {
	now := time.Now()
	claims := Claims{
		Issuer:    issuer,
		Subject:   subjectDID,
		Audience:  audience,
		IssuedAt:  now.Unix(),
		ExpiresAt: now.Add(ttl).Unix(),
		DID:       subjectDID,
		TokenType: "access",
	}
	return signJWT(claims, key, keyID)
}

func signJWT(claims any, key ed25519.PrivateKey, keyID string) (string, error) {
	headerJSON, _ := json.Marshal(map[string]string{"alg": "EdDSA", "typ": "JWT", "kid": keyID})
	header := base64.RawURLEncoding.EncodeToString(headerJSON)

	payload, err := json.Marshal(claims)
	if err != nil {
		return "", fmt.Errorf("jwt marshal: %w", err)
	}
	payloadB64 := base64.RawURLEncoding.EncodeToString(payload)
	sigInput := header + "." + payloadB64
	sig := ed25519.Sign(key, []byte(sigInput))
	return sigInput + "." + base64.RawURLEncoding.EncodeToString(sig), nil
}

// ParseUnverified parses a JWT without verifying its signature (for reading claims).
func ParseUnverified(token string) (*Claims, error) {
	parts := strings.SplitN(token, ".", 3)
	if len(parts) != 3 {
		return nil, fmt.Errorf("jwt: malformed token")
	}
	payload, err := base64.RawURLEncoding.DecodeString(parts[1])
	if err != nil {
		return nil, fmt.Errorf("jwt: decode payload: %w", err)
	}
	var c Claims
	if err := json.Unmarshal(payload, &c); err != nil {
		return nil, fmt.Errorf("jwt: unmarshal: %w", err)
	}
	return &c, nil
}

// Verify checks the JWT signature and validates expiry.
func Verify(token string, pub ed25519.PublicKey) (*Claims, error) {
	parts := strings.SplitN(token, ".", 3)
	if len(parts) != 3 {
		return nil, fmt.Errorf("jwt: malformed token")
	}
	sig, err := base64.RawURLEncoding.DecodeString(parts[2])
	if err != nil {
		return nil, fmt.Errorf("jwt: decode signature: %w", err)
	}
	if !ed25519.Verify(pub, []byte(parts[0]+"."+parts[1]), sig) {
		return nil, fmt.Errorf("jwt: invalid signature")
	}
	claims, err := ParseUnverified(token)
	if err != nil {
		return nil, err
	}
	if time.Now().Unix() > claims.ExpiresAt {
		return nil, fmt.Errorf("jwt: token expired")
	}
	return claims, nil
}

// issueJWTWithExtra issues an access token with additional arbitrary claims merged
// in at the top level of the payload. Used by token exchange and step-up auth.
func issueJWTWithExtra(issuer, subject, clientID string, ttl time.Duration, key any, keyID string, extra map[string]any) (string, error) {
	edKey, ok := key.(ed25519.PrivateKey)
	if !ok {
		return "", fmt.Errorf("jwt: key must be ed25519.PrivateKey")
	}
	now := time.Now()
	// Build base claims as a map so we can merge extras freely.
	payload := map[string]any{
		"iss":        issuer,
		"sub":        subject,
		"aud":        clientID,
		"iat":        now.Unix(),
		"exp":        now.Add(ttl).Unix(),
		"did":        subject,
		"token_type": "access",
	}
	for k, v := range extra {
		payload[k] = v
	}
	return signJWT(payload, edKey, keyID)
}

// ParseHeader returns the header claims of a JWT (alg, kid, typ) without verifying.
func ParseHeader(token string) (map[string]string, error) {
	parts := strings.SplitN(token, ".", 3)
	if len(parts) != 3 {
		return nil, fmt.Errorf("jwt: malformed token")
	}
	raw, err := base64.RawURLEncoding.DecodeString(parts[0])
	if err != nil {
		return nil, fmt.Errorf("jwt: decode header: %w", err)
	}
	var h map[string]string
	if err := json.Unmarshal(raw, &h); err != nil {
		return nil, fmt.Errorf("jwt: unmarshal header: %w", err)
	}
	return h, nil
}
