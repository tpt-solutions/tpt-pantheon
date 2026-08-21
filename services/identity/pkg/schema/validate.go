package schema

import (
	"fmt"
	"strconv"
	"sync"

	"github.com/santhosh-tekuri/jsonschema/v6"
)

// compiled caches *jsonschema.Schema keyed by the canonical base schema ID.
var compiled sync.Map

// Validate checks that claims (string-serialised values, as stored in VCs)
// satisfy the required fields and type constraints defined by schemaID.
// Accepts both versioned ("identity.legal-name-v1") and base ("identity.legal-name") IDs.
func Validate(schemaID string, claims map[string]string) error {
	sch, err := compiledFor(schemaID)
	if err != nil {
		return err
	}
	// Convert map[string]string → map[string]any for the JSON Schema validator.
	data := make(map[string]any, len(claims))
	for k, v := range claims {
		data[k] = v
	}
	if err := sch.Validate(data); err != nil {
		return fmt.Errorf("schema %s: %w", schemaID, err)
	}
	return nil
}

// ValidateAny validates typed JSON claim values (as parsed from a credential body)
// against the schema at schemaID. Number and boolean values are normalised to strings
// to match the VC string-serialisation convention before validation.
func ValidateAny(schemaID string, claims map[string]any) error {
	str := make(map[string]string, len(claims))
	for k, v := range claims {
		switch val := v.(type) {
		case string:
			str[k] = val
		case float64:
			str[k] = strconv.FormatFloat(val, 'f', -1, 64)
		case bool:
			if val {
				str[k] = "true"
			} else {
				str[k] = "false"
			}
		default:
			str[k] = fmt.Sprintf("%v", v)
		}
	}
	return Validate(schemaID, str)
}

// compiledFor returns the compiled (and cached) JSON Schema for schemaID.
func compiledFor(schemaID string) (*jsonschema.Schema, error) {
	s, err := GetSchema(schemaID)
	if err != nil {
		return nil, err
	}
	if v, ok := compiled.Load(s.ID); ok {
		return v.(*jsonschema.Schema), nil
	}
	sch, err := compileSchema(s)
	if err != nil {
		return nil, fmt.Errorf("compile schema %s: %w", s.ID, err)
	}
	compiled.Store(s.ID, sch)
	return sch, nil
}

func compileSchema(s Schema) (*jsonschema.Schema, error) {
	c := jsonschema.NewCompiler()
	// Enable format assertions so "format":"date" is actively validated,
	// not treated as a mere annotation.
	c.AssertFormat()
	url := "tpt://" + s.ID + ".json"
	if err := c.AddResource(url, schemaDoc(s)); err != nil {
		return nil, err
	}
	return c.Compile(url)
}

// schemaDoc builds a JSON Schema 2020-12 document from a Schema's ClaimDefinitions.
// All claim values are treated as strings to match the VC serialisation format.
func schemaDoc(s Schema) map[string]any {
	props := make(map[string]any, len(s.Claims))
	var required []any
	for _, def := range s.Claims {
		props[def.Name] = claimProp(def.Type)
		if def.Required {
			required = append(required, def.Name)
		}
	}
	doc := map[string]any{
		"$schema":              "https://json-schema.org/draft/2020-12/schema",
		"type":                 "object",
		"properties":           props,
		"additionalProperties": true,
	}
	if len(required) > 0 {
		doc["required"] = required
	}
	return doc
}

// claimProp returns the JSON Schema property descriptor for a ClaimDefinition type.
// All values are strings (VC serialisation format), with format/pattern constraints.
func claimProp(typ string) map[string]any {
	switch typ {
	case "date":
		// ISO 8601 date; enforcement requires c.AssertFormat().
		return map[string]any{"type": "string", "format": "date"}
	case "number":
		// Numeric value serialised as a decimal string.
		return map[string]any{"type": "string", "pattern": `^-?[0-9]+(\.[0-9]+)?$`}
	case "boolean":
		return map[string]any{"enum": []any{"true", "false"}}
	default: // "string"
		return map[string]any{"type": "string", "minLength": 1}
	}
}
