package api

import (
	"context"
	"crypto/rand"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"net"
	"net/http"
	"net/url"
	"time"

	"github.com/PhillipC05/tpt-identity/internal/store"
)

type registerWebhookRequest struct {
	URL        string   `json:"url"`
	EventTypes []string `json:"event_types"`
}

type registerWebhookResponse struct {
	ID           string   `json:"id"`
	URL          string   `json:"url"`
	EventTypes   []string `json:"event_types"`
	SigningSecret string  `json:"signing_secret"` // shown once — store it safely
	CreatedAt    string   `json:"created_at"`
}

// handleRegisterWebhook registers a new webhook subscription.
// POST /api/v1/webhooks
func (s *Server) handleRegisterWebhook(w http.ResponseWriter, r *http.Request) {
	var req registerWebhookRequest
	if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
		http.Error(w, "invalid request", http.StatusBadRequest)
		return
	}
	if req.URL == "" {
		http.Error(w, "url required", http.StatusBadRequest)
		return
	}
	if err := validateWebhookURL(r.Context(), req.URL); err != nil {
		http.Error(w, "invalid url: "+err.Error(), http.StatusBadRequest)
		return
	}
	if len(req.EventTypes) == 0 {
		req.EventTypes = []string{"*"}
	}

	// Generate a signing secret — returned once, never stored in plaintext.
	rawSecret, err := randomWebhookSecret()
	if err != nil {
		http.Error(w, "internal error", http.StatusInternalServerError)
		return
	}
	secretHash := sha256WebhookSecret(rawSecret)

	id, err := randomWebhookID()
	if err != nil {
		http.Error(w, "internal error", http.StatusInternalServerError)
		return
	}
	sub := &store.WebhookSubscription{
		ID:         id,
		URL:        req.URL,
		EventTypes: req.EventTypes,
		SecretHash: secretHash,
		CreatedAt:  time.Now(),
	}
	if err := s.store.SaveWebhookSubscription(r.Context(), sub); err != nil {
		http.Error(w, "internal error", http.StatusInternalServerError)
		return
	}

	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(http.StatusCreated)
	json.NewEncoder(w).Encode(registerWebhookResponse{
		ID:           sub.ID,
		URL:          sub.URL,
		EventTypes:   sub.EventTypes,
		SigningSecret: rawSecret,
		CreatedAt:    sub.CreatedAt.UTC().Format(time.RFC3339),
	})
}

// handleDeleteWebhook removes a webhook subscription.
// DELETE /api/v1/webhooks/{id}
func (s *Server) handleDeleteWebhook(w http.ResponseWriter, r *http.Request) {
	id := r.PathValue("id")
	if err := s.store.DeleteWebhookSubscription(r.Context(), id); err != nil {
		http.Error(w, "not found", http.StatusNotFound)
		return
	}
	w.WriteHeader(http.StatusNoContent)
}

// handleListWebhooks lists all registered webhook subscriptions.
// GET /api/v1/webhooks
func (s *Server) handleListWebhooks(w http.ResponseWriter, r *http.Request) {
	// Pass empty string to get all subscriptions regardless of event type.
	subs, err := s.store.ListWebhookSubscriptions(r.Context(), "")
	if err != nil {
		http.Error(w, "internal error", http.StatusInternalServerError)
		return
	}
	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(subs)
}

// handleTestWebhook handles POST /api/v1/webhooks/{id}/test.
//
// Sends a synthetic "ping" event to the webhook endpoint so operators can
// verify delivery before relying on it in production. Returns the HTTP status
// code returned by the target.
func (s *Server) handleTestWebhook(w http.ResponseWriter, r *http.Request) {
	id := r.PathValue("id")
	sub, err := s.store.GetWebhookSubscription(r.Context(), id)
	if err != nil {
		writeJSONError(w, http.StatusNotFound, "not_found", "webhook subscription not found")
		return
	}

	s.events.Publish(r.Context(), "webhook.ping", map[string]any{
		"webhook_id": id,
		"message":    "Test delivery from tpt-identity. If you receive this, your webhook is configured correctly.",
		"sent_at":    time.Now().UTC(),
	})

	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(map[string]any{
		"webhook_id": sub.ID,
		"url":        sub.URL,
		"status":     "ping_sent",
		"message":    "Test event published. Check your webhook endpoint for delivery.",
	})
}

func randomWebhookSecret() (string, error) {
	b := make([]byte, 32)
	if _, err := rand.Read(b); err != nil {
		return "", err
	}
	return "whsec_" + hex.EncodeToString(b), nil
}

func sha256WebhookSecret(s string) string {
	h := sha256.Sum256([]byte(s))
	return hex.EncodeToString(h[:])
}

func randomWebhookID() (string, error) {
	b := make([]byte, 8)
	_, err := rand.Read(b)
	return "wh_" + hex.EncodeToString(b), err
}

// validateWebhookURL rejects loopback / RFC-1918 / link-local targets (SSRF hardening).
func validateWebhookURL(ctx context.Context, rawURL string) error {
	u, err := url.ParseRequestURI(rawURL)
	if err != nil {
		return fmt.Errorf("malformed URL")
	}
	if u.Scheme != "https" && u.Scheme != "http" {
		return fmt.Errorf("scheme must be http or https")
	}
	host := u.Hostname()
	ips, err := net.DefaultResolver.LookupHost(ctx, host)
	if err != nil {
		return fmt.Errorf("cannot resolve host: %w", err)
	}
	for _, ipStr := range ips {
		ip := net.ParseIP(ipStr)
		if ip == nil {
			continue
		}
		if webhookIsPrivateIP(ip) {
			return fmt.Errorf("host resolves to a private/loopback address")
		}
	}
	return nil
}

var webhookPrivateRanges = func() []*net.IPNet {
	var blocks []*net.IPNet
	for _, cidr := range []string{
		"10.0.0.0/8", "172.16.0.0/12", "192.168.0.0/16",
		"127.0.0.0/8", "::1/128", "fc00::/7",
		"169.254.0.0/16", "fe80::/10",
		"100.64.0.0/10", "198.18.0.0/15", "0.0.0.0/8",
	} {
		_, block, _ := net.ParseCIDR(cidr)
		if block != nil {
			blocks = append(blocks, block)
		}
	}
	return blocks
}()

func webhookIsPrivateIP(ip net.IP) bool {
	if ip.IsLoopback() || ip.IsLinkLocalUnicast() || ip.IsLinkLocalMulticast() {
		return true
	}
	for _, block := range webhookPrivateRanges {
		if block.Contains(ip) {
			return true
		}
	}
	return false
}
