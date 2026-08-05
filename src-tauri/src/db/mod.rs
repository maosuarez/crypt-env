use serde::{Deserialize, Serialize};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{Row, SqlitePool};
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DbCategory {
    pub cid: String,
    pub name: String,
    pub color: String,
    pub description: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DbWorkspace {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    pub paths: Vec<String>,
    pub template: String,
    pub created: String,
    pub updated: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DbWorkspaceVar {
    pub id: i64,
    pub workspace_id: i64,
    pub key: String,
    pub item_id: Option<i64>,
    pub literal: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct ProjectDeleteImpact {
    pub environments: i64,
    pub items_deleted: i64,
    pub items_orphaned: i64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DbProject {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    pub template: String,
    pub created: String,
    pub updated: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DbEnvironment {
    pub id: i64,
    pub project_id: i64,
    pub name: String,
    pub is_default: bool,
    pub paths: Vec<String>,
    pub created: String,
    pub updated: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DbEnvironmentVar {
    pub id: i64,
    pub environment_id: i64,
    pub key: String,
    pub item_id: Option<i64>,
    pub literal: Option<String>,
}

pub struct VaultDb {
    pool: SqlitePool,
    path: String,
}

impl VaultDb {
    pub async fn open(path: &str) -> Result<Self, String> {
        let opts = SqliteConnectOptions::new()
            .filename(Path::new(path))
            .create_if_missing(true);

        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(opts)
            .await
            .map_err(|e| format!("db open: {e}"))?;

        let db = VaultDb { pool, path: path.to_string() };
        db.init_schema().await?;
        Ok(db)
    }

    async fn init_schema(&self) -> Result<(), String> {
        let stmts = [
            "PRAGMA journal_mode=WAL",
            "PRAGMA foreign_keys=ON",
            "CREATE TABLE IF NOT EXISTS vault_meta (
                id            INTEGER PRIMARY KEY CHECK(id = 1),
                kdf_salt      TEXT NOT NULL,
                verify_token  TEXT NOT NULL
            )",
            "CREATE TABLE IF NOT EXISTS items (
                id        INTEGER PRIMARY KEY AUTOINCREMENT,
                item_type TEXT NOT NULL,
                data      TEXT NOT NULL,
                created   TEXT NOT NULL,
                updated   TEXT NOT NULL
            )",
            "CREATE TABLE IF NOT EXISTS categories (
                cid   TEXT PRIMARY KEY,
                name  TEXT NOT NULL,
                color TEXT NOT NULL
            )",
            "CREATE TABLE IF NOT EXISTS settings (
                key   TEXT PRIMARY KEY,
                value TEXT NOT NULL
            )",
            "CREATE TABLE IF NOT EXISTS share_log (
                id         INTEGER PRIMARY KEY AUTOINCREMENT,
                mode       TEXT NOT NULL,
                direction  TEXT NOT NULL,
                item_ids   TEXT NOT NULL,
                peer_fp    TEXT,
                timestamp  TEXT NOT NULL
            )",
        ];
        for stmt in &stmts {
            sqlx::query(stmt)
                .execute(&self.pool)
                .await
                .map_err(|e| format!("schema init: {e}"))?;
        }
        // Additive migrations
        let _ = sqlx::query("ALTER TABLE categories ADD COLUMN description TEXT")
            .execute(&self.pool)
            .await;
        let _ = sqlx::query("ALTER TABLE items ADD COLUMN is_global INTEGER NOT NULL DEFAULT 0")
            .execute(&self.pool)
            .await;
        let migrations = [
            "CREATE TABLE IF NOT EXISTS workspaces (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                name        TEXT NOT NULL,
                description TEXT,
                path        TEXT,
                template    TEXT NOT NULL DEFAULT 'generic',
                created     TEXT NOT NULL,
                updated     TEXT NOT NULL
            )",
            "CREATE TABLE IF NOT EXISTS workspace_vars (
                id           INTEGER PRIMARY KEY AUTOINCREMENT,
                workspace_id INTEGER NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
                key          TEXT NOT NULL,
                item_id      INTEGER,
                literal      TEXT,
                UNIQUE(workspace_id, key)
            )",
            "CREATE TABLE IF NOT EXISTS workspace_paths (
                id           INTEGER PRIMARY KEY AUTOINCREMENT,
                workspace_id INTEGER NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
                path         TEXT NOT NULL,
                UNIQUE(workspace_id, path)
            )",
            "INSERT OR IGNORE INTO workspace_paths (workspace_id, path)
                SELECT id, path FROM workspaces WHERE path IS NOT NULL",
            // Projects/environments: additive, coexists with the legacy workspace
            // tables above (still used by the whole-workspace relay share feature).
            "CREATE TABLE IF NOT EXISTS projects (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                name        TEXT NOT NULL,
                description TEXT,
                template    TEXT NOT NULL DEFAULT 'generic',
                created     TEXT NOT NULL,
                updated     TEXT NOT NULL
            )",
            "CREATE TABLE IF NOT EXISTS environments (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                project_id  INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
                name        TEXT NOT NULL,
                is_default  INTEGER NOT NULL DEFAULT 0,
                created     TEXT NOT NULL,
                updated     TEXT NOT NULL,
                UNIQUE(project_id, name)
            )",
            "CREATE TABLE IF NOT EXISTS environment_vars (
                id             INTEGER PRIMARY KEY AUTOINCREMENT,
                environment_id INTEGER NOT NULL REFERENCES environments(id) ON DELETE CASCADE,
                key            TEXT NOT NULL,
                item_id        INTEGER,
                literal        TEXT,
                UNIQUE(environment_id, key)
            )",
            "CREATE TABLE IF NOT EXISTS environment_paths (
                id             INTEGER PRIMARY KEY AUTOINCREMENT,
                environment_id INTEGER NOT NULL REFERENCES environments(id) ON DELETE CASCADE,
                path           TEXT NOT NULL,
                UNIQUE(environment_id, path)
            )",
            // Backfill: one project per existing workspace (idempotent — PK conflicts are skipped).
            "INSERT OR IGNORE INTO projects (id, name, description, template, created, updated)
                SELECT id, name, description, template, created, updated FROM workspaces",
            // Backfill: every project without an environment yet gets a 'default' one.
            "INSERT INTO environments (project_id, name, is_default, created, updated)
                SELECT p.id, 'default', 1, p.created, p.updated
                FROM projects p
                WHERE NOT EXISTS (SELECT 1 FROM environments e WHERE e.project_id = p.id)",
            // Backfill: workspace_vars/workspace_paths into each project's default environment.
            "INSERT OR IGNORE INTO environment_vars (environment_id, key, item_id, literal)
                SELECT e.id, wv.key, wv.item_id, wv.literal
                FROM workspace_vars wv
                JOIN environments e ON e.project_id = wv.workspace_id AND e.name = 'default'",
            "INSERT OR IGNORE INTO environment_paths (environment_id, path)
                SELECT e.id, wp.path
                FROM workspace_paths wp
                JOIN environments e ON e.project_id = wp.workspace_id AND e.name = 'default'",
            // Item ownership: many-to-many so a global item can belong to more
            // than one project at once (see project::mod for the fork-on-unglobal
            // and cascade-delete-if-orphaned logic that relies on this table).
            "CREATE TABLE IF NOT EXISTS item_projects (
                item_id    INTEGER NOT NULL REFERENCES items(id) ON DELETE CASCADE,
                project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
                PRIMARY KEY (item_id, project_id)
            )",
            "CREATE TABLE IF NOT EXISTS project_categories (
                project_id  INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
                category_id TEXT NOT NULL REFERENCES categories(cid) ON DELETE CASCADE,
                PRIMARY KEY (project_id, category_id)
            )",
            // Backfill: items already referenced by an environment_var are owned
            // by that environment's project. `environment_vars.item_id` was never
            // FK-enforced, so a stale reference to an already-deleted item is
            // possible — the join against `items` skips those instead of failing
            // the new (enforced) item_projects FK.
            "INSERT OR IGNORE INTO item_projects (item_id, project_id)
                SELECT ev.item_id, e.project_id
                FROM environment_vars ev
                JOIN environments e ON e.id = ev.environment_id
                JOIN items i ON i.id = ev.item_id",
            // Case-insensitive uniqueness on project names. Prevents two
            // concurrent CLI auto-creates (see scope::resolve) from ever
            // landing two rows for the same folder-derived name — the
            // second insert now fails with a UNIQUE constraint violation
            // instead of silently creating a duplicate/orphaned project.
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_projects_name_nocase ON projects(name COLLATE NOCASE)",
        ];
        for stmt in &migrations {
            sqlx::query(stmt).execute(&self.pool).await.map_err(|e| format!("migration: {e}"))?;
        }

        // One-time (not re-run every launch): pre-existing items that end up with
        // zero owners after the backfill above predate the whole project/ownership
        // model — promote them to global so they surface in Global Secrets instead
        // of silently disappearing from the UI. Gated so it never re-flips items
        // that legitimately become owner-less later via normal project deletion.
        if self.get_setting("backfilled_global_orphans_v1").await?.is_none() {
            sqlx::query(
                "UPDATE items SET is_global = 1
                 WHERE id NOT IN (SELECT item_id FROM item_projects)",
            )
            .execute(&self.pool)
            .await
            .map_err(|e| format!("migration: {e}"))?;
            self.set_setting("backfilled_global_orphans_v1", "true").await?;
        }

        Ok(())
    }

    pub async fn is_initialized(&self) -> Result<bool, String> {
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM vault_meta")
            .fetch_one(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        Ok(count > 0)
    }

    pub async fn init_vault(&self, kdf_salt: &str, verify_token: &str) -> Result<(), String> {
        sqlx::query(
            "INSERT INTO vault_meta (id, kdf_salt, verify_token) VALUES (1, ?1, ?2)",
        )
        .bind(kdf_salt)
        .bind(verify_token)
        .execute(&self.pool)
        .await
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub async fn get_meta(&self) -> Result<Option<(String, String)>, String> {
        let row = sqlx::query("SELECT kdf_salt, verify_token FROM vault_meta WHERE id = 1")
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        Ok(row.map(|r| (r.get::<String, _>(0), r.get::<String, _>(1))))
    }

    /// Returns (id, item_type, encrypted_data, created, is_global).
    pub async fn list_items(&self) -> Result<Vec<(i64, String, String, String, bool)>, String> {
        let rows =
            sqlx::query("SELECT id, item_type, data, created, is_global FROM items ORDER BY id ASC")
                .fetch_all(&self.pool)
                .await
                .map_err(|e| e.to_string())?;
        Ok(rows
            .into_iter()
            .map(|r| {
                let is_global: i64 = r.get(4);
                (
                    r.get::<i64, _>(0),
                    r.get::<String, _>(1),
                    r.get::<String, _>(2),
                    r.get::<String, _>(3),
                    is_global != 0,
                )
            })
            .collect())
    }

    /// id = 0 → INSERT (returns new id). id > 0 → UPDATE (returns same id).
    /// `is_global` is written on both insert and update — callers must pass the
    /// item's current/intended value (updates never silently reset it).
    pub async fn upsert_item(
        &self,
        id: i64,
        item_type: &str,
        data: &str,
        created: &str,
        is_global: bool,
    ) -> Result<i64, String> {
        let now = now_ts();
        let is_global_i: i64 = if is_global { 1 } else { 0 };
        if id == 0 {
            let res = sqlx::query(
                "INSERT INTO items (item_type, data, created, updated, is_global) VALUES (?1, ?2, ?3, ?4, ?5)",
            )
            .bind(item_type)
            .bind(data)
            .bind(created)
            .bind(&now)
            .bind(is_global_i)
            .execute(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
            Ok(res.last_insert_rowid())
        } else {
            sqlx::query("UPDATE items SET data = ?1, updated = ?2, is_global = ?3 WHERE id = ?4")
                .bind(data)
                .bind(&now)
                .bind(is_global_i)
                .bind(id)
                .execute(&self.pool)
                .await
                .map_err(|e| e.to_string())?;
            Ok(id)
        }
    }

    pub async fn delete_item(&self, id: i64) -> Result<(), String> {
        sqlx::query("DELETE FROM environment_vars WHERE item_id = ?1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        sqlx::query("DELETE FROM items WHERE id = ?1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub async fn set_item_global(&self, id: i64, is_global: bool) -> Result<(), String> {
        let is_global_i: i64 = if is_global { 1 } else { 0 };
        sqlx::query("UPDATE items SET is_global = ?1 WHERE id = ?2")
            .bind(is_global_i)
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub async fn is_item_global(&self, id: i64) -> Result<Option<bool>, String> {
        let val: Option<i64> = sqlx::query_scalar("SELECT is_global FROM items WHERE id = ?1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        Ok(val.map(|v| v != 0))
    }

    pub async fn item_owner_count(&self, item_id: i64) -> Result<i64, String> {
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM item_projects WHERE item_id = ?1")
            .bind(item_id)
            .fetch_one(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        Ok(count)
    }

    pub async fn list_owning_projects(&self, item_id: i64) -> Result<Vec<i64>, String> {
        let rows = sqlx::query("SELECT project_id FROM item_projects WHERE item_id = ?1")
            .bind(item_id)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        Ok(rows.into_iter().map(|r| r.get(0)).collect())
    }

    pub async fn add_item_owner(&self, item_id: i64, project_id: i64) -> Result<(), String> {
        sqlx::query("INSERT OR IGNORE INTO item_projects (item_id, project_id) VALUES (?1, ?2)")
            .bind(item_id)
            .bind(project_id)
            .execute(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub async fn list_owned_item_ids(&self, project_id: i64) -> Result<Vec<i64>, String> {
        let rows = sqlx::query("SELECT item_id FROM item_projects WHERE project_id = ?1")
            .bind(project_id)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        Ok(rows.into_iter().map(|r| r.get(0)).collect())
    }

    pub async fn delete_environment_vars_by_item(&self, item_id: i64) -> Result<(), String> {
        sqlx::query("DELETE FROM environment_vars WHERE item_id = ?1")
            .bind(item_id)
            .execute(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Legacy literal-only vars (predating the "every var is a real item"
    /// model) — returns (environment_var_id, key, literal, owning project_id).
    pub async fn list_unmigrated_literal_vars(&self) -> Result<Vec<(i64, String, String, i64)>, String> {
        let rows = sqlx::query(
            "SELECT ev.id, ev.key, ev.literal, e.project_id
             FROM environment_vars ev
             JOIN environments e ON e.id = ev.environment_id
             WHERE ev.item_id IS NULL AND ev.literal IS NOT NULL",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| e.to_string())?;
        Ok(rows
            .into_iter()
            .map(|r| (r.get(0), r.get(1), r.get(2), r.get(3)))
            .collect())
    }

    pub async fn set_environment_var_item(&self, env_var_id: i64, item_id: i64) -> Result<(), String> {
        sqlx::query("UPDATE environment_vars SET item_id = ?1, literal = NULL WHERE id = ?2")
            .bind(item_id)
            .bind(env_var_id)
            .execute(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Repoints every environment_var in `project_id`'s environments that
    /// referenced `old_item_id` to `new_item_id` — used when forking a
    /// multi-owner item back into independent per-project copies.
    pub async fn repoint_env_var_item(
        &self,
        project_id: i64,
        old_item_id: i64,
        new_item_id: i64,
    ) -> Result<(), String> {
        sqlx::query(
            "UPDATE environment_vars SET item_id = ?1
             WHERE item_id = ?2 AND environment_id IN
                 (SELECT id FROM environments WHERE project_id = ?3)",
        )
        .bind(new_item_id)
        .bind(old_item_id)
        .bind(project_id)
        .execute(&self.pool)
        .await
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub async fn list_categories(&self) -> Result<Vec<DbCategory>, String> {
        let rows = sqlx::query("SELECT cid, name, color, description FROM categories ORDER BY rowid ASC")
            .fetch_all(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        Ok(rows
            .into_iter()
            .map(|r| DbCategory {
                cid: r.get(0),
                name: r.get(1),
                color: r.get(2),
                description: r.get(3),
            })
            .collect())
    }

    pub async fn save_categories(&self, cats: &[DbCategory]) -> Result<(), String> {
        sqlx::query("DELETE FROM categories")
            .execute(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        for cat in cats {
            sqlx::query(
                "INSERT INTO categories (cid, name, color, description) VALUES (?1, ?2, ?3, ?4)",
            )
            .bind(&cat.cid)
            .bind(&cat.name)
            .bind(&cat.color)
            .bind(&cat.description)
            .execute(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    pub async fn insert_category(&self, cat: &DbCategory) -> Result<(), String> {
        sqlx::query(
            "INSERT INTO categories (cid, name, color, description) VALUES (?1, ?2, ?3, ?4)",
        )
        .bind(&cat.cid)
        .bind(&cat.name)
        .bind(&cat.color)
        .bind(&cat.description)
        .execute(&self.pool)
        .await
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub async fn update_category(&self, cat: &DbCategory) -> Result<bool, String> {
        let res =
            sqlx::query("UPDATE categories SET name = ?1, color = ?2, description = ?3 WHERE cid = ?4")
                .bind(&cat.name)
                .bind(&cat.color)
                .bind(&cat.description)
                .bind(&cat.cid)
                .execute(&self.pool)
                .await
                .map_err(|e| e.to_string())?;
        Ok(res.rows_affected() > 0)
    }

    pub async fn delete_category(&self, cid: &str) -> Result<bool, String> {
        let res = sqlx::query("DELETE FROM categories WHERE cid = ?1")
            .bind(cid)
            .execute(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        Ok(res.rows_affected() > 0)
    }

    pub async fn get_setting(&self, key: &str) -> Result<Option<String>, String> {
        let val: Option<String> =
            sqlx::query_scalar("SELECT value FROM settings WHERE key = ?1")
                .bind(key)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| e.to_string())?;
        Ok(val)
    }

    /// Record a share event to the audit log.
    pub async fn log_share(
        &self,
        mode: &str,
        direction: &str,
        item_ids: &[i64],
        peer_fp: Option<&str>,
    ) -> Result<(), String> {
        let ids_str: String = item_ids
            .iter()
            .map(|id| id.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let ts = now_ts();
        sqlx::query(
            "INSERT INTO share_log (mode, direction, item_ids, peer_fp, timestamp) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
        )
        .bind(mode)
        .bind(direction)
        .bind(ids_str)
        .bind(peer_fp)
        .bind(ts)
        .execute(&self.pool)
        .await
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub async fn set_setting(&self, key: &str, value: &str) -> Result<(), String> {
        sqlx::query("INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)")
            .bind(key)
            .bind(value)
            .execute(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Re-key: atomically replaces vault_meta and re-encrypts all item blobs.
    pub async fn rekey(
        &self,
        new_salt: &str,
        new_token: &str,
        items: Vec<(i64, String)>,
    ) -> Result<(), String> {
        let mut tx = self.pool.begin().await.map_err(|e| e.to_string())?;
        sqlx::query("UPDATE vault_meta SET kdf_salt = ?1, verify_token = ?2 WHERE id = 1")
            .bind(new_salt)
            .bind(new_token)
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
        let now = now_ts();
        for (id, data) in &items {
            sqlx::query("UPDATE items SET data = ?1, updated = ?2 WHERE id = ?3")
                .bind(data.as_str())
                .bind(&now)
                .bind(id)
                .execute(&mut *tx)
                .await
                .map_err(|e| e.to_string())?;
        }
        tx.commit().await.map_err(|e| e.to_string())?;
        Ok(())
    }

    pub async fn list_workspaces(&self) -> Result<Vec<DbWorkspace>, String> {
        let rows = sqlx::query(
            "SELECT id, name, description, template, created, updated FROM workspaces ORDER BY id ASC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| e.to_string())?;

        let path_rows = sqlx::query(
            "SELECT workspace_id, path FROM workspace_paths ORDER BY workspace_id, id ASC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| e.to_string())?;

        let mut paths_map: HashMap<i64, Vec<String>> = HashMap::new();
        for row in path_rows {
            let ws_id: i64 = row.get(0);
            let path: String = row.get(1);
            paths_map.entry(ws_id).or_default().push(path);
        }

        Ok(rows
            .into_iter()
            .map(|r| {
                let id: i64 = r.get(0);
                DbWorkspace {
                    id,
                    name: r.get(1),
                    description: r.get(2),
                    paths: paths_map.remove(&id).unwrap_or_default(),
                    template: r.get(3),
                    created: r.get(4),
                    updated: r.get(5),
                }
            })
            .collect())
    }

    /// id = 0 → INSERT, returns new id. id > 0 → UPDATE, returns same id.
    pub async fn upsert_workspace(
        &self,
        id: i64,
        name: &str,
        description: Option<&str>,
        template: &str,
    ) -> Result<i64, String> {
        let now = now_ts();
        if id == 0 {
            let res = sqlx::query(
                "INSERT INTO workspaces (name, description, template, created, updated) VALUES (?1,?2,?3,?4,?5)",
            )
            .bind(name)
            .bind(description)
            .bind(template)
            .bind(&now)
            .bind(&now)
            .execute(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
            Ok(res.last_insert_rowid())
        } else {
            sqlx::query(
                "UPDATE workspaces SET name=?1, description=?2, template=?3, updated=?4 WHERE id=?5",
            )
            .bind(name)
            .bind(description)
            .bind(template)
            .bind(&now)
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
            Ok(id)
        }
    }

    pub async fn get_workspace_vars(&self, workspace_id: i64) -> Result<Vec<DbWorkspaceVar>, String> {
        let rows = sqlx::query(
            "SELECT id, workspace_id, key, item_id, literal FROM workspace_vars WHERE workspace_id = ?1 ORDER BY id ASC",
        )
        .bind(workspace_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| e.to_string())?;
        Ok(rows
            .into_iter()
            .map(|r| DbWorkspaceVar {
                id: r.get(0),
                workspace_id: r.get(1),
                key: r.get(2),
                item_id: r.get(3),
                literal: r.get(4),
            })
            .collect())
    }

    pub async fn set_workspace_vars(
        &self,
        workspace_id: i64,
        vars: &[DbWorkspaceVar],
    ) -> Result<(), String> {
        sqlx::query("DELETE FROM workspace_vars WHERE workspace_id = ?1")
            .bind(workspace_id)
            .execute(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        for v in vars {
            sqlx::query(
                "INSERT INTO workspace_vars (workspace_id, key, item_id, literal) VALUES (?1,?2,?3,?4)",
            )
            .bind(workspace_id)
            .bind(&v.key)
            .bind(v.item_id)
            .bind(&v.literal)
            .execute(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    pub async fn list_projects(&self) -> Result<Vec<DbProject>, String> {
        let rows = sqlx::query(
            "SELECT id, name, description, template, created, updated FROM projects ORDER BY id ASC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| e.to_string())?;
        Ok(rows
            .into_iter()
            .map(|r| DbProject {
                id: r.get(0),
                name: r.get(1),
                description: r.get(2),
                template: r.get(3),
                created: r.get(4),
                updated: r.get(5),
            })
            .collect())
    }

    /// id = 0 → INSERT, returns new id. id > 0 → UPDATE, returns same id.
    pub async fn upsert_project(
        &self,
        id: i64,
        name: &str,
        description: Option<&str>,
        template: &str,
    ) -> Result<i64, String> {
        let now = now_ts();
        if id == 0 {
            let res = sqlx::query(
                "INSERT INTO projects (name, description, template, created, updated) VALUES (?1,?2,?3,?4,?5)",
            )
            .bind(name)
            .bind(description)
            .bind(template)
            .bind(&now)
            .bind(&now)
            .execute(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
            Ok(res.last_insert_rowid())
        } else {
            sqlx::query(
                "UPDATE projects SET name=?1, description=?2, template=?3, updated=?4 WHERE id=?5",
            )
            .bind(name)
            .bind(description)
            .bind(template)
            .bind(&now)
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
            Ok(id)
        }
    }

    /// Read-only dry run of `delete_project`'s bookkeeping, for the
    /// typed-confirmation modal's blast-radius summary.
    pub async fn preview_delete_project(&self, project_id: i64) -> Result<ProjectDeleteImpact, String> {
        let environments: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM environments WHERE project_id = ?1")
                .bind(project_id)
                .fetch_one(&self.pool)
                .await
                .map_err(|e| e.to_string())?;

        let owned = self.list_owned_item_ids(project_id).await?;
        let mut items_deleted = 0i64;
        let mut items_orphaned = 0i64;
        for item_id in owned {
            if self.item_owner_count(item_id).await? <= 1 {
                if self.is_item_global(item_id).await?.unwrap_or(false) {
                    items_orphaned += 1;
                } else {
                    items_deleted += 1;
                }
            }
        }
        Ok(ProjectDeleteImpact { environments, items_deleted, items_orphaned })
    }

    /// Deletes a project and everything it exclusively owns, in one
    /// transaction. Environments/environment_vars/environment_paths and this
    /// project's `item_projects` ownership links cascade automatically via
    /// FK. Afterwards, for every item this project used to own: if it still
    /// has another owner, nothing to do; if it has none left, delete it
    /// (local secret with nowhere else to live) unless it's global, in which
    /// case it survives as an unowned orphan, still visible in Global Secrets.
    pub async fn delete_project(&self, id: i64) -> Result<ProjectDeleteImpact, String> {
        let mut tx = self.pool.begin().await.map_err(|e| e.to_string())?;

        let environments: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM environments WHERE project_id = ?1")
                .bind(id)
                .fetch_one(&mut *tx)
                .await
                .map_err(|e| e.to_string())?;

        let owned_rows = sqlx::query("SELECT item_id FROM item_projects WHERE project_id = ?1")
            .bind(id)
            .fetch_all(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
        let owned: Vec<i64> = owned_rows.into_iter().map(|r| r.get(0)).collect();

        sqlx::query("DELETE FROM projects WHERE id = ?1")
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;

        let mut items_deleted = 0i64;
        let mut items_orphaned = 0i64;
        for item_id in owned {
            let owner_count: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM item_projects WHERE item_id = ?1")
                    .bind(item_id)
                    .fetch_one(&mut *tx)
                    .await
                    .map_err(|e| e.to_string())?;
            if owner_count > 0 {
                continue;
            }
            let is_global: i64 = sqlx::query_scalar("SELECT is_global FROM items WHERE id = ?1")
                .bind(item_id)
                .fetch_one(&mut *tx)
                .await
                .map_err(|e| e.to_string())?;
            if is_global != 0 {
                items_orphaned += 1;
            } else {
                // Defensive: any stray environment_var reference (there shouldn't
                // be any left outside this project, since owner_count is 0) is
                // cleaned up before the item itself goes.
                sqlx::query("DELETE FROM environment_vars WHERE item_id = ?1")
                    .bind(item_id)
                    .execute(&mut *tx)
                    .await
                    .map_err(|e| e.to_string())?;
                sqlx::query("DELETE FROM items WHERE id = ?1")
                    .bind(item_id)
                    .execute(&mut *tx)
                    .await
                    .map_err(|e| e.to_string())?;
                items_deleted += 1;
            }
        }

        tx.commit().await.map_err(|e| e.to_string())?;
        Ok(ProjectDeleteImpact { environments, items_deleted, items_orphaned })
    }

    pub async fn set_project_categories(&self, project_id: i64, category_ids: &[String]) -> Result<(), String> {
        sqlx::query("DELETE FROM project_categories WHERE project_id = ?1")
            .bind(project_id)
            .execute(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        for cid in category_ids {
            sqlx::query(
                "INSERT OR IGNORE INTO project_categories (project_id, category_id) VALUES (?1, ?2)",
            )
            .bind(project_id)
            .bind(cid)
            .execute(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    /// Returns category NAMES (not ids) — same convention as `VaultItem.categories`.
    pub async fn list_project_categories(&self, project_id: i64) -> Result<Vec<String>, String> {
        let rows = sqlx::query(
            "SELECT c.name FROM project_categories pc
             JOIN categories c ON c.cid = pc.category_id
             WHERE pc.project_id = ?1",
        )
        .bind(project_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| e.to_string())?;
        Ok(rows.into_iter().map(|r| r.get(0)).collect())
    }

    pub async fn list_environments(&self, project_id: i64) -> Result<Vec<DbEnvironment>, String> {
        let rows = sqlx::query(
            "SELECT id, project_id, name, is_default, created, updated FROM environments WHERE project_id = ?1 ORDER BY id ASC",
        )
        .bind(project_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| e.to_string())?;

        let path_rows = sqlx::query(
            "SELECT ep.environment_id, ep.path FROM environment_paths ep
             JOIN environments e ON e.id = ep.environment_id
             WHERE e.project_id = ?1 ORDER BY ep.environment_id, ep.id ASC",
        )
        .bind(project_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| e.to_string())?;

        let mut paths_map: HashMap<i64, Vec<String>> = HashMap::new();
        for row in path_rows {
            let env_id: i64 = row.get(0);
            let path: String = row.get(1);
            paths_map.entry(env_id).or_default().push(path);
        }

        Ok(rows
            .into_iter()
            .map(|r| {
                let id: i64 = r.get(0);
                let is_default_i: i64 = r.get(3);
                DbEnvironment {
                    id,
                    project_id: r.get(1),
                    name: r.get(2),
                    is_default: is_default_i != 0,
                    paths: paths_map.remove(&id).unwrap_or_default(),
                    created: r.get(4),
                    updated: r.get(5),
                }
            })
            .collect())
    }

    pub async fn get_environment(&self, id: i64) -> Result<Option<DbEnvironment>, String> {
        let row = sqlx::query(
            "SELECT id, project_id, name, is_default, created, updated FROM environments WHERE id = ?1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| e.to_string())?;

        let row = match row {
            Some(r) => r,
            None => return Ok(None),
        };

        let is_default_i: i64 = row.get(3);
        let paths = self.get_environment_paths(id).await?;

        Ok(Some(DbEnvironment {
            id: row.get(0),
            project_id: row.get(1),
            name: row.get(2),
            is_default: is_default_i != 0,
            paths,
            created: row.get(4),
            updated: row.get(5),
        }))
    }

    /// id = 0 → INSERT, returns new id. id > 0 → UPDATE, returns same id.
    pub async fn upsert_environment(
        &self,
        id: i64,
        project_id: i64,
        name: &str,
        is_default: bool,
    ) -> Result<i64, String> {
        let now = now_ts();
        let is_default_i: i64 = if is_default { 1 } else { 0 };
        if id == 0 {
            let res = sqlx::query(
                "INSERT INTO environments (project_id, name, is_default, created, updated) VALUES (?1,?2,?3,?4,?5)",
            )
            .bind(project_id)
            .bind(name)
            .bind(is_default_i)
            .bind(&now)
            .bind(&now)
            .execute(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
            Ok(res.last_insert_rowid())
        } else {
            sqlx::query(
                "UPDATE environments SET name=?1, is_default=?2, updated=?3 WHERE id=?4",
            )
            .bind(name)
            .bind(is_default_i)
            .bind(&now)
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
            Ok(id)
        }
    }

    pub async fn delete_environment(&self, id: i64) -> Result<(), String> {
        sqlx::query("DELETE FROM environments WHERE id = ?1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub async fn get_environment_paths(&self, environment_id: i64) -> Result<Vec<String>, String> {
        let rows = sqlx::query(
            "SELECT path FROM environment_paths WHERE environment_id = ?1 ORDER BY id ASC",
        )
        .bind(environment_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| e.to_string())?;
        Ok(rows.into_iter().map(|r| r.get(0)).collect())
    }

    pub async fn set_environment_paths(
        &self,
        environment_id: i64,
        paths: &[String],
    ) -> Result<(), String> {
        sqlx::query("DELETE FROM environment_paths WHERE environment_id = ?1")
            .bind(environment_id)
            .execute(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        for path in paths {
            sqlx::query(
                "INSERT INTO environment_paths (environment_id, path) VALUES (?1, ?2)",
            )
            .bind(environment_id)
            .bind(path)
            .execute(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    pub async fn get_environment_vars(&self, environment_id: i64) -> Result<Vec<DbEnvironmentVar>, String> {
        let rows = sqlx::query(
            "SELECT id, environment_id, key, item_id, literal FROM environment_vars WHERE environment_id = ?1 ORDER BY id ASC",
        )
        .bind(environment_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| e.to_string())?;
        Ok(rows
            .into_iter()
            .map(|r| DbEnvironmentVar {
                id: r.get(0),
                environment_id: r.get(1),
                key: r.get(2),
                item_id: r.get(3),
                literal: r.get(4),
            })
            .collect())
    }

    pub async fn set_environment_vars(
        &self,
        environment_id: i64,
        vars: &[DbEnvironmentVar],
    ) -> Result<(), String> {
        sqlx::query("DELETE FROM environment_vars WHERE environment_id = ?1")
            .bind(environment_id)
            .execute(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        for v in vars {
            sqlx::query(
                "INSERT INTO environment_vars (environment_id, key, item_id, literal) VALUES (?1,?2,?3,?4)",
            )
            .bind(environment_id)
            .bind(&v.key)
            .bind(v.item_id)
            .bind(&v.literal)
            .execute(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    /// Links a single item into an environment under `key`, without touching
    /// any other vars already set on that environment (unlike
    /// `set_environment_vars`, which replaces the whole set). If `key` is
    /// already used in this environment, it's repointed to `item_id`.
    /// Returns the environment_vars row id.
    pub async fn upsert_environment_var(
        &self,
        environment_id: i64,
        key: &str,
        item_id: i64,
    ) -> Result<i64, String> {
        sqlx::query(
            "INSERT INTO environment_vars (environment_id, key, item_id, literal) VALUES (?1, ?2, ?3, NULL)
             ON CONFLICT(environment_id, key) DO UPDATE SET item_id = excluded.item_id, literal = NULL",
        )
        .bind(environment_id)
        .bind(key)
        .bind(item_id)
        .execute(&self.pool)
        .await
        .map_err(|e| e.to_string())?;

        sqlx::query_scalar("SELECT id FROM environment_vars WHERE environment_id = ?1 AND key = ?2")
            .bind(environment_id)
            .bind(key)
            .fetch_one(&self.pool)
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn wipe_and_reset(&mut self) -> Result<(), String> {
        self.pool.close().await;
        // Sobreescribir contenido con ceros antes de eliminar (mitigación forense básica)
        if let Ok(meta) = std::fs::metadata(&self.path) {
            if let Ok(mut f) = std::fs::OpenOptions::new().write(true).open(&self.path) {
                use std::io::Write;
                let zeros = vec![0u8; meta.len() as usize];
                let _ = f.write_all(&zeros);
            }
        }
        std::fs::remove_file(&self.path).map_err(|e| format!("wipe db: {e}"))?;
        let opts = SqliteConnectOptions::new()
            .filename(Path::new(&self.path))
            .create_if_missing(true);
        self.pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(opts)
            .await
            .map_err(|e| format!("db reopen: {e}"))?;
        self.init_schema().await
    }
}

fn now_ts() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Simulates a pre-migration install: only the legacy `workspaces` /
    /// `workspace_vars` / `workspace_paths` tables exist, with real data.
    /// Opening `VaultDb` against this file must run the additive migration
    /// and backfill a 'default' environment per project without touching
    /// the legacy rows.
    #[tokio::test]
    async fn migrates_legacy_workspace_into_project_default_environment() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("legacy.db");
        let path_str = path.to_str().unwrap().to_string();

        {
            let opts = SqliteConnectOptions::new()
                .filename(&path)
                .create_if_missing(true);
            let pool = SqlitePoolOptions::new().max_connections(1).connect_with(opts).await.unwrap();

            sqlx::query(
                "CREATE TABLE workspaces (
                    id          INTEGER PRIMARY KEY AUTOINCREMENT,
                    name        TEXT NOT NULL,
                    description TEXT,
                    path        TEXT,
                    template    TEXT NOT NULL DEFAULT 'generic',
                    created     TEXT NOT NULL,
                    updated     TEXT NOT NULL
                )",
            )
            .execute(&pool)
            .await
            .unwrap();
            sqlx::query(
                "CREATE TABLE workspace_vars (
                    id           INTEGER PRIMARY KEY AUTOINCREMENT,
                    workspace_id INTEGER NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
                    key          TEXT NOT NULL,
                    item_id      INTEGER,
                    literal      TEXT,
                    UNIQUE(workspace_id, key)
                )",
            )
            .execute(&pool)
            .await
            .unwrap();
            sqlx::query(
                "CREATE TABLE workspace_paths (
                    id           INTEGER PRIMARY KEY AUTOINCREMENT,
                    workspace_id INTEGER NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
                    path         TEXT NOT NULL,
                    UNIQUE(workspace_id, path)
                )",
            )
            .execute(&pool)
            .await
            .unwrap();

            sqlx::query(
                "INSERT INTO workspaces (id, name, description, template, created, updated)
                 VALUES (1, 'api-backend', 'legacy workspace', 'node', '1000', '1000')",
            )
            .execute(&pool)
            .await
            .unwrap();
            sqlx::query(
                "INSERT INTO workspace_vars (workspace_id, key, item_id, literal) VALUES
                 (1, 'DB_HOST', 42, NULL),
                 (1, 'DB_PASSWORD', NULL, 'literal-secret')",
            )
            .execute(&pool)
            .await
            .unwrap();
            sqlx::query(
                "INSERT INTO workspace_paths (workspace_id, path) VALUES
                 (1, '/srv/api-backend/.env'),
                 (1, '/srv/api-backend/.env.local')",
            )
            .execute(&pool)
            .await
            .unwrap();

            pool.close().await;
        }

        let db = VaultDb::open(&path_str).await.expect("open should run the migration");

        let projects = db.list_projects().await.unwrap();
        assert_eq!(projects.len(), 1, "one project should be backfilled from the legacy workspace");
        let project = &projects[0];
        assert_eq!(project.id, 1);
        assert_eq!(project.name, "api-backend");
        assert_eq!(project.description.as_deref(), Some("legacy workspace"));
        assert_eq!(project.template, "node");

        let environments = db.list_environments(project.id).await.unwrap();
        assert_eq!(environments.len(), 1, "exactly one 'default' environment per migrated project");
        let env = &environments[0];
        assert_eq!(env.name, "default");
        assert!(env.is_default);

        let mut paths = env.paths.clone();
        paths.sort();
        assert_eq!(paths, vec!["/srv/api-backend/.env", "/srv/api-backend/.env.local"]);

        let mut vars = db.get_environment_vars(env.id).await.unwrap();
        vars.sort_by(|a, b| a.key.cmp(&b.key));
        assert_eq!(vars.len(), 2);
        assert_eq!(vars[0].key, "DB_HOST");
        assert_eq!(vars[0].item_id, Some(42));
        assert_eq!(vars[1].key, "DB_PASSWORD");
        assert_eq!(vars[1].literal.as_deref(), Some("literal-secret"));

        // Legacy rows must survive untouched — the relay "share whole workspace"
        // feature still reads them directly until it moves to ProjectBundle.
        let legacy = db.list_workspaces().await.unwrap();
        assert_eq!(legacy.len(), 1);
        assert_eq!(legacy[0].name, "api-backend");

        // Re-opening (idempotent init_schema) must not create duplicate projects/environments.
        drop(db);
        let db2 = VaultDb::open(&path_str).await.unwrap();
        assert_eq!(db2.list_projects().await.unwrap().len(), 1);
        assert_eq!(db2.list_environments(1).await.unwrap().len(), 1);
    }

    // ─── upsert_environment_var (R2: ON CONFLICT repoint, not insert) ─────

    async fn fresh_env() -> (tempfile::TempDir, VaultDb, i64) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("vault.db");
        let db = VaultDb::open(path.to_str().unwrap()).await.unwrap();
        let project_id = db.upsert_project(0, "demo", None, "generic").await.unwrap();
        let env_id = db.upsert_environment(0, project_id, "production", true).await.unwrap();
        (dir, db, env_id)
    }

    #[tokio::test]
    async fn upsert_environment_var_first_insert_creates_a_row() {
        let (_dir, db, env_id) = fresh_env().await;
        let row_id = db.upsert_environment_var(env_id, "DB_HOST", 42).await.unwrap();

        let vars = db.get_environment_vars(env_id).await.unwrap();
        assert_eq!(vars.len(), 1);
        assert_eq!(vars[0].id, row_id);
        assert_eq!(vars[0].item_id, Some(42));
    }

    #[tokio::test]
    async fn upsert_environment_var_same_key_twice_repoints_not_duplicates() {
        let (_dir, db, env_id) = fresh_env().await;
        let first_id = db.upsert_environment_var(env_id, "DB_HOST", 42).await.unwrap();
        let second_id = db.upsert_environment_var(env_id, "DB_HOST", 99).await.unwrap();

        assert_eq!(first_id, second_id, "same key must repoint the same row, not insert a new one");
        let vars = db.get_environment_vars(env_id).await.unwrap();
        assert_eq!(vars.len(), 1, "ON CONFLICT must not leave two rows for the same (environment_id, key)");
        assert_eq!(vars[0].item_id, Some(99));
    }

    #[tokio::test]
    async fn upsert_environment_var_two_different_keys_yield_two_rows() {
        let (_dir, db, env_id) = fresh_env().await;
        db.upsert_environment_var(env_id, "DB_HOST", 1).await.unwrap();
        db.upsert_environment_var(env_id, "DB_PASSWORD", 2).await.unwrap();

        let vars = db.get_environment_vars(env_id).await.unwrap();
        assert_eq!(vars.len(), 2);
    }

    #[tokio::test]
    async fn upsert_environment_var_same_key_in_different_environments_are_independent() {
        let (dir, db, env_a) = fresh_env().await;
        let _ = &dir;
        let project_id = db.upsert_project(0, "other", None, "generic").await.unwrap();
        let env_b = db.upsert_environment(0, project_id, "staging", true).await.unwrap();

        db.upsert_environment_var(env_a, "SHARED_KEY", 1).await.unwrap();
        db.upsert_environment_var(env_b, "SHARED_KEY", 2).await.unwrap();

        let vars_a = db.get_environment_vars(env_a).await.unwrap();
        let vars_b = db.get_environment_vars(env_b).await.unwrap();
        assert_eq!(vars_a.len(), 1);
        assert_eq!(vars_b.len(), 1);
        assert_eq!(vars_a[0].item_id, Some(1));
        assert_eq!(vars_b[0].item_id, Some(2));
    }

    #[tokio::test]
    async fn upsert_environment_var_updating_one_key_leaves_others_untouched() {
        let (_dir, db, env_id) = fresh_env().await;
        db.upsert_environment_var(env_id, "DB_HOST", 1).await.unwrap();
        db.upsert_environment_var(env_id, "DB_PASSWORD", 2).await.unwrap();

        db.upsert_environment_var(env_id, "DB_HOST", 100).await.unwrap();

        let mut vars = db.get_environment_vars(env_id).await.unwrap();
        vars.sort_by(|a, b| a.key.cmp(&b.key));
        assert_eq!(vars.len(), 2);
        assert_eq!(vars[0].key, "DB_HOST");
        assert_eq!(vars[0].item_id, Some(100));
        assert_eq!(vars[1].key, "DB_PASSWORD");
        assert_eq!(vars[1].item_id, Some(2), "unrelated key must be untouched by repointing DB_HOST");
    }

    #[tokio::test]
    async fn upsert_environment_var_returned_id_matches_subsequent_lookup() {
        let (_dir, db, env_id) = fresh_env().await;
        let row_id = db.upsert_environment_var(env_id, "DB_HOST", 1).await.unwrap();

        let vars = db.get_environment_vars(env_id).await.unwrap();
        let found = vars.iter().find(|v| v.key == "DB_HOST").unwrap();
        assert_eq!(found.id, row_id);
    }
}
