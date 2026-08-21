# tpt-identity Protocol Reference

Technical specification for the DID methods, VC profiles, OIDC flows, and consent model used by tpt-identity.

---

## DID Methods

### did:web

**Purpose:** Organisational / domain-scoped identities. Use for issuers (hospitals, government agencies, businesses).

**Resolution:** HTTPS fetch of `/.well-known/did.json` (or `/{path}/did.json` for sub-path DIDs).

**SSRF hardening:** The resolver blocks RFC-1918 ranges, loopback, link-local, and any address not in the configured `AllowedDomains` list. Maximum one redirect. 5-second timeout.

**Key format:** Ed25519VerificationKey2020 (multibase-encoded, base58btc).

**Example:**
```
did:web:identity.example.com
→ GET https://identity.example.com/.well-known/did.json
```

### did:key

**Purpose:** Ephemeral or offline use only. Derive a self-certifying DID directly from an Ed25519 public key.

**Constraint:** `did:key` **must not** be used as a VC issuer DID. Because the DID encodes the public key directly, key rotation is structurally impossible. Any credential issued under a `did:key` cannot be re-issued after key compromise. Use `did:web` or `did:peer` for issuers.

**Resolution:** Offline — the public key is decoded from the DID string itself.

### did:peer

**Purpose:** Pairwise relationships. Two parties create private DIDs for their bilateral connection.

**Resolution:** Local only. Did documents are exchanged out-of-band (e.g. during connection establishment) and stored in the local resolver cache.

---

## Verifiable Credentials

### Data Model

Follows [W3C VC Data Model 2.0](https://www.w3.org/TR/vc-data-model-2.0/). Key fields:

```json
{
  "@context": ["https://www.w3.org/ns/credentials/v2", "https://tpt-identity.org/ns/v1"],
  "id": "urn:uuid:<uuid>",
  "type": ["VerifiableCredential", "identity.legal-name-v1"],
  "issuer": "did:web:identity.example.com",
  "validFrom": "2026-01-01T00:00:00Z",
  "validUntil": "2027-01-01T00:00:00Z",
  "credentialSubject": {
    "id": "did:peer:<pairwise>",
    "claims": { "givenNames": "Alice", "familyName": "Smith" }
  },
  "credentialSchema": {
    "id": "identity.legal-name-v1",
    "type": "TptCredentialSchema"
  },
  "credentialStatus": {
    "id": "https://identity.example.com/api/v1/status/abc123#42",
    "type": "BitstringStatusListEntry",
    "statusPurpose": "revocation",
    "statusListIndex": 42,
    "statusListCredential": "https://identity.example.com/api/v1/status/abc123"
  },
  "proof": {
    "type": "DataIntegrityProof",
    "cryptosuite": "eddsa-jcs-2022",
    "created": "2026-01-01T00:00:00Z",
    "verificationMethod": "did:web:identity.example.com#signing-key-1",
    "proofPurpose": "assertionMethod",
    "proofValue": "<base64url-encoded signature>"
  }
}
```

### DataIntegrityProof (eddsa-jcs-2022)

Signing algorithm:
1. Serialize the **proof options document** (type, cryptosuite, created, verificationMethod, proofPurpose) using JCS (RFC 8785).
2. Serialize the **credential document** (without proof) using JCS.
3. `hashData = SHA-256(proofOptionsBytes) ‖ SHA-256(documentBytes)`
4. `signature = Ed25519.Sign(privateKey, hashData)`
5. `proofValue = BASE64URL(signature)`

Verification: reverse steps 1–3, then `Ed25519.Verify(publicKey, hashData, signature)`.

### Schema IDs and versioning

Schemas are registered with a `Version` integer (default: 1). Issued credentials reference the **versioned** schema ID: `{category}.{name}-v{version}`, e.g. `identity.legal-name-v1`.

The validator accepts both base (`identity.legal-name`) and versioned (`identity.legal-name-v1`) forms when looking up schemas.

### Credential Revocation — BitstringStatusList

- A **StatusList** is a gzip-compressed bitstring (minimum 131 072 entries per spec).
- Each credential carries a `credentialStatus` pointer with `statusListIndex` and `statusListCredential` URL.
- Revocation: the issuer flips the bit at the index and re-issues a `BitstringStatusListCredential` VC.
- Verifiers fetch the status list VC from `GET /api/v1/status/{listId}` and call `CheckStatus(bits, status)`.

---

## OIDC Provider

Implements [OpenID Connect Core 1.0](https://openid.net/specs/openid-connect-core-1_0.html) authorization code flow.

### Authorization Code Flow with PKCE

PKCE is **mandatory** — all flows must use `code_challenge_method=S256`.

```
Client                        tpt-identity
  │                                │
  │  GET /authorize                │
  │  ?client_id=...                │
  │  &redirect_uri=...             │
  │  &response_type=code           │
  │  &state=<random>               │
  │  &code_challenge=<S256>        │
  │  &code_challenge_method=S256   │
  │  X-Subject-DID: <did>          │
  │ ───────────────────────────────►
  │                                │  validate client, redirect_uri
  │                                │  store session with code_challenge
  │  302 ?code=...&state=...       │
  │ ◄───────────────────────────── │
  │                                │
  │  POST /token                   │
  │  code=...                      │
  │  code_verifier=...             │
  │ ───────────────────────────────►
  │                                │  verify PKCE: SHA256(verifier)==challenge
  │                                │  issue id_token + access_token + refresh_token
  │  200 { id_token, access_token }│
  │ ◄───────────────────────────── │
```

**state** is required on every authorization request (prevents CSRF).

### Dynamic Client Registration (RFC 7591)

`POST /oidc/register` — downstream TPT modules self-register at startup. Returns `client_id` and `client_secret` (confidential clients).

### Token Revocation (RFC 7009)

`POST /oidc/revoke` — accepts `token` and optional `token_type_hint`. Refresh tokens are deleted from the store; access tokens are blacklisted in memory until their natural expiry. Always returns 200.

---

## Consent Model

Consent is enforced at the **schema** or **category** level.

### Grant levels

| Level | Description | ExplicitlyConfirmed |
|-------|-------------|---------------------|
| `schema` | Access to one specific schema (e.g. `identity.legal-name`) | Required for extra-sensitive schemas |
| `category` | Access to all non-extra-sensitive schemas in a category | Always required |

### Extra-sensitive schemas

The following schemas require an **individual explicit grant** even when a category-wide grant exists:
- `healthcare.mental-health`, `healthcare.sexual-health`, `healthcare.reproductive-health`, `healthcare.addiction`
- `legal.criminal-record`

### Grant lifecycle

```
AddGrant()  →  [active]  →  RevokeGrant()  →  [revoked: RevokedAt set]
                          →  ExpiresAt reached  →  [expired: ignored in CanAccess]
```

Grants are **never deleted**. `RevokedAt` is set on withdrawal to preserve the audit trail as required by the NZ Privacy Act 2020.

### Consent receipts

A `Receipt` is a cryptographically signed record (Ed25519) created when a relying party accesses data. Fields:
- `subjectDid`, `relyingDid`, `schemaId`, `accessedAt`, `legalBasis`, `purpose`
- `expiresAt` (optional) — when the authorisation underlying this access expires
- `revokedAt` (optional) — set when the subject withdraws consent after the fact

Receipts are immutable once issued. `Withdraw()` sets `revokedAt` but does not alter the original signature.

---

## Inter-Service Trust

### Permits

A **permit** is a short-lived Ed25519-signed JWT that grants one TPT service permission to perform a specific action on behalf of a subject DID.

```json
{
  "iss": "did:web:email.tpt.nz",
  "sub": "did:peer:alice",
  "aud": "did:web:healthcare.tpt.nz",
  "action": "read:email",
  "iat": 1748908800,
  "exp": 1748909100,
  "jti": "unique-id"
}
```

### Reputation (DNS TXT)

TPT services publish reputation records at `_tpt-rep.<domain>`:

```
_tpt-rep.example.com. 300 IN TXT "v=tpt1 score=85 tier=verified since=2024-01-01"
```

| Field | Description |
|-------|-------------|
| `v=tpt1` | Record version (required) |
| `score` | 0–100 trust score |
| `tier` | Human-readable label |
| `since` | Date reputation established |

Trust levels: `unknown` (no record) → `restricted` (0–24) → `provisional` (25–59) → `verified` (60–84) → `trusted` (85–100).
