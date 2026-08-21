package didcomm

import (
	"bytes"
	"encoding/json"
	"fmt"
	"testing"
)

func TestAnoncryptRoundTrip(t *testing.T) {
	recipPriv, recipPub, err := generateX25519()
	if err != nil {
		t.Fatal(err)
	}

	msg := &Message{
		ID:   "msg-anon-1",
		Type: "https://didcomm.org/basicmessage/2.0/message",
		Body: map[string]any{"content": "hello anoncrypt"},
	}
	plaintext, _ := json.Marshal(msg)

	packed, err := PackAnoncrypt(plaintext, []Recipient{
		{KID: "did:example:bob#enc-1", PubKey: recipPub},
	})
	if err != nil {
		t.Fatalf("PackAnoncrypt: %v", err)
	}

	// Verify it is valid JSON with required JWE fields.
	var env Envelope
	if err := json.Unmarshal(packed, &env); err != nil {
		t.Fatalf("packed envelope not valid JSON: %v", err)
	}
	if env.Protected == "" || env.IV == "" || env.Ciphertext == "" || env.Tag == "" {
		t.Fatal("packed envelope missing fields")
	}
	if len(env.Recipients) != 1 {
		t.Fatalf("expected 1 recipient, got %d", len(env.Recipients))
	}

	decrypted, senderKID, err := Unpack(packed, func(kid string) ([32]byte, error) {
		if kid == "did:example:bob#enc-1" {
			return recipPriv, nil
		}
		return [32]byte{}, fmt.Errorf("unknown kid: %s", kid)
	}, nil)
	if err != nil {
		t.Fatalf("Unpack: %v", err)
	}
	if senderKID != "" {
		t.Errorf("anoncrypt: expected empty senderKID, got %q", senderKID)
	}
	if !bytes.Equal(decrypted, plaintext) {
		t.Errorf("decrypted content mismatch:\ngot  %s\nwant %s", decrypted, plaintext)
	}
}

func TestAuthcryptRoundTrip(t *testing.T) {
	recipPriv, recipPub, err := generateX25519()
	if err != nil {
		t.Fatal(err)
	}
	senderPriv, senderPub, err := generateX25519()
	if err != nil {
		t.Fatal(err)
	}

	msg := &Message{
		ID:   "msg-auth-1",
		Type: "https://didcomm.org/basicmessage/2.0/message",
		From: "did:example:alice",
		To:   []string{"did:example:bob"},
		Body: map[string]any{"content": "hello authcrypt"},
	}
	plaintext, _ := json.Marshal(msg)

	const senderKID = "did:example:alice#enc-1"
	packed, err := PackAuthcrypt(plaintext, []Recipient{
		{KID: "did:example:bob#enc-1", PubKey: recipPub},
	}, senderKID, senderPriv)
	if err != nil {
		t.Fatalf("PackAuthcrypt: %v", err)
	}

	decrypted, gotSenderKID, err := Unpack(packed,
		func(kid string) ([32]byte, error) {
			if kid == "did:example:bob#enc-1" {
				return recipPriv, nil
			}
			return [32]byte{}, fmt.Errorf("unknown kid: %s", kid)
		},
		func(skid string) ([32]byte, error) {
			if skid == senderKID {
				return senderPub, nil
			}
			return [32]byte{}, fmt.Errorf("unknown sender kid: %s", skid)
		},
	)
	if err != nil {
		t.Fatalf("Unpack: %v", err)
	}
	if gotSenderKID != senderKID {
		t.Errorf("senderKID: got %q, want %q", gotSenderKID, senderKID)
	}
	if !bytes.Equal(decrypted, plaintext) {
		t.Errorf("decrypted content mismatch:\ngot  %s\nwant %s", decrypted, plaintext)
	}
}

func TestMultipleRecipients(t *testing.T) {
	const n = 3
	privs := make([][32]byte, n)
	pubs := make([][32]byte, n)
	kids := make([]string, n)
	for i := range n {
		var err error
		privs[i], pubs[i], err = generateX25519()
		if err != nil {
			t.Fatal(err)
		}
		kids[i] = fmt.Sprintf("did:example:user%d#enc-1", i)
	}

	recipients := make([]Recipient, n)
	for i := range n {
		recipients[i] = Recipient{KID: kids[i], PubKey: pubs[i]}
	}

	plaintext := []byte(`{"id":"multi","type":"test"}`)
	packed, err := PackAnoncrypt(plaintext, recipients)
	if err != nil {
		t.Fatalf("PackAnoncrypt: %v", err)
	}

	// Each recipient should be able to independently decrypt.
	for i := range n {
		idx := i
		decrypted, _, err := Unpack(packed, func(kid string) ([32]byte, error) {
			if kid == kids[idx] {
				return privs[idx], nil
			}
			return [32]byte{}, fmt.Errorf("not my key")
		}, nil)
		if err != nil {
			t.Errorf("recipient %d Unpack: %v", idx, err)
			continue
		}
		if !bytes.Equal(decrypted, plaintext) {
			t.Errorf("recipient %d: content mismatch", idx)
		}
	}
}

func TestWrongKeyFails(t *testing.T) {
	_, recipPub, _ := generateX25519()
	wrongPriv, _, _ := generateX25519()

	plaintext := []byte(`{"id":"x","type":"test"}`)
	packed, err := PackAnoncrypt(plaintext, []Recipient{
		{KID: "did:example:bob#enc-1", PubKey: recipPub},
	})
	if err != nil {
		t.Fatal(err)
	}

	_, _, err = Unpack(packed, func(kid string) ([32]byte, error) {
		return wrongPriv, nil // wrong private key
	}, nil)
	if err == nil {
		t.Fatal("expected error with wrong key, got nil")
	}
}

func TestAESKeyWrapRoundTrip(t *testing.T) {
	var kek [32]byte
	var cek [32]byte
	for i := range kek {
		kek[i] = byte(i)
		cek[i] = byte(i + 32)
	}
	wrapped, err := aesKeyWrap(kek[:], cek[:])
	if err != nil {
		t.Fatalf("wrap: %v", err)
	}
	if len(wrapped) != 40 { // 32-byte key + 8-byte IV
		t.Errorf("wrapped length: got %d, want 40", len(wrapped))
	}
	unwrapped, err := aesKeyUnwrap(kek[:], wrapped)
	if err != nil {
		t.Fatalf("unwrap: %v", err)
	}
	if !bytes.Equal(unwrapped, cek[:]) {
		t.Error("unwrapped key mismatch")
	}
}

func TestAESKeyUnwrapTamperDetection(t *testing.T) {
	var kek [32]byte
	var cek [32]byte
	wrapped, _ := aesKeyWrap(kek[:], cek[:])

	// Flip a bit in the ciphertext.
	wrapped[10] ^= 0xFF

	_, err := aesKeyUnwrap(kek[:], wrapped)
	if err == nil {
		t.Fatal("expected integrity check failure, got nil")
	}
}
