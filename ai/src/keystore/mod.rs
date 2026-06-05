use parking_lot::RwLock;
/// Encrypted API key storage.
/// Keys encrypted with AES-256-GCM at rest, decrypted only in-memory during requests.
/// Uses `ring` for encryption and `rusqlite` for persistence.
use std::sync::Arc;
use zeroize::{Zeroize, ZeroizeOnDrop};

pub mod pool;
pub mod quota;

/// A decrypted API key that zeroizes on drop.
#[derive(Zeroize, ZeroizeOnDrop)]
pub struct DecryptedApiKey {
    #[zeroize]
    pub value: String,
    pub provider: String,
}

impl Clone for DecryptedApiKey {
    fn clone(&self) -> Self {
        Self {
            value: self.value.clone(),
            provider: self.provider.clone(),
        }
    }
}

impl std::fmt::Debug for DecryptedApiKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DecryptedApiKey")
            .field("provider", &self.provider)
            .field("value", &"***")
            .finish()
    }
}

/// Identifier for a stored API key.
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct KeyId(pub String);

/// In-memory only key store (no encryption, for development/testing).
/// In production, use `EncryptedKeyStore` with `keychain` feature.
pub struct EphemeralKeyStore {
    keys: Arc<RwLock<Vec<(KeyId, DecryptedApiKey)>>>,
}

impl EphemeralKeyStore {
    pub fn new() -> Self {
        EphemeralKeyStore {
            keys: Arc::new(RwLock::new(Vec::new())),
        }
    }

    pub fn add_key(&self, provider: &str, key: &str) -> KeyId {
        let id = KeyId(format!("{}-{}", provider, uuid::Uuid::new_v4()));
        let mut store = self.keys.write();
        store.push((
            id.clone(),
            DecryptedApiKey {
                value: key.to_string(),
                provider: provider.to_string(),
            },
        ));
        id
    }

    pub fn get_key(&self, id: &KeyId) -> Option<DecryptedApiKey> {
        let store = self.keys.read();
        store
            .iter()
            .find(|(k, _)| k.0 == id.0)
            .map(|(_, v)| DecryptedApiKey {
                value: v.value.clone(),
                provider: v.provider.clone(),
            })
    }

    pub fn list_keys(&self) -> Vec<(KeyId, String)> {
        let store = self.keys.read();
        store
            .iter()
            .map(|(id, key)| (id.clone(), key.provider.clone()))
            .collect()
    }

    pub fn remove_key(&self, id: &KeyId) {
        let mut store = self.keys.write();
        store.retain(|(k, _)| k.0 != id.0);
    }
}

impl Default for EphemeralKeyStore {
    fn default() -> Self {
        Self::new()
    }
}

// Re-export pool and quota types for convenience.
pub use pool::{
    KeyHealth, KeyInfo, KeySelectionStrategy, KeySource, KeyUsageSnapshot, PoolKeyEntry,
    ProviderKeyPool, SelectedKey,
};
pub use quota::ProviderQuota;

// ---------------------------------------------------------------------------
// Encrypted key store (requires `keychain` feature)
// ---------------------------------------------------------------------------

#[cfg(feature = "keychain")]
pub mod encrypted {
    use super::*;
    use crate::AiResult;
    use ring::aead::{Aad, LessSafeKey, Nonce, UnboundKey, AES_256_GCM};
    use ring::rand::{SecureRandom, SystemRandom};
    use rusqlite::Connection;
    use std::path::PathBuf;

    const NONCE_LEN: usize = 12;

    pub struct EncryptedKeyStore {
        db: Connection,
        cipher: LessSafeKey,
        rng: SystemRandom,
        db_path: PathBuf,
    }

    impl EncryptedKeyStore {
        /// Open or create the key store at the default path (~/.aurora/keys.db).
        /// `encryption_key` should be a 32-byte key (AES-256).
        pub fn open(encryption_key: &[u8; 32]) -> AiResult<Self> {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
            let dir = PathBuf::from(&home).join(".aurora");
            std::fs::create_dir_all(&dir).ok();
            let db_path = dir.join("keys.db");

            let db = Connection::open(&db_path)
                .map_err(|e| crate::error::AiError::KeyStore(format!("SQLite open: {}", e)))?;

            db.execute_batch(
                "CREATE TABLE IF NOT EXISTS api_keys (
                    key_id TEXT PRIMARY KEY,
                    provider TEXT NOT NULL,
                    label TEXT,
                    encrypted_value BLOB NOT NULL,
                    nonce BLOB NOT NULL,
                    created_at TEXT NOT NULL DEFAULT (datetime('now')),
                    last_used TEXT
                );",
            )
            .map_err(|e| crate::error::AiError::KeyStore(format!("SQLite init: {}", e)))?;

            let unbound = UnboundKey::new(&AES_256_GCM, encryption_key)
                .map_err(|e| crate::error::AiError::KeyStore(format!("Key setup: {}", e)))?;
            let cipher = LessSafeKey::new(unbound);
            let rng = SystemRandom::new();

            Ok(Self {
                db,
                cipher,
                rng,
                db_path,
            })
        }

        /// Store an API key encrypted at rest.
        pub fn add_key(&self, provider: &str, label: Option<&str>, plaintext: &str) -> AiResult<KeyId> {
            let key_id = KeyId(format!("{}-{}", provider, uuid::Uuid::new_v4()));

            let mut nonce_bytes = [0u8; NONCE_LEN];
            self.rng
                .fill(&mut nonce_bytes)
                .map_err(|e| crate::error::AiError::KeyStore(format!("RNG: {}", e)))?;
            let nonce = Nonce::assume_unique_for_key(nonce_bytes);

            let mut in_out = plaintext.as_bytes().to_vec();
            self.cipher
                .seal_in_place_append_tag(nonce, Aad::empty(), &mut in_out)
                .map_err(|e| crate::error::AiError::KeyStore(format!("Encrypt: {}", e)))?;

            self.db.execute(
                "INSERT INTO api_keys (key_id, provider, label, encrypted_value, nonce) VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![key_id.0, provider, label.unwrap_or(""), in_out, &nonce_bytes[..]],
            ).map_err(|e| crate::error::AiError::KeyStore(format!("SQLite insert: {}", e)))?;

            Ok(key_id)
        }

        /// Retrieve and decrypt an API key.
        pub fn get_key(&self, id: &KeyId) -> AiResult<DecryptedApiKey> {
            let mut stmt = self
                .db
                .prepare("SELECT provider, encrypted_value, nonce FROM api_keys WHERE key_id = ?1")
                .map_err(|e| crate::error::AiError::KeyStore(format!("SQLite prepare: {}", e)))?;

            let result = stmt.query_row(rusqlite::params![id.0], |row| {
                let provider: String = row.get(0)?;
                let encrypted: Vec<u8> = row.get(1)?;
                let nonce_bytes: Vec<u8> = row.get(2)?;
                Ok((provider, encrypted, nonce_bytes))
            });

            match result {
                Ok((provider, mut encrypted, nonce_bytes)) => {
                    let mut nonce_arr = [0u8; NONCE_LEN];
                    nonce_arr.copy_from_slice(&nonce_bytes);
                    let nonce = Nonce::assume_unique_for_key(nonce_arr);

                    let plaintext = self
                        .cipher
                        .open_in_place(nonce, Aad::empty(), &mut encrypted)
                        .map_err(|e| crate::error::AiError::KeyStore(format!("Decrypt: {}", e)))?;

                    let value = String::from_utf8(plaintext.to_vec())
                        .map_err(|e| crate::error::AiError::KeyStore(format!("UTF-8: {}", e)))?;

                    // Update last_used
                    self.db
                        .execute(
                            "UPDATE api_keys SET last_used = datetime('now') WHERE key_id = ?1",
                            rusqlite::params![id.0],
                        )
                        .ok();

                    Ok(DecryptedApiKey { value, provider })
                }
                Err(rusqlite::Error::QueryReturnedNoRows) => {
                    Err(crate::error::AiError::KeyNotFound(id.0.clone()))
                }
                Err(e) => Err(crate::error::AiError::KeyStore(format!(
                    "SQLite query: {}",
                    e
                ))),
            }
        }

        /// List all stored key IDs, providers, and labels (without revealing keys).
        pub fn list_keys(&self) -> AiResult<Vec<(KeyId, String, String)>> {
            let mut stmt = self
                .db
                .prepare("SELECT key_id, provider, label FROM api_keys ORDER BY created_at DESC")
                .map_err(|e| crate::error::AiError::KeyStore(format!("SQLite prepare: {}", e)))?;

            let rows = stmt
                .query_map([], |row| {
                    let key_id: String = row.get(0)?;
                    let provider: String = row.get(1)?;
                    let label: String = row.get(2)?;
                    Ok((KeyId(key_id), provider, label))
                })
                .map_err(|e| crate::error::AiError::KeyStore(format!("SQLite query: {}", e)))?;

            let mut keys = Vec::new();
            for row in rows {
                keys.push(row.map_err(|e| crate::error::AiError::KeyStore(format!("Row: {}", e)))?);
            }
            Ok(keys)
        }

        /// Remove a key from the store.
        pub fn remove_key(&self, id: &KeyId) -> AiResult<()> {
            self.db
                .execute(
                    "DELETE FROM api_keys WHERE key_id = ?1",
                    rusqlite::params![id.0],
                )
                .map_err(|e| crate::error::AiError::KeyStore(format!("SQLite delete: {}", e)))?;
            Ok(())
        }

        /// Path to the database file.
        pub fn db_path(&self) -> &PathBuf {
            &self.db_path
        }
    }
}
