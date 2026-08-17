---
name: "create-migration"
description: "Creates a new SQLite database migration script with proper naming, SQL conventions, and FTS5 triggers. Invoke when adding or modifying database tables."
---

# Create Database Migration

Creates a new refinery migration script following project SQL naming conventions.

## When to Invoke

- User asks to create a new database table
- User asks to modify database schema
- User asks to add a migration
- Adding new tables for features

## Steps

### 1. Read Rules First

Before generating any SQL, read:
- `rules/sql.md` — SQL naming conventions and migration rules

### 2. Determine Migration Details

Ask or infer:
- What table(s) to create or modify
- What columns, types, constraints
- What indexes needed
- Whether FTS5 full-text search is needed
- Whether triggers are needed for FTS sync

### 3. Determine Migration Number

```bash
# Find the highest migration number
ls src-tauri/src/db/migrations/ | sort | tail -1
# Next number = highest + 1, formatted as V{NNN}
```

### 4. Generate Migration File

Write to `src-tauri/src/db/migrations/V{NNN}__{description}.sql`:

```sql
-- V{NNN}__{description}.sql
-- {Brief description of what this migration does}

CREATE TABLE {table_name} (
    id           TEXT PRIMARY KEY,
    -- snake_case column names
    -- TEXT for strings, INTEGER for numbers/booleans
    -- REFERENCES for foreign keys
    {column}     {TYPE} {CONSTRAINTS},
    is_deleted   INTEGER DEFAULT 0,           -- boolean as 0/1
    created_at   TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at   TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Indexes: idx_{table}_{columns}
CREATE INDEX idx_{table}_{column} ON {table}({column});
```

### 5. Naming Checklist (MUST verify)

- [ ] File name: `V{NNN}__{description}.sql` (double underscore, sequential number)
- [ ] Table name: snake_case, plural (`files`, `categories`, `operations_log`)
- [ ] Column names: snake_case (`content_hash`, `created_at`)
- [ ] Primary key: `id TEXT PRIMARY KEY` (UUID)
- [ ] Foreign key: `{ref_table_singular}_id` (e.g., `file_id`, `category_id`)
- [ ] Index: `idx_{table}_{columns}` (e.g., `idx_files_content_hash`)
- [ ] Timestamp: `{action}_at` TEXT ISO 8601 (e.g., `created_at`, `updated_at`)
- [ ] Boolean: `is_{state}` INTEGER 0/1 (e.g., `is_deleted`, `is_cloud`)
- [ ] Comment header: `-- V{NNN}__{description}.sql` + `-- {description}`

### 6. If FTS5 Needed

Add FTS5 virtual table and sync triggers:

```sql
-- FTS5 full-text search table
CREATE VIRTUAL TABLE {table}_fts USING fts5(
    {table}_id UNINDEXED,
    {searchable_column_1},
    {searchable_column_2},
    tokenize = 'unicode61'
);

-- Sync triggers
CREATE TRIGGER {table}_ai AFTER INSERT ON {table} BEGIN
    INSERT INTO {table}_fts({table}_id, {columns})
    VALUES (new.id, new.{columns});
END;

CREATE TRIGGER {table}_ad AFTER DELETE ON {table} BEGIN
    DELETE FROM {table}_fts WHERE {table}_id = old.id;
END;

CREATE TRIGGER {table}_au AFTER UPDATE ON {table} BEGIN
    DELETE FROM {table}_fts WHERE {table}_id = old.id;
    INSERT INTO {table}_fts({table}_id, {columns})
    VALUES (new.id, new.{columns});
END;
```

### 7. Migration Rules (MUST follow)

- [ ] Migration scripts are **immutable once committed** (refinery tracks by filename hash)
- [ ] DDL changes and DML data migrations in **separate** scripts
- [ ] No `DROP TABLE` without backup migration strategy
- [ ] No `ALTER TABLE ... DROP COLUMN` (SQLite doesn't support — use recreate pattern)
- [ ] Embedding model version changes: new LanceDB table (`documents_{model}_v{version}`), not modify old

### 8. If Data Migration Needed

If the migration includes data changes (DML), create a **separate** migration:

```sql
-- V{NNN+1}__migrate_{description}.sql
-- Data migration for {description}

UPDATE {table} SET {column} = {value} WHERE {condition};
```

### 9. Test Migration

```bash
# Verify SQL syntax (basic check)
sqlite3 :memory: < src-tauri/src/db/migrations/V{NNN}__{description}.sql

# Run refinery migrations (in test mode)
cargo test --manifest-path src-tauri/Cargo.toml db::tests
```

### 10. Update Models

If new table/columns added, update Rust models in `src-tauri/src/db/models.rs`:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct {EntityName}Record {
    pub id: String,
    // New columns as Rust types
    pub {column}: {rust_type},
    pub created_at: String,
    pub updated_at: String,
}
```
