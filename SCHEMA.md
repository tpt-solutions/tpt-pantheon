# tpt-identity Credential Schema Taxonomy

All schemas registered in `pkg/schema/core/`. Schema IDs use the form `{category}.{name}`. Issued credentials reference the **versioned** ID `{category}.{name}-v{version}` (e.g. `identity.legal-name-v1`).

Extra-sensitive schemas (marked ★) require an individual explicit consent grant even when a category-wide grant exists.

---

## identity

Core personal identity documents and government identifiers.

| Schema ID | Name | Required claims |
|-----------|------|-----------------|
| `identity.legal-name` | Legal Name | givenNames, familyName |
| `identity.dob` | Date of Birth | dateOfBirth (YYYY-MM-DD) |
| `identity.address` | Address | streetAddress, city, country |
| `identity.passport` | Passport | passportNumber, issuingCountry, expiryDate |
| `identity.drivers-licence` | Driver's Licence | licenceNumber, issuingAuthority, expiryDate |
| `identity.nhi` | NHI Number (NZ) | nhiNumber |
| `identity.ird-number` | IRD Number (NZ) | irdNumber |

---

## healthcare

Clinical and health records.

| Schema ID | Name | Notes |
|-----------|------|-------|
| `healthcare.gp-records` | GP Records | |
| `healthcare.specialist` | Specialist Records | |
| `healthcare.pharmacy` | Pharmacy Records | |
| `healthcare.allergies` | Allergies | |
| `healthcare.immunisation` | Immunisation | |
| `healthcare.radiology` | Radiology | |
| `healthcare.pathology` | Pathology | |
| `healthcare.dental` | Dental | |
| `healthcare.acc-injury` | ACC Injury | |
| `healthcare.disability` | Disability | |
| `healthcare.mental-health` ★ | Mental Health | Extra-sensitive |
| `healthcare.sexual-health` ★ | Sexual Health | Extra-sensitive |
| `healthcare.reproductive-health` ★ | Reproductive Health | Extra-sensitive |
| `healthcare.addiction` ★ | Addiction | Extra-sensitive |

---

## finance

Financial accounts, income, and assets.

| Schema ID | Name |
|-----------|------|
| `finance.bank-account` | Bank Account |
| `finance.income` | Income |
| `finance.tax-records` | Tax Records |
| `finance.credit-history` | Credit History |
| `finance.benefits` | Benefits |
| `finance.insurance-policies` | Insurance Policies |
| `finance.investments` | Investments |
| `finance.property-ownership` | Property Ownership |

---

## professional

Qualifications, registrations, and employment.

| Schema ID | Name |
|-----------|------|
| `professional.qualifications` | Qualifications |
| `professional.registrations` | Professional Registrations |
| `professional.employment` | Employment |
| `professional.practising-certificates` | Practising Certificates |

---

## education

Academic records.

| Schema ID | Name |
|-----------|------|
| `education.enrolments` | Enrolments |
| `education.transcripts` | Transcripts |
| `education.qualifications` | Qualifications |
| `education.nzqa` | NZQA Records |

---

## legal

Legal documents and status.

| Schema ID | Name | Notes |
|-----------|------|-------|
| `legal.court-orders` | Court Orders | |
| `legal.poa` | Power of Attorney | |
| `legal.will-estate` | Will / Estate | |
| `legal.immigration-status` | Immigration Status | |
| `legal.criminal-record` ★ | Criminal Record | Extra-sensitive |

---

## property

Real estate and personal property.

| Schema ID | Name |
|-----------|------|
| `property.real-estate` | Real Estate |
| `property.vehicles` | Vehicles |
| `property.assets` | Assets |

---

## civic

Electoral, civic, and business records.

| Schema ID | Name |
|-----------|------|
| `civic.electoral-roll` | Electoral Roll |
| `civic.benefits-entitlements` | Benefits Entitlements |
| `civic.tax-filing` | Tax Filing |
| `civic.business-registration` | Business Registration |

---

## social

Verified social connections.

| Schema ID | Name |
|-----------|------|
| `social.verified-contacts` | Verified Contacts |
| `social.social-graph` | Social Graph |
| `social.reputation` | Reputation |

---

## travel

Travel documents.

| Schema ID | Name |
|-----------|------|
| `travel.passport` | Passport |
| `travel.visas` | Visas |
| `travel.vaccination-certs` | Vaccination Certificates |
| `travel.travel-insurance` | Travel Insurance |

---

## insurance

Insurance policies.

| Schema ID | Name |
|-----------|------|
| `insurance.health` | Health Insurance |
| `insurance.life` | Life Insurance |
| `insurance.vehicle` | Vehicle Insurance |
| `insurance.home` | Home Insurance |
| `insurance.business` | Business Insurance |

---

## Registering custom schemas

```go
schema.RegisterCategory(schema.Category{
    ID:   "my-org",
    Name: "My Organisation",
})
schema.RegisterSchema(schema.Schema{
    ID:         "my-org.employee-badge",
    CategoryID: "my-org",
    Name:       "Employee Badge",
    Version:    1,
    Source:     schema.SourceThirdParty,
    Claims: []schema.ClaimDefinition{
        {Name: "employeeID", Type: "string", Required: true},
        {Name: "department", Type: "string"},
    },
})
```

The versioned ID `my-org.employee-badge-v1` will be stored in issued credentials.
