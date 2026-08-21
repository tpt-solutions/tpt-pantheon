package api

import (
	"net/http"

	"github.com/prometheus/client_golang/prometheus"
	"github.com/prometheus/client_golang/prometheus/promhttp"
)

var (
	httpRequestsTotal = prometheus.NewCounterVec(prometheus.CounterOpts{
		Name: "tpt_http_requests_total",
		Help: "Total HTTP requests by method, path, and status code.",
	}, []string{"method", "path", "status"})

	httpRequestDuration = prometheus.NewHistogramVec(prometheus.HistogramOpts{
		Name:    "tpt_http_request_duration_seconds",
		Help:    "HTTP request latency by method and path.",
		Buckets: prometheus.DefBuckets,
	}, []string{"method", "path"})

	CredentialsIssuedTotal = prometheus.NewCounterVec(prometheus.CounterOpts{
		Name: "tpt_credentials_issued_total",
		Help: "Total credentials issued by format (vc, sdjwt).",
	}, []string{"format"})

	WebhookDeliveriesTotal = prometheus.NewCounterVec(prometheus.CounterOpts{
		Name: "tpt_webhook_deliveries_total",
		Help: "Total webhook delivery attempts by result (success, failure).",
	}, []string{"result"})
)

func init() {
	prometheus.MustRegister(httpRequestsTotal, httpRequestDuration, CredentialsIssuedTotal, WebhookDeliveriesTotal)
}

// MetricsHandler returns the Prometheus /metrics handler.
func MetricsHandler() http.Handler {
	return promhttp.Handler()
}
