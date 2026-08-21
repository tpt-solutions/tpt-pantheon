package did

import (
	"crypto/ed25519"
	"encoding/json"
	"errors"
	"fmt"
	"strings"

	"github.com/multiformats/go-multibase"
)

func init() {
	RegisterMethod(&keyMethod{})
}

type keyMethod struct{}

func (k *keyMethod) Prefix() string { return "key" }

// Create derives a did:key DID directly from the Ed25519 public key.
// The DID is self-certifying: it encodes the public key, so no network resolution is needed.
func (k *keyMethod) Create(opts CreateOptions) (string, *Document, error) {
	if len(opts.SigningPub) != ed25519.PublicKeySize {
		return "", nil, errors.New("did:key requires a 32-byte Ed25519 public key")
	}
	did, err := encodeKeyDID(opts.SigningPub)
	if err != nil {
		return "", nil, err
	}
	doc := keyDocument(did, opts.SigningPub, opts.EncPub)
	return did, doc, nil
}

// Resolve decodes the public key directly from the DID string — no network call.
func (k *keyMethod) Resolve(did string) (*Document, error) {
	if !strings.HasPrefix(did, "did:key:") {
		return nil, fmt.Errorf("did:key: invalid DID %q", did)
	}
	mb := strings.TrimPrefix(did, "did:key:")
	_, raw, err := multibase.Decode(mb)
	if err != nil {
		return nil, fmt.Errorf("did:key decode: %w", err)
	}
	// raw starts with a 2-byte multicodec varint prefix (0xed 0x01 for ed25519-pub)
	if len(raw) < 3 {
		return nil, errors.New("did:key: too short")
	}
	codec, n := decodeUvarint(raw)
	if codec != 0xed {
		return nil, fmt.Errorf("did:key: unsupported multicodec 0x%x", codec)
	}
	pubKey := raw[n:]
	if len(pubKey) != ed25519.PublicKeySize {
		return nil, errors.New("did:key: invalid ed25519 key length")
	}
	return keyDocument(did, pubKey, nil), nil
}

func encodeKeyDID(pub []byte) (string, error) {
	// Multicodec prefix for ed25519-pub is 0xed (varint: 0xed 0x01)
	prefix := []byte{0xed, 0x01}
	raw := append(prefix, pub...)
	mb, err := multibase.Encode(multibase.Base58BTC, raw)
	if err != nil {
		return "", fmt.Errorf("did:key encode: %w", err)
	}
	return "did:key:" + mb, nil
}

func keyDocument(did string, sigPub, encPub []byte) *Document {
	sigKeyID := did + "#" + strings.TrimPrefix(did, "did:key:")
	sigPubMB, _ := toMultibase(sigPub, 0xed)

	doc := &Document{
		Context: DefaultContext(),
		ID:      did,
		VerificationMethod: []VerificationMethod{{
			ID:                 sigKeyID,
			Type:               "Ed25519VerificationKey2020",
			Controller:         did,
			PublicKeyMultibase: &sigPubMB,
		}},
		Authentication:  []string{sigKeyID},
		AssertionMethod: []string{sigKeyID},
	}

	if len(encPub) == 32 {
		encKeyID := did + "#enc"
		encPubMB, _ := toMultibase(encPub, 0xec)
		doc.VerificationMethod = append(doc.VerificationMethod, VerificationMethod{
			ID:                 encKeyID,
			Type:               "X25519KeyAgreementKey2020",
			Controller:         did,
			PublicKeyMultibase: &encPubMB,
		})
		doc.KeyAgreement = []string{encKeyID}
	}
	return doc
}

// MarshalDocument marshals a DID Document to JSON (helper for serving /.well-known/did.json).
func MarshalDocument(doc *Document) ([]byte, error) {
	return json.MarshalIndent(doc, "", "  ")
}

func decodeUvarint(b []byte) (uint64, int) {
	var x uint64
	var s uint
	for i, c := range b {
		if c < 0x80 {
			return x | uint64(c)<<s, i + 1
		}
		x |= uint64(c&0x7f) << s
		s += 7
	}
	return 0, 0
}
