package crypto

import (
	"crypto/sha256"
	"crypto/sha512"
	"encoding/base64"
	"encoding/json"
	"fmt"
)

// JCS serialises v to RFC 8785 JSON Canonicalization Scheme output.
// It works by round-tripping through json.Unmarshal (which produces
// map[string]any for objects) and re-marshaling, since encoding/json
// sorts map keys alphabetically — matching RFC 8785 for the ASCII key
// sets used in tpt-identity credentials.
func JCS(v any) ([]byte, error) {
	raw, err := json.Marshal(v)
	if err != nil {
		return nil, fmt.Errorf("jcs marshal: %w", err)
	}
	var canonical any
	if err := json.Unmarshal(raw, &canonical); err != nil {
		return nil, fmt.Errorf("jcs unmarshal: %w", err)
	}
	out, err := json.Marshal(canonical)
	if err != nil {
		return nil, fmt.Errorf("jcs remarshal: %w", err)
	}
	return out, nil
}

// SHA256Hex returns the hex-encoded SHA-256 digest of data.
func SHA256Hex(data []byte) string {
	h := sha256.Sum256(data)
	return fmt.Sprintf("%x", h)
}

// SHA512B64 returns the base64url-encoded SHA-512 digest of data.
func SHA512B64(data []byte) string {
	h := sha512.Sum512(data)
	return base64.RawURLEncoding.EncodeToString(h[:])
}

// CanonicalJSON marshals v to compact, deterministic JSON.
func CanonicalJSON(v any) ([]byte, error) {
	b, err := json.Marshal(v)
	if err != nil {
		return nil, fmt.Errorf("canonical json: %w", err)
	}
	return b, nil
}

// HashJSON returns the SHA-512 base64url digest of v's canonical JSON.
func HashJSON(v any) (string, error) {
	b, err := CanonicalJSON(v)
	if err != nil {
		return "", err
	}
	return SHA512B64(b), nil
}
