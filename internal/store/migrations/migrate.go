// Package migrations provides an embedded SQL migration runner.
// Migration files are numbered sequentially: 001_initial.sql, 002_add_foo.sql, etc.
// The runner tracks applied versions in a schema_migrations table and applies
// only unapplied migrations in order.
package migrations

import (
	"database/sql"
	"embed"
	"fmt"
	"io/fs"
	"sort"
	"strconv"
	"strings"
	"time"
)

//go:embed *.sql
var files embed.FS

// Migration represents a single SQL migration file.
type Migration struct {
	Version int
	Name    string
	SQL     string
}

// Load reads all embedded *.sql files and returns them sorted by version number.
func Load() ([]Migration, error) {
	entries, err := fs.ReadDir(files, ".")
	if err != nil {
		return nil, fmt.Errorf("migrations: read dir: %w", err)
	}

	var migrations []Migration
	for _, e := range entries {
		if e.IsDir() || !strings.HasSuffix(e.Name(), ".sql") {
			continue
		}
		version, name, err := parseName(e.Name())
		if err != nil {
			return nil, err
		}
		content, err := files.ReadFile(e.Name())
		if err != nil {
			return nil, fmt.Errorf("migrations: read %s: %w", e.Name(), err)
		}
		migrations = append(migrations, Migration{
			Version: version,
			Name:    name,
			SQL:     string(content),
		})
	}
	sort.Slice(migrations, func(i, j int) bool {
		return migrations[i].Version < migrations[j].Version
	})
	return migrations, nil
}

// Up applies all unapplied migrations to db in order.
func Up(db *sql.DB) error {
	if err := ensureTable(db); err != nil {
		return err
	}
	applied, err := appliedVersions(db)
	if err != nil {
		return err
	}
	migrations, err := Load()
	if err != nil {
		return err
	}
	for _, m := range migrations {
		if applied[m.Version] {
			continue
		}
		if err := apply(db, m); err != nil {
			return fmt.Errorf("migrations: apply v%03d %s: %w", m.Version, m.Name, err)
		}
	}
	return nil
}

// Down rolls back the most recently applied migration.
// NOTE: SQL rollback is not generally reversible. This implementation only
// removes the version record; it does NOT execute a down-script. A full
// reversible migration system would require paired down files.
func Down(db *sql.DB) error {
	if err := ensureTable(db); err != nil {
		return err
	}
	row := db.QueryRow(`SELECT version FROM schema_migrations ORDER BY version DESC LIMIT 1`)
	var v int
	if err := row.Scan(&v); err == sql.ErrNoRows {
		return fmt.Errorf("migrations: nothing to roll back")
	} else if err != nil {
		return fmt.Errorf("migrations: query latest version: %w", err)
	}
	_, err := db.Exec(`DELETE FROM schema_migrations WHERE version = ?`, v)
	if err != nil {
		return fmt.Errorf("migrations: remove version record %d: %w", v, err)
	}
	fmt.Printf("migrations: rolled back version %03d (schema_migrations record removed; manual DDL rollback required)\n", v)
	return nil
}

// Version returns the highest applied migration version, or 0 if none.
func Version(db *sql.DB) (int, error) {
	if err := ensureTable(db); err != nil {
		return 0, err
	}
	row := db.QueryRow(`SELECT COALESCE(MAX(version), 0) FROM schema_migrations`)
	var v int
	return v, row.Scan(&v)
}

func ensureTable(db *sql.DB) error {
	_, err := db.Exec(`
		CREATE TABLE IF NOT EXISTS schema_migrations (
			version    INTEGER PRIMARY KEY,
			applied_at DATETIME NOT NULL
		)`)
	return err
}

func appliedVersions(db *sql.DB) (map[int]bool, error) {
	rows, err := db.Query(`SELECT version FROM schema_migrations`)
	if err != nil {
		return nil, fmt.Errorf("migrations: query applied: %w", err)
	}
	defer rows.Close()
	m := map[int]bool{}
	for rows.Next() {
		var v int
		if err := rows.Scan(&v); err != nil {
			return nil, err
		}
		m[v] = true
	}
	return m, rows.Err()
}

func apply(db *sql.DB, m Migration) error {
	tx, err := db.Begin()
	if err != nil {
		return err
	}
	defer tx.Rollback() //nolint:errcheck
	if _, err := tx.Exec(m.SQL); err != nil {
		return fmt.Errorf("exec: %w", err)
	}
	if _, err := tx.Exec(
		`INSERT INTO schema_migrations (version, applied_at) VALUES (?, ?)`,
		m.Version, time.Now().UTC(),
	); err != nil {
		return fmt.Errorf("record version: %w", err)
	}
	return tx.Commit()
}

// parseName extracts the version number and description from a migration filename.
// Format: NNN_description.sql where NNN is a zero-padded integer.
func parseName(filename string) (version int, name string, err error) {
	base := strings.TrimSuffix(filename, ".sql")
	idx := strings.IndexByte(base, '_')
	if idx < 0 {
		return 0, "", fmt.Errorf("migrations: invalid filename %q (expected NNN_name.sql)", filename)
	}
	v, err := strconv.Atoi(base[:idx])
	if err != nil {
		return 0, "", fmt.Errorf("migrations: invalid version in %q: %w", filename, err)
	}
	return v, base[idx+1:], nil
}
