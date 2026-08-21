# tpt-identity — Deferred Risks

Items here are real risks that are deliberately deferred because they depend on context that doesn't exist yet (downstream consumers, deployment model, frontend), or because they are pre-release polish. Each has a clear trigger for when to address it.

---

## 1. OIDC back-channel logout

**Risk:** When a user's session at tpt-identity ends, downstream services (tpt-healthcare, tpt-email) retain valid access tokens until expiry. For a multi-service ecosystem this means session termination is not atomic.

**Why deferred:** No downstream consumers exist yet. The back-channel logout spec (OIDC Core / Back-Channel Logout 1.0) requires registered logout URIs per client — that wiring can't be done until there are clients to wire.

**Trigger:** Implement when the first downstream TPT module goes live and registers as an OIDC client. Add `oidc/logout.go` — `POST /oidc/backchannel_logout`; fan out logout tokens to all registered client logout URIs on session end.

---

## 2. DNS reputation is a weak trust anchor

**Risk:** `pkg/trust/reputation.go` (ported from tpt-email) uses DNS TXT records for federated reputation signals. DNS is controlled by registrars, subject to BGP/DNS hijacking, and doesn't align with DID resolution trust models. It should not be used as a security assertion.

**Why deferred:** The module is a direct port and not yet wired into anything critical. Redesigning it requires a clearer trust model for the broader TPT ecosystem.

**Trigger:** Before any production deployment relies on reputation scores for access control decisions. Long-term replacement: reputation expressed as a signed VC from a known issuer, not a DNS lookup.

---

## 3. Keystore backup and recovery

**Risk:** If the keystore file is lost or corrupted, the identity is unrecoverable. For an operator running tpt-identity as the backbone of the TPT ecosystem, this is catastrophic.

**Why deferred:** The right backup mechanism depends on the deployment model (bare metal, Docker, Kubernetes with secrets management, HSM). Providing a generic backup mechanism before knowing the deployment target risks providing a false sense of security.

**Trigger:** Before first production deployment. Add `cmd/tpt-identity/cmd/backup.go` and `restore.go` — encrypted export/import with a separate recovery passphrase. Document that this is an operator responsibility and what the recovery procedure is.

---

## 4. CORS configuration

**Risk:** If any browser-based frontend calls the tpt-identity API directly, CORS headers need to be explicitly configured. Leaving CORS to framework defaults will either block legitimate requests or be too permissive.

**Why deferred:** No browser frontend is planned for MVP. The API is consumed by server-to-server clients.

**Trigger:** When the first browser-facing frontend is built. Add explicit CORS middleware in `api/server.go` with an allowlist of permitted origins; never use wildcard `*` for an authenticated API.

---

## 5. SECURITY.md

**Risk:** Open-source security-critical projects are expected to have a responsible disclosure policy. Without one, security researchers don't know how to report vulnerabilities.

**Why deferred:** Pre-public-release. No external contributors yet.

**Trigger:** Before the repository is made public or announced. Minimum content: contact method, response SLA, disclosure timeline, PGP key if available.

---

## 6. did:key key rotation (permanent limitation)

**Risk:** did:key encodes the public key into the DID. Key rotation is structurally impossible — a compromised did:key DID is compromised permanently.

**Why deferred:** This is a spec constraint, not a fixable bug. The TODO already enforces the usage constraint (did:key for ephemeral/offline use only; persistent credentials must use did:web). Documenting it here for operator awareness.

**Trigger:** Document explicitly in `PROTOCOL.md` with guidance on what "ephemeral/offline" means in practice and what to do if a did:key private key is suspected compromised (answer: the DID must be considered abandoned; issue new credentials under a did:web DID).
