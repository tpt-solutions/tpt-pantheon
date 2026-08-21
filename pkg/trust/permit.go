// Package trust provides inter-service trust primitives: short-lived signed permits
// and federated reputation lookups.
package trust

import (
	"crypto/ed25519"
	"encoding/base64"
	"encoding/json"
	"errors"
	"fmt"
	"strings"
	"time"
)

// Permit is a short-lived, Ed25519-signed capability token issued by one TPT service
// to authorise a specific action on behalf of a subject DID.
//
// Example uses:
//   - tpt-email grants tpt-identity permission to read a user's verified email address
//   - tpt-identity grants tpt-healthcare permission to create a health record for a DID
type Permit struct {
	// Issuer is the DID of the service issuing the permit.
	Issuer string `json:"iss"`
	// Subject is the DID of the end-user the permit relates to.
	Subject string `json:"sub"`
	// Audience is the DID (or service name) the permit is granted to.
	Audience string `json:"aud"`
	// Action is the capability being granted, e.g. "read:email", "create:health-record".
	Action string `json:"action"`
	// IssuedAt is the Unix timestamp when the permit was signed.
	IssuedAt int64 `json:"iat"`
	// ExpiresAt is the Unix timestamp after which the permit is invalid.
	ExpiresAt int64 `json:"exp"`
	// JTI is a unique identifier to prevent replay (the signer should ensure uniqueness).
	JTI string `json:"jti,omitempty"`
}

// IssuePermit creates and signs a new permit.
func IssuePermit(
	issuerDID string,
	issuerKey ed25519.PrivateKey,
	subjectDID, audienceDID, action string,
	ttl time.Duration,
	jti string,
) (string, error) {
	now := time.Now()
	p := Permit{
		Issuer:    issuerDID,
		Subject:   subjectDID,
		Audience:  audienceDID,
		Action:    action,
		IssuedAt:  now.Unix(),
		ExpiresAt: now.Add(ttl).Unix(),
		JTI:       jti,
	}
	return signPermit(p, issuerKey)
}

// VerifyPermit parses and verifies a signed permit string.
// It checks the signature, expiry, expected issuer, audience, and action.
func VerifyPermit(token string, issuerPub ed25519.PublicKey, expectedAudience, expectedAction string) (*Permit, error) {
	p, sig, err := parsePermitToken(token)
	if err != nil {
		return nil, err
	}
	// Verify signature over header.payload.
	dot := strings.LastIndex(token, ".")
	if dot < 0 {
		return nil, errors.New("permit: malformed token")
	}
	sigInput := token[:dot]
	if !ed25519.Verify(issuerPub, []byte(sigInput), sig) {
		return nil, errors.New("permit: invalid signature")
	}
	if time.Now().Unix() > p.ExpiresAt {
		return nil, errors.New("permit: expired")
	}
	if expectedAudience != "" && p.Audience != expectedAudience {
		return nil, fmt.Errorf("permit: audience mismatch (got %q, want %q)", p.Audience, expectedAudience)
	}
	if expectedAction != "" && p.Action != expectedAction {
		return nil, fmt.Errorf("permit: action mismatch (got %q, want %q)", p.Action, expectedAction)
	}
	return p, nil
}

// signPermit serialises the permit as a compact token: base64url(header).base64url(payload).base64url(sig).
func signPermit(p Permit, key ed25519.PrivateKey) (string, error) {
	header := base64.RawURLEncoding.EncodeToString([]byte(`{"alg":"EdDSA","typ":"tpt-permit"}`))
	payload, err := json.Marshal(p)
	if err != nil {
		return "", fmt.Errorf("permit: marshal: %w", err)
	}
	payloadB64 := base64.RawURLEncoding.EncodeToString(payload)
	sigInput := header + "." + payloadB64
	sig := ed25519.Sign(key, []byte(sigInput))
	return sigInput + "." + base64.RawURLEncoding.EncodeToString(sig), nil
}

func parsePermitToken(token string) (*Permit, []byte, error) {
	parts := strings.SplitN(token, ".", 3)
	if len(parts) != 3 {
		return nil, nil, errors.New("permit: malformed token (expected 3 parts)")
	}
	payloadBytes, err := base64.RawURLEncoding.DecodeString(parts[1])
	if err != nil {
		return nil, nil, fmt.Errorf("permit: decode payload: %w", err)
	}
	var p Permit
	if err := json.Unmarshal(payloadBytes, &p); err != nil {
		return nil, nil, fmt.Errorf("permit: unmarshal payload: %w", err)
	}
	sig, err := base64.RawURLEncoding.DecodeString(parts[2])
	if err != nil {
		return nil, nil, fmt.Errorf("permit: decode signature: %w", err)
	}
	return &p, sig, nil
}
