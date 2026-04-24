use serde::{Deserialize, Serialize};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{Row, SqlitePool};
use std::path::Path;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DbCategory {
    pub cid: String,
    pub name: String,
    pub color: String,
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
        ];
        for stmt in &stmts {
            sqlx::query(stmt)
                .execute(&self.pool)
                .await
                .map_err(|e| format!("schema init: {e}"))?;
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
        let rows = sqlx::query("SELECT cid, name, color FROM categories ORDER BY rowid ASC")
            .fetch_all(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        Ok(rows
            .into_iter()
            .map(|r| DbCategory {
                cid: r.get(0),
                name: r.get(1),
                color: r.get(2),
            })
            .collect())
    }

    pub async fn save_categories(&self, cats: &[DbCategory]) -> Result<(), String> {
        sqlx::query("DELETE FROM categories")
            .execute(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        for cat in cats {
            sqlx::query("INSERT INTO categories (cid, name, color) VALUES (?1, ?2, ?3)")
                .bind(&cat.cid)
                .bind(&cat.name)
                .bind(&cat.color)
                .execute(&self.pool)
                .await
                .map_err(|e| e.to_string())?;
        }
        Ok(())
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
