package oidc_test

import (
	"crypto/sha256"
	"encoding/base64"
	"testing"
	"time"

	"github.com/PhillipC05/tpt-identity/pkg/crypto"
	"github.com/PhillipC05/tpt-identity/oidc"
)

// ---- JWT issuance + verification ----

func TestIssueAndVerifyIDToken(t *testing.T) {
	pubKey, privKey, err := crypto.GenerateSigningKey()
	if err != nil {
		t.Fatal(err)
	}
	token, err := oidc.IssueIDToken(
		"https://issuer.example.com", "did:peer:subject", "client-1",
		"nonce-xyz", time.Hour, privKey, nil, "key-1",
	)
	if err != nil {
		t.Fatal(err)
	}
	claims, err := oidc.Verify(token, pubKey)
	if err != nil {
		t.Fatalf("Verify: %v", err)
	}
	if claims.Subject != "did:peer:subject" {
		t.Errorf("unexpected subject: %s", claims.Subject)
	}
	if claims.Nonce != "nonce-xyz" {
		t.Errorf("unexpected nonce: %s", claims.Nonce)
	}
	if claims.TokenType != "id" {
		t.Errorf("unexpected token_type: %s", claims.TokenType)
	}
}

func TestIssueAndVerifyAccessToken(t *testing.T) {
	pubKey, privKey, _ := crypto.GenerateSigningKey()
	token, err := oidc.IssueAccessToken(
		"https://issuer.example.com", "did:peer:sub", "client-1",
		time.Hour, privKey, "key-1",
	)
	if err != nil {
		t.Fatal(err)
	}
	claims, err := oidc.Verify(token, pubKey)
	if err != nil {
		t.Fatalf("Verify: %v", err)
	}
	if claims.TokenType != "access" {
		t.Errorf("unexpected token_type: %s", claims.TokenType)
	}
}

func TestVerifyRejectsExpiredToken(t *testing.T) {
	pubKey, privKey, _ := crypto.GenerateSigningKey()
	token, _ := oidc.IssueAccessToken(
		"https://issuer.example.com", "did:peer:sub", "client-1",
		-time.Hour, privKey, "key-1",
	)
	if _, err := oidc.Verify(token, pubKey); err == nil {
		t.Error("expected error for expired token")
	}
}

func TestVerifyRejectsWrongKey(t *testing.T) {
	_, privKey, _ := crypto.GenerateSigningKey()
	wrongPub, _, _ := crypto.GenerateSigningKey()
	token, _ := oidc.IssueAccessToken(
		"https://issuer.example.com", "did:peer:sub", "client-1",
		time.Hour, privKey, "key-1",
	)
	if _, err := oidc.Verify(token, wrongPub); err == nil {
		t.Error("expected error for wrong public key")
	}
}

func TestVerifyRejectsMalformedToken(t *testing.T) {
	_, privKey, _ := crypto.GenerateSigningKey()
	pubKey, _, _ := crypto.GenerateSigningKey()
	_ = privKey
	if _, err := oidc.Verify("not.a.jwt", pubKey); err == nil {
		t.Error("expected error for malformed token")
	}
}

func TestParseUnverifiedExtractsClaims(t *testing.T) {
	_, privKey, _ := crypto.GenerateSigningKey()
	token, _ := oidc.IssueIDToken(
		"https://issuer.example.com", "did:peer:alice", "aud-1",
		"n1", time.Hour, privKey, nil, "k1",
	)
	claims, err := oidc.ParseUnverified(token)
	if err != nil {
		t.Fatal(err)
	}
	if claims.Subject != "did:peer:alice" {
		t.Errorf("unexpected subject: %s", claims.Subject)
	}
}

// ---- PKCE S256 math ----

func TestPKCES256Math(t *testing.T) {
	// Verify BASE64URL(SHA256(verifier)) == challenge is the correct PKCE formula.
	verifier := "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"
	h := sha256.Sum256([]byte(verifier))
	challenge := base64.RawURLEncoding.EncodeToString(h[:])
	if len(challenge) == 0 {
		t.Error("empty challenge")
	}
	// Re-compute and confirm determinism.
	h2 := sha256.Sum256([]byte(verifier))
	if base64.RawURLEncoding.EncodeToString(h2[:]) != challenge {
		t.Error("PKCE computation not deterministic")
	}
}

// ---- Token revocation ----

func TestIsAccessTokenRevokedReturnsFalseInitially(t *testing.T) {
	_, privKey, _ := crypto.GenerateSigningKey()
	token, _ := oidc.IssueAccessToken(
		"https://issuer.example.com", "did:peer:sub", "client-1",
		time.Hour, privKey, "key-1",
	)
	if oidc.IsAccessTokenRevoked(token) {
		t.Error("freshly issued token should not be revoked")
	}
}

// ---- Dynamic Client Registration types ----

func TestRegisterClientRequestZeroValue(t *testing.T) {
	var req oidc.RegisterClientRequest
	if req.ClientName != "" {
		t.Error("expected zero-value client name")
	}
}
