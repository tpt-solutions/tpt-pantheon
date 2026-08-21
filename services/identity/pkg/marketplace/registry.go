// Package marketplace provides a discoverable registry of credential issuers
// and the schemas they offer, with a running count of issued credentials per pair.
package marketplace

import (
	"sync"
)

// Entry describes a single issuer+schema combination in the marketplace.
type Entry struct {
	IssuerDID   string `json:"issuer_did"`
	SchemaID    string `json:"schema_id"`
	SchemaName  string `json:"schema_name,omitempty"`
	IssuedCount int64  `json:"issued_count"`
}

// Registry is a thread-safe in-memory credential marketplace.
type Registry struct {
	mu      sync.RWMutex
	entries map[string]*Entry // key: issuerDID + ":" + schemaID
}

// New creates an empty Registry.
func New() *Registry {
	return &Registry{entries: make(map[string]*Entry)}
}

// Register adds or updates an issuer+schema entry. Safe to call multiple times.
func (r *Registry) Register(issuerDID, schemaID, schemaName string) {
	key := issuerDID + ":" + schemaID
	r.mu.Lock()
	defer r.mu.Unlock()
	if _, ok := r.entries[key]; !ok {
		r.entries[key] = &Entry{
			IssuerDID:  issuerDID,
			SchemaID:   schemaID,
			SchemaName: schemaName,
		}
	}
}

// RecordIssuance increments the issued_count for the given issuer+schema pair.
// If the pair is not registered, it is added automatically.
func (r *Registry) RecordIssuance(issuerDID, schemaID string) {
	key := issuerDID + ":" + schemaID
	r.mu.Lock()
	defer r.mu.Unlock()
	e, ok := r.entries[key]
	if !ok {
		e = &Entry{IssuerDID: issuerDID, SchemaID: schemaID}
		r.entries[key] = e
	}
	e.IssuedCount++
}

// List returns a snapshot of all registry entries.
func (r *Registry) List() []*Entry {
	r.mu.RLock()
	defer r.mu.RUnlock()
	out := make([]*Entry, 0, len(r.entries))
	for _, e := range r.entries {
		cp := *e
		out = append(out, &cp)
	}
	return out
}
