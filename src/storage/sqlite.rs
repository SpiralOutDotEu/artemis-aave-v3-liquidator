use crate::strategies::aave_strategy::{Borrower, StateCache};
use anyhow::{Context, Result};
use async_trait::async_trait;
use rusqlite::{Connection, OpenFlags};
use std::{
    collections::HashMap,
    path::Path,
    sync::{Arc, Mutex},
};
use tracing::{debug, info};

/// SQLite storage implementation for borrower state persistence
/// 
/// Provides a clean, type-safe interface for storing and retrieving
/// borrower information and bot state. Uses connection pooling and
/// proper error handling following Rust best practices.
#[derive(Debug)]
pub struct SqliteStorage {
    /// Database connection pool for thread-safe access
    connections: Arc<Mutex<Vec<Connection>>>,
    /// Path to the SQLite database file
    db_path: String,
    /// Maximum number of connections in the pool
    max_connections: usize,
}

impl SqliteStorage {
    /// Creates a new SQLite storage instance
    /// 
    /// # Arguments
    /// * `db_path` - Path to the SQLite database file
    /// * `max_connections` - Maximum number of connections in the pool
    /// 
    /// # Returns
    /// * `Result<Self>` - Success or error from initialization
    pub fn new<P: AsRef<Path>>(db_path: P, max_connections: usize) -> Result<Self> {
        let db_path = db_path.as_ref().to_string_lossy().to_string();
        
        // Ensure the directory exists
        if let Some(parent) = Path::new(&db_path).parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create directory: {}", parent.display()))?;
        }
        
        let storage = Self {
            connections: Arc::new(Mutex::new(Vec::new())),
            db_path: db_path.clone(),
            max_connections,
        };
        
        // Initialize the database schema
        storage.initialize_schema()?;
        
        // Pre-populate the connection pool
        for _ in 0..max_connections {
            storage.add_connection()?;
        }
        
        info!("SQLite storage initialized at {} with {} connections", db_path, max_connections);
        Ok(storage)
    }
    
    /// Initializes the database schema with required tables
    fn initialize_schema(&self) -> Result<()> {
        let conn = self.get_connection()?;
        
        // Create borrowers table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS borrowers (
                address TEXT PRIMARY KEY NOT NULL,
                collateral_count INTEGER NOT NULL DEFAULT 0,
                debt_count INTEGER NOT NULL DEFAULT 0,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            )",
            [],
        ).with_context(|| "Failed to create borrowers table")?;
        
        // Create borrower_collateral table (many-to-many relationship)
        conn.execute(
            "CREATE TABLE IF NOT EXISTS borrower_collateral (
                borrower_address TEXT NOT NULL,
                asset_address TEXT NOT NULL,
                added_at INTEGER NOT NULL,
                PRIMARY KEY (borrower_address, asset_address),
                FOREIGN KEY (borrower_address) REFERENCES borrowers(address) ON DELETE CASCADE
            )",
            [],
        ).with_context(|| "Failed to create borrower_collateral table")?;
        
        // Create borrower_debt table (many-to-many relationship)
        conn.execute(
            "CREATE TABLE IF NOT EXISTS borrower_debt (
                borrower_address TEXT NOT NULL,
                asset_address TEXT NOT NULL,
                added_at INTEGER NOT NULL,
                PRIMARY KEY (borrower_address, asset_address),
                FOREIGN KEY (borrower_address) REFERENCES borrowers(address) ON DELETE CASCADE
            )",
            [],
        ).with_context(|| "Failed to create borrower_debt table")?;
        
        // Create bot_state table for tracking last processed block
        conn.execute(
            "CREATE TABLE IF NOT EXISTS bot_state (
                key TEXT PRIMARY KEY NOT NULL,
                value TEXT NOT NULL,
                updated_at INTEGER NOT NULL
            )",
            [],
        ).with_context(|| "Failed to create bot_state table")?;
        
        // Create indices for better query performance
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_borrower_collateral_asset ON borrower_collateral(asset_address)",
            [],
        ).with_context(|| "Failed to create collateral asset index")?;
        
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_borrower_debt_asset ON borrower_debt(asset_address)",
            [],
        ).with_context(|| "Failed to create debt asset index")?;
        
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_borrowers_updated ON borrowers(updated_at)",
            [],
        ).with_context(|| "Failed to create borrowers updated index")?;
        
        info!("Database schema initialized successfully");
        Ok(())
    }
    
    /// Gets a connection from the pool
    fn get_connection(&self) -> Result<Connection> {
        let mut connections = self.connections.lock()
            .map_err(|_| anyhow::anyhow!("Failed to acquire connection pool lock"))?;
        
        if let Some(conn) = connections.pop() {
            // Test if connection is still valid
            if conn.execute("SELECT 1", []).is_ok() {
                return Ok(conn);
            }
        }
        
        // Create new connection if pool is empty or connection is invalid
        self.add_connection()
    }
    
    /// Adds a new connection to the pool
    fn add_connection(&self) -> Result<Connection> {
        let conn = Connection::open_with_flags(
            &self.db_path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE,
        ).with_context(|| format!("Failed to open SQLite database: {}", self.db_path))?;
        
        // Enable foreign key constraints
        conn.execute("PRAGMA foreign_keys = ON", [])
            .with_context(|| "Failed to enable foreign key constraints")?;
        
        // Enable WAL mode for better concurrency
        conn.query_row("PRAGMA journal_mode = WAL", [], |_| Ok(()))
            .with_context(|| "Failed to enable WAL mode")?;
        
        // Set busy timeout
        conn.busy_timeout(std::time::Duration::from_secs(30))
            .with_context(|| "Failed to set busy timeout")?;
        
        Ok(conn)
    }
    
    /// Returns a connection to the pool
    fn return_connection(&self, conn: Connection) -> Result<()> {
        let mut connections = self.connections.lock()
            .map_err(|_| anyhow::anyhow!("Failed to acquire connection pool lock"))?;
        
        if connections.len() < self.max_connections {
            connections.push(conn);
        }
        
        Ok(())
    }
    
    /// Gets the current timestamp in seconds since Unix epoch
    fn current_timestamp() -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64
    }
}

#[async_trait]
impl super::Storage for SqliteStorage {
    /// Loads the complete state cache from the database
    async fn load_cache(&self) -> Result<StateCache> {
        let conn = self.get_connection()?;
        
        // Get last processed block number
        let last_block_number = conn.query_row(
            "SELECT value FROM bot_state WHERE key = 'last_block_number'",
            [],
            |row| row.get::<_, String>(0),
        ).unwrap_or_else(|_| "0".to_string())
        .parse::<u64>()
        .unwrap_or(0);
        
        // Get all borrowers with their collateral and debt
        let mut borrowers = HashMap::new();
        
        let mut borrower_rows = conn.prepare(
            "SELECT address, collateral_count, debt_count FROM borrowers"
        ).with_context(|| "Failed to prepare borrowers query")?;
        
        let mut borrower_iter = borrower_rows.query([])
            .with_context(|| "Failed to execute borrowers query")?;
        
        while let Some(borrower_row) = borrower_iter.next()? {
            let address: String = borrower_row.get(0)?;
            let address = address.parse()
                .with_context(|| format!("Invalid address format: {}", address))?;
            
            // Get collateral assets
            let collateral = self.get_borrower_assets(&conn, &address, "collateral")?;
            
            // Get debt assets
            let debt = self.get_borrower_assets(&conn, &address, "debt")?;
            
            borrowers.insert(address, Borrower {
                address,
                collateral,
                debt,
            });
        }
        
        info!("Loaded {} borrowers from database, last block: {}", borrowers.len(), last_block_number);
        
        Ok(StateCache {
            last_block_number,
            borrowers,
        })
    }
    
    /// Saves the complete state cache to the database
    async fn save_cache(&self, cache: &StateCache) -> Result<()> {
        let mut conn = self.get_connection()?;
        
        // Start transaction for atomicity
        let tx = conn.transaction()
            .with_context(|| "Failed to start database transaction")?;
        
        // Update last processed block
        tx.execute(
            "INSERT OR REPLACE INTO bot_state (key, value, updated_at) VALUES (?, ?, ?)",
            ("last_block_number", &cache.last_block_number.to_string(), Self::current_timestamp()),
        ).with_context(|| "Failed to update last block number")?;
        
        // Clear existing borrower data
        tx.execute("DELETE FROM borrower_collateral", [])
            .with_context(|| "Failed to clear borrower collateral")?;
        tx.execute("DELETE FROM borrower_debt", [])
            .with_context(|| "Failed to clear borrower debt")?;
        tx.execute("DELETE FROM borrowers", [])
            .with_context(|| "Failed to clear borrowers")?;
        
        // Insert new borrower data
        for (address, borrower) in &cache.borrowers {
            let address_str = format!("{:?}", address);
            let timestamp = Self::current_timestamp();
            
            // Insert borrower record
            tx.execute(
                "INSERT INTO borrowers (address, collateral_count, debt_count, created_at, updated_at) VALUES (?, ?, ?, ?, ?)",
                (
                    &address_str,
                    borrower.collateral.len() as i64,
                    borrower.debt.len() as i64,
                    timestamp,
                    timestamp,
                ),
            ).with_context(|| format!("Failed to insert borrower: {}", address_str))?;
            
            // Insert collateral assets
            for asset in &borrower.collateral {
                let asset_str = format!("{:?}", asset);
                tx.execute(
                    "INSERT INTO borrower_collateral (borrower_address, asset_address, added_at) VALUES (?, ?, ?)",
                    (&address_str, &asset_str, timestamp),
                ).with_context(|| format!("Failed to insert collateral asset: {} for borrower: {}", asset_str, address_str))?;
            }
            
            // Insert debt assets
            for asset in &borrower.debt {
                let asset_str = format!("{:?}", asset);
                tx.execute(
                    "INSERT INTO borrower_debt (borrower_address, asset_address, added_at) VALUES (?, ?, ?)",
                    (&address_str, &asset_str, timestamp),
                ).with_context(|| format!("Failed to insert debt asset: {} for borrower: {}", asset_str, address_str))?;
            }
        }
        
        // Commit transaction
        tx.commit()
            .with_context(|| "Failed to commit database transaction")?;
        
        info!("Saved {} borrowers to database, last block: {}", cache.borrowers.len(), cache.last_block_number);
        
        // Return connection to pool
        self.return_connection(conn)?;
        
        Ok(())
    }
    
    /// Adds a new borrower to the database
    async fn add_borrower(&self, borrower: &Borrower) -> Result<()> {
        let mut conn = self.get_connection()?;
        let address_str = format!("{:?}", borrower.address);
        let timestamp = Self::current_timestamp();
        
        // Start transaction
        let tx = conn.transaction()
            .with_context(|| "Failed to start database transaction")?;
        
        // Insert borrower record
        tx.execute(
            "INSERT OR REPLACE INTO borrowers (address, collateral_count, debt_count, created_at, updated_at) VALUES (?, ?, ?, ?, ?)",
            (
                &address_str,
                borrower.collateral.len() as i64,
                borrower.debt.len() as i64,
                timestamp,
                timestamp,
            ),
        ).with_context(|| format!("Failed to insert borrower: {}", address_str))?;
        
        // Insert collateral assets
        for asset in &borrower.collateral {
            let asset_str = format!("{:?}", asset);
            tx.execute(
                "INSERT OR REPLACE INTO borrower_collateral (borrower_address, asset_address, added_at) VALUES (?, ?, ?)",
                (&address_str, &asset_str, timestamp),
            ).with_context(|| format!("Failed to insert collateral asset: {} for borrower: {}", asset_str, address_str))?;
        }
        
        // Insert debt assets
        for asset in &borrower.debt {
            let asset_str = format!("{:?}", asset);
            tx.execute(
                "INSERT OR REPLACE INTO borrower_debt (borrower_address, asset_address, added_at) VALUES (?, ?, ?)",
                (&address_str, &asset_str, timestamp),
            ).with_context(|| format!("Failed to insert debt asset: {} for borrower: {}", asset_str, address_str))?;
        }
        
        // Commit transaction
        tx.commit()
            .with_context(|| "Failed to commit database transaction")?;
        
        debug!("Added borrower {} with {} collateral and {} debt assets", 
               address_str, borrower.collateral.len(), borrower.debt.len());
        
        // Return connection to pool
        self.return_connection(conn)?;
        
        Ok(())
    }
    
    /// Updates an existing borrower in the database
    async fn update_borrower(&self, borrower: &Borrower) -> Result<()> {
        let mut conn = self.get_connection()?;
        let address_str = format!("{:?}", borrower.address);
        let timestamp = Self::current_timestamp();
        
        // Start transaction
        let tx = conn.transaction()
            .with_context(|| "Failed to start database transaction")?;
        
        // Update borrower record
        tx.execute(
            "UPDATE borrowers SET collateral_count = ?, debt_count = ?, updated_at = ? WHERE address = ?",
            (
                borrower.collateral.len() as i64,
                borrower.debt.len() as i64,
                timestamp,
                &address_str,
            ),
        ).with_context(|| format!("Failed to update borrower: {}", address_str))?;
        
        // Remove old collateral and debt records
        tx.execute(
            "DELETE FROM borrower_collateral WHERE borrower_address = ?",
            [&address_str],
        ).with_context(|| format!("Failed to remove old collateral for borrower: {}", address_str))?;
        
        tx.execute(
            "DELETE FROM borrower_debt WHERE borrower_address = ?",
            [&address_str],
        ).with_context(|| format!("Failed to remove old debt for borrower: {}", address_str))?;
        
        // Insert new collateral assets
        for asset in &borrower.collateral {
            let asset_str = format!("{:?}", asset);
            tx.execute(
                "INSERT INTO borrower_collateral (borrower_address, asset_address, added_at) VALUES (?, ?, ?)",
                (&address_str, &asset_str, timestamp),
            ).with_context(|| format!("Failed to insert collateral asset: {} for borrower: {}", asset_str, address_str))?;
        }
        
        // Insert new debt assets
        for asset in &borrower.debt {
            let asset_str = format!("{:?}", asset);
            tx.execute(
                "INSERT INTO borrower_debt (borrower_address, asset_address, added_at) VALUES (?, ?, ?)",
                (&address_str, &asset_str, timestamp),
            ).with_context(|| format!("Failed to insert debt asset: {} for borrower: {}", asset_str, address_str))?;
        }
        
        // Commit transaction
        tx.commit()
            .with_context(|| "Failed to commit database transaction")?;
        
        debug!("Updated borrower {} with {} collateral and {} debt assets", 
               address_str, borrower.collateral.len(), borrower.debt.len());
        
        // Return connection to pool
        self.return_connection(conn)?;
        
        Ok(())
    }
    
    /// Removes a borrower from the database
    async fn remove_borrower(&self, address: &ethers::types::Address) -> Result<()> {
        let mut conn = self.get_connection()?;
        let address_str = format!("{:?}", address);
        
        // Start transaction
        let tx = conn.transaction()
            .with_context(|| "Failed to start database transaction")?;
        
        // Remove borrower (cascade will handle related records)
        tx.execute(
            "DELETE FROM borrowers WHERE address = ?",
            [&address_str],
        ).with_context(|| format!("Failed to remove borrower: {}", address_str))?;
        
        // Commit transaction
        tx.commit()
            .with_context(|| "Failed to commit database transaction")?;
        
        debug!("Removed borrower: {}", address_str);
        
        // Return connection to pool
        self.return_connection(conn)?;
        
        Ok(())
    }
    
    /// Gets a borrower by address
    async fn get_borrower(&self, address: &ethers::types::Address) -> Result<Option<Borrower>> {
        let conn = self.get_connection()?;
        let address_str = format!("{:?}", address);
        
        // Check if borrower exists
        let exists: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM borrowers WHERE address = ?)",
            [&address_str],
            |row| row.get(0),
        ).unwrap_or(false);
        
        if !exists {
            return Ok(None);
        }
        
        // Get collateral assets
        let collateral = self.get_borrower_assets(&conn, address, "collateral")?;
        
        // Get debt assets
        let debt = self.get_borrower_assets(&conn, address, "debt")?;
        
        let borrower = Borrower {
            address: *address,
            collateral,
            debt,
        };
        
        // Return connection to pool
        self.return_connection(conn)?;
        
        Ok(Some(borrower))
    }
    
    /// Gets all borrowers
    async fn get_all_borrowers(&self) -> Result<HashMap<ethers::types::Address, Borrower>> {
        let conn = self.get_connection()?;
        let mut borrowers = HashMap::new();
        
        // Get all borrower addresses first
        let mut borrower_addresses = Vec::new();
        {
            let mut borrower_rows = conn.prepare(
                "SELECT address FROM borrowers"
            ).with_context(|| "Failed to prepare borrowers query")?;
            
            let mut borrower_iter = borrower_rows.query([])
                .with_context(|| "Failed to execute borrowers query")?;
            
            while let Some(borrower_row) = borrower_iter.next()? {
                let address: String = borrower_row.get(0)?;
                let address = address.parse()
                    .with_context(|| format!("Invalid address format: {}", address))?;
                borrower_addresses.push(address);
            }
        } // borrower_rows and borrower_iter are dropped here
        
        // Now get detailed information for each borrower
        for address in borrower_addresses {
            // Get collateral assets
            let collateral = self.get_borrower_assets(&conn, &address, "collateral")?;
            
            // Get debt assets
            let debt = self.get_borrower_assets(&conn, &address, "debt")?;
            
            borrowers.insert(address, Borrower {
                address,
                collateral,
                debt,
            });
        }
        
        // Return connection to pool
        self.return_connection(conn)?;
        
        Ok(borrowers)
    }
    
    /// Gets the last processed block number
    async fn get_last_block_number(&self) -> Result<u64> {
        let conn = self.get_connection()?;
        
        let last_block_number = conn.query_row(
            "SELECT value FROM bot_state WHERE key = 'last_block_number'",
            [],
            |row| row.get::<_, String>(0),
        ).unwrap_or_else(|_| "0".to_string())
        .parse::<u64>()
        .unwrap_or(0);
        
        // Return connection to pool
        self.return_connection(conn)?;
        
        Ok(last_block_number)
    }
    
    /// Sets the last processed block number
    async fn set_last_block_number(&self, block_number: u64) -> Result<()> {
        let conn = self.get_connection()?;
        
        conn.execute(
            "INSERT OR REPLACE INTO bot_state (key, value, updated_at) VALUES (?, ?, ?)",
            ("last_block_number", &block_number.to_string(), Self::current_timestamp()),
        ).with_context(|| "Failed to update last block number")?;
        
        // Return connection to pool
        self.return_connection(conn)?;
        
        Ok(())
    }
}

impl SqliteStorage {
    /// Helper method to get assets for a specific borrower and type
    fn get_borrower_assets(
        &self,
        conn: &Connection,
        address: &ethers::types::Address,
        asset_type: &str,
    ) -> Result<std::collections::HashSet<ethers::types::Address>> {
        let address_str = format!("{:?}", address);
        let table_name = match asset_type {
            "collateral" => "borrower_collateral",
            "debt" => "borrower_debt",
            _ => return Err(anyhow::anyhow!("Invalid asset type: {}", asset_type)),
        };
        
        let mut assets = std::collections::HashSet::new();
        
        let mut asset_rows = conn.prepare(
            &format!("SELECT asset_address FROM {} WHERE borrower_address = ?", table_name)
        ).with_context(|| format!("Failed to prepare {} query", table_name))?;
        
        let mut asset_iter = asset_rows.query([&address_str])
            .with_context(|| format!("Failed to execute {} query", table_name))?;
        
        while let Some(asset_row) = asset_iter.next()? {
            let asset_str: String = asset_row.get(0)?;
            let asset = asset_str.parse()
                .with_context(|| format!("Invalid asset address format: {}", asset_str))?;
            assets.insert(asset);
        }
        
        Ok(assets)
    }
}

impl Drop for SqliteStorage {
    fn drop(&mut self) {
        // Close all connections in the pool
        if let Ok(mut connections) = self.connections.lock() {
            connections.clear();
        }
        info!("SQLite storage dropped, all connections closed");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::storage_trait::Storage;
    use ethers::types::Address;
    use std::collections::HashSet;
    use tempfile::tempdir;
    
    #[tokio::test]
    async fn test_sqlite_storage_lifecycle() -> Result<()> {
        let temp_dir = tempdir()?;
        let db_path = temp_dir.path().join("test.db");
        
        // Create storage
        let storage = SqliteStorage::new(&db_path, 2)?;
        
        // Test initial state
        let cache = storage.load_cache().await?;
        assert_eq!(cache.last_block_number, 0);
        assert_eq!(cache.borrowers.len(), 0);
        
        // Test adding a borrower
        let borrower = Borrower {
            address: Address::random(),
            collateral: HashSet::from([Address::random(), Address::random()]),
            debt: HashSet::from([Address::random()]),
        };
        
        storage.add_borrower(&borrower).await?;
        
        // Test retrieving the borrower
        let retrieved = storage.get_borrower(&borrower.address).await?;
        assert!(retrieved.is_some());
        let retrieved = retrieved.unwrap();
        assert_eq!(retrieved.address, borrower.address);
        assert_eq!(retrieved.collateral.len(), 2);
        assert_eq!(retrieved.debt.len(), 1);
        
        // Test updating the borrower
        let mut updated_borrower = borrower.clone();
        updated_borrower.collateral.insert(Address::random());
        storage.update_borrower(&updated_borrower).await?;
        
        let retrieved = storage.get_borrower(&borrower.address).await?;
        assert_eq!(retrieved.unwrap().collateral.len(), 3);
        
        // Test removing the borrower
        storage.remove_borrower(&borrower.address).await?;
        let retrieved = storage.get_borrower(&borrower.address).await?;
        assert!(retrieved.is_none());
        
        // Test block number persistence
        storage.set_last_block_number(12345).await?;
        assert_eq!(storage.get_last_block_number().await?, 12345);
        
        Ok(())
    }
    
    #[tokio::test]
    async fn test_sqlite_storage_concurrent_access() -> Result<()> {
        let temp_dir = tempdir()?;
        let db_path = temp_dir.path().join("concurrent_test.db");
        
        let storage = Arc::new(SqliteStorage::new(&db_path, 5)?);
        
        // Spawn multiple tasks to test concurrent access
        let mut handles = Vec::new();
        
        for i in 0..10 {
            let storage_clone = storage.clone();
            let handle = tokio::spawn(async move {
                let borrower = Borrower {
                    address: Address::random(),
                    collateral: HashSet::from([Address::random()]),
                    debt: HashSet::from([Address::random()]),
                };
                
                storage_clone.add_borrower(&borrower).await?;
                storage_clone.get_borrower(&borrower.address).await?;
                storage_clone.remove_borrower(&borrower.address).await?;
                
                Ok::<(), anyhow::Error>(())
            });
            handles.push(handle);
        }
        
        // Wait for all tasks to complete
        for handle in handles {
            handle.await??;
        }
        
        Ok(())
    }
}
