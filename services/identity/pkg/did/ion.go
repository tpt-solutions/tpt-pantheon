//go:build ion

// Package did provides the did:ion implementation.
// Build with: go build -tags ion ./...
// Requires a running ION node or access to a public ION resolution endpoint.
package did

import (
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"strings"
	"time"
)

func init() {
	RegisterMethod(&ionMethod{
		endpoint: "https://beta.discover.did.microsoft.com/1.0/identifiers/",
		client:   &http.Client{Timeout: 15 * time.Second},
	})
}

type ionMethod struct {
	endpoint string
	client   *http.Client
}

func (i *ionMethod) Prefix() string { return "ion" }

// Create is not yet implemented for did:ion — creating an ION DID requires anchoring
// a Sidetree operation to the Bitcoin network, which is an asynchronous process.
// Use the ION CLI or SDK to create ION DIDs and then resolve them with tpt-identity.
func (i *ionMethod) Create(opts CreateOptions) (string, *Document, error) {
	return "", nil, fmt.Errorf("did:ion: Create not implemented — use the ION CLI to anchor a new DID, then resolve it here")
}

// Resolve fetches the DID Document from a configured ION resolution endpoint.
func (i *ionMethod) Resolve(did string) (*Document, error) {
	if !strings.HasPrefix(did, "did:ion:") {
		return nil, fmt.Errorf("did:ion: invalid DID %q", did)
	}
	url := i.endpoint + did
	resp, err := i.client.Get(url)
	if err != nil {
		return nil, fmt.Errorf("did:ion resolve %s: %w", did, err)
	}
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusOK {
		return nil, fmt.Errorf("did:ion resolve %s: HTTP %d", did, resp.StatusCode)
	}
	body, err := io.ReadAll(io.LimitReader(resp.Body, 1<<20))
	if err != nil {
		return nil, fmt.Errorf("did:ion read body: %w", err)
	}

	// ION resolution response wraps the DID Document in a resolution result envelope.
	var result struct {
		DidDocument *Document `json:"didDocument"`
	}
	if err := json.Unmarshal(body, &result); err != nil {
		return nil, fmt.Errorf("did:ion parse: %w", err)
	}
	if result.DidDocument == nil {
		return nil, fmt.Errorf("did:ion: no didDocument in response for %s", did)
	}
	return result.DidDocument, nil
}
