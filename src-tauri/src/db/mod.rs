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
        ];
        for stmt in &migrations {
            sqlx::query(stmt).execute(&self.pool).await.map_err(|e| format!("migration: {e}"))?;
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

    /// Returns (id, item_type, encrypted_data, created).
    pub async fn list_items(&self) -> Result<Vec<(i64, String, String, String)>, String> {
        let rows =
            sqlx::query("SELECT id, item_type, data, created FROM items ORDER BY id ASC")
                .fetch_all(&self.pool)
                .await
                .map_err(|e| e.to_string())?;
        Ok(rows
            .into_iter()
            .map(|r| {
                (
                    r.get::<i64, _>(0),
                    r.get::<String, _>(1),
                    r.get::<String, _>(2),
                    r.get::<String, _>(3),
                )
            })
            .collect())
    }

    /// id = 0 → INSERT (returns new id). id > 0 → UPDATE (returns same id).
    pub async fn upsert_item(
        &self,
        id: i64,
        item_type: &str,
        data: &str,
        created: &str,
    ) -> Result<i64, String> {
        let now = now_ts();
        if id == 0 {
            let res = sqlx::query(
                "INSERT INTO items (item_type, data, created, updated) VALUES (?1, ?2, ?3, ?4)",
            )
            .bind(item_type)
            .bind(data)
            .bind(created)
            .bind(&now)
            .execute(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
            Ok(res.last_insert_rowid())
        } else {
            sqlx::query("UPDATE items SET data = ?1, updated = ?2 WHERE id = ?3")
                .bind(data)
                .bind(&now)
                .bind(id)
                .execute(&self.pool)
                .await
                .map_err(|e| e.to_string())?;
            Ok(id)
        }
    }

    pub async fn delete_item(&self, id: i64) -> Result<(), String> {
        sqlx::query("DELETE FROM items WHERE id = ?1")
            .bind(id)
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

    pub async fn get_workspace_paths(&self, workspace_id: i64) -> Result<Vec<String>, String> {
        let rows = sqlx::query(
            "SELECT path FROM workspace_paths WHERE workspace_id = ?1 ORDER BY id ASC",
        )
        .bind(workspace_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| e.to_string())?;
        Ok(rows.into_iter().map(|r| r.get(0)).collect())
    }

    pub async fn set_workspace_paths(
        &self,
        workspace_id: i64,
        paths: &[String],
    ) -> Result<(), String> {
        sqlx::query("DELETE FROM workspace_paths WHERE workspace_id = ?1")
            .bind(workspace_id)
            .execute(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        for path in paths {
            sqlx::query(
                "INSERT INTO workspace_paths (workspace_id, path) VALUES (?1, ?2)",
            )
            .bind(workspace_id)
            .bind(path)
            .execute(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    pub async fn delete_workspace(&self, id: i64) -> Result<(), String> {
        sqlx::query("DELETE FROM workspaces WHERE id = ?1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
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
