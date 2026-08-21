package crypto

import (
	"crypto/ed25519"
	"crypto/rand"
	"crypto/sha512"
	"encoding/json"
	"errors"
	"fmt"
)

// GenerateSigningKey generates a new Ed25519 keypair.
func GenerateSigningKey() (ed25519.PublicKey, ed25519.PrivateKey, error) {
	return ed25519.GenerateKey(rand.Reader)
}

// Sign signs the canonical SHA-512 hash of payload with key.
func Sign(key ed25519.PrivateKey, payload []byte) ([]byte, error) {
	h := sha512.Sum512(payload)
	return ed25519.Sign(key, h[:]), nil
}

// Verify verifies a signature produced by Sign.
func Verify(pub ed25519.PublicKey, payload, sig []byte) error {
	h := sha512.Sum512(payload)
	if !ed25519.Verify(pub, h[:], sig) {
		return errors.New("invalid signature")
	}
	return nil
}

// SignJSON canonicalises v as JSON then signs it.
func SignJSON(key ed25519.PrivateKey, v any) ([]byte, error) {
	b, err := json.Marshal(v)
	if err != nil {
		return nil, fmt.Errorf("marshal: %w", err)
	}
	return Sign(key, b)
}

// VerifyJSON canonicalises v as JSON then verifies the signature.
func VerifyJSON(pub ed25519.PublicKey, v any, sig []byte) error {
	b, err := json.Marshal(v)
	if err != nil {
		return fmt.Errorf("marshal: %w", err)
	}
	return Verify(pub, b, sig)
}
