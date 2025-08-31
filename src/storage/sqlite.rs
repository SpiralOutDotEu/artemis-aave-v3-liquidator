use crate::strategies::aave_strategy::{Borrower, StateCache, MissedLiquidationEvent};
use crate::storage::storage_trait::MissedLiquidationStats;
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
        
        // Create borrowers table with new fields for dirty tracking
        conn.execute(
            "CREATE TABLE IF NOT EXISTS borrowers (
                address TEXT PRIMARY KEY NOT NULL,
                collateral_count INTEGER NOT NULL DEFAULT 0,
                debt_count INTEGER NOT NULL DEFAULT 0,
                is_dirty INTEGER NOT NULL DEFAULT 1,
                last_touched_block INTEGER NOT NULL DEFAULT 0,
                last_health_factor INTEGER,
                last_reconciled_block INTEGER,
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
        
        // Create missed liquidations table for analysis
        conn.execute(
            "CREATE TABLE IF NOT EXISTS missed_liquidations (
                id TEXT PRIMARY KEY NOT NULL,
                tx_hash TEXT NOT NULL,
                block_number INTEGER NOT NULL,
                block_timestamp INTEGER NOT NULL,
                collateral_asset TEXT NOT NULL,
                debt_asset TEXT NOT NULL,
                user_address TEXT NOT NULL,
                debt_to_cover TEXT NOT NULL,
                liquidated_collateral_amount TEXT NOT NULL,
                liquidator TEXT NOT NULL,
                receive_a_token INTEGER NOT NULL,
                log_index INTEGER NOT NULL,
                gas_price TEXT,
                gas_used TEXT,
                tx_fee TEXT,
                base_fee TEXT,
                priority_fee TEXT,
                was_tracked_borrower INTEGER NOT NULL,
                our_health_factor TEXT,
                estimated_profit TEXT,
                missed_reason TEXT,
                recorded_at INTEGER NOT NULL
            )",
            [],
        ).with_context(|| "Failed to create missed_liquidations table")?;
        
        // Add new columns to existing borrowers table if they don't exist
        self.migrate_schema(&conn)?;
        
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
        
        // Create indices for missed liquidations table
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_missed_liquidations_block ON missed_liquidations(block_number)",
            [],
        ).with_context(|| "Failed to create missed liquidations block index")?;
        
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_missed_liquidations_user ON missed_liquidations(user_address)",
            [],
        ).with_context(|| "Failed to create missed liquidations user index")?;
        
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_missed_liquidations_tracked ON missed_liquidations(was_tracked_borrower)",
            [],
        ).with_context(|| "Failed to create missed liquidations tracked index")?;
        
        info!("Database schema initialized successfully");
        Ok(())
    }
    
    /// Migrates the schema to add new columns if they don't exist
    fn migrate_schema(&self, conn: &Connection) -> Result<()> {
        // Check if new columns exist and add them if they don't
        let columns = ["is_dirty", "last_touched_block", "last_health_factor", "last_reconciled_block"];
        
        for column in &columns {
            let exists: bool = conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM pragma_table_info('borrowers') WHERE name = ?)",
                [column],
                |row| row.get(0),
            ).unwrap_or(false);
            
            if !exists {
                let sql = match *column {
                    "is_dirty" => "ALTER TABLE borrowers ADD COLUMN is_dirty INTEGER NOT NULL DEFAULT 1",
                    "last_touched_block" => "ALTER TABLE borrowers ADD COLUMN last_touched_block INTEGER NOT NULL DEFAULT 0",
                    "last_health_factor" => "ALTER TABLE borrowers ADD COLUMN last_health_factor INTEGER",
                    "last_reconciled_block" => "ALTER TABLE borrowers ADD COLUMN last_reconciled_block INTEGER",
                    _ => continue,
                };
                
                conn.execute(sql, [])
                    .with_context(|| format!("Failed to add column {} to borrowers table", column))?;
            }
        }
        
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
            "SELECT address, collateral_count, debt_count, is_dirty, last_touched_block, last_health_factor, last_reconciled_block FROM borrowers"
        ).with_context(|| "Failed to prepare borrowers query")?;
        
        let mut borrower_iter = borrower_rows.query([])
            .with_context(|| "Failed to execute borrowers query")?;
        
        while let Some(borrower_row) = borrower_iter.next()? {
            let address: String = borrower_row.get(0)?;
            let address = address.parse()
                .with_context(|| format!("Invalid address format: {}", address))?;
            
            let is_dirty: bool = borrower_row.get(3)?;
            let last_touched_block: u64 = borrower_row.get(4)?;
            let last_health_factor: Option<u64> = borrower_row.get(5)?;
            let last_reconciled_block: Option<u64> = borrower_row.get(6)?;
            
            // Get collateral assets
            let collateral = self.get_borrower_assets(&conn, &address, "collateral")?;
            
            // Get debt assets
            let debt = self.get_borrower_assets(&conn, &address, "debt")?;
            
            borrowers.insert(address, Borrower {
                address,
                collateral,
                debt,
                is_dirty,
                last_touched_block,
                last_health_factor,
                last_reconciled_block,
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
                "INSERT INTO borrowers (address, collateral_count, debt_count, is_dirty, last_touched_block, last_health_factor, last_reconciled_block, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
                (
                    &address_str,
                    borrower.collateral.len() as i64,
                    borrower.debt.len() as i64,
                    borrower.is_dirty as i64,
                    borrower.last_touched_block as i64,
                    borrower.last_health_factor.map(|hf| hf as i64),
                    borrower.last_reconciled_block.map(|b| b as i64),
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
                "INSERT OR REPLACE INTO borrowers (address, collateral_count, debt_count, is_dirty, last_touched_block, last_health_factor, last_reconciled_block, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
                (
                    &address_str,
                    borrower.collateral.len() as i64,
                    borrower.debt.len() as i64,
                    borrower.is_dirty as i64,
                    borrower.last_touched_block as i64,
                    borrower.last_health_factor.map(|hf| hf as i64),
                    borrower.last_reconciled_block.map(|b| b as i64),
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
            "UPDATE borrowers SET collateral_count = ?, debt_count = ?, is_dirty = ?, last_touched_block = ?, last_health_factor = ?, last_reconciled_block = ?, updated_at = ? WHERE address = ?",
            (
                borrower.collateral.len() as i64,
                borrower.debt.len() as i64,
                borrower.is_dirty as i64,
                borrower.last_touched_block as i64,
                borrower.last_health_factor.map(|hf| hf as i64),
                borrower.last_reconciled_block.map(|b| b as i64),
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
        
        // Get borrower details
        let borrower_data = conn.query_row(
            "SELECT is_dirty, last_touched_block, last_health_factor, last_reconciled_block FROM borrowers WHERE address = ?",
            [&address_str],
            |row| {
                Ok((
                    row.get::<_, bool>(0)?,
                    row.get::<_, u64>(1)?,
                    row.get::<_, Option<u64>>(2)?,
                    row.get::<_, Option<u64>>(3)?,
                ))
            },
        ).with_context(|| format!("Failed to get borrower data for: {}", address_str))?;
        
        let (is_dirty, last_touched_block, last_health_factor, last_reconciled_block) = borrower_data;
        
        // Get collateral assets
        let collateral = self.get_borrower_assets(&conn, address, "collateral")?;
        
        // Get debt assets
        let debt = self.get_borrower_assets(&conn, address, "debt")?;
        
        let borrower = Borrower {
            address: *address,
            collateral,
            debt,
            is_dirty,
            last_touched_block,
            last_health_factor,
            last_reconciled_block,
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
            // Get borrower details
            let address_str = format!("{:?}", address);
            let borrower_data = conn.query_row(
                "SELECT is_dirty, last_touched_block, last_health_factor, last_reconciled_block FROM borrowers WHERE address = ?",
                [&address_str],
                |row| {
                    Ok((
                        row.get::<_, bool>(0)?,
                        row.get::<_, u64>(1)?,
                        row.get::<_, Option<u64>>(2)?,
                        row.get::<_, Option<u64>>(3)?,
                    ))
                },
            ).with_context(|| format!("Failed to get borrower data for: {}", address_str))?;
            
            let (is_dirty, last_touched_block, last_health_factor, last_reconciled_block) = borrower_data;
            
            // Get collateral assets
            let collateral = self.get_borrower_assets(&conn, &address, "collateral")?;
            
            // Get debt assets
            let debt = self.get_borrower_assets(&conn, &address, "debt")?;
            
            borrowers.insert(address, Borrower {
                address,
                collateral,
                debt,
                is_dirty,
                last_touched_block,
                last_health_factor,
                last_reconciled_block,
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
    
    /// Marks a borrower as dirty (needs reconciliation)
    async fn mark_borrower_dirty(&self, address: &ethers::types::Address, block_number: u64) -> Result<()> {
        let conn = self.get_connection()?;
        let address_str = format!("{:?}", address);
        let timestamp = Self::current_timestamp();
        
        conn.execute(
            "UPDATE borrowers SET is_dirty = 1, last_touched_block = ?, updated_at = ? WHERE address = ?",
            (block_number, timestamp, &address_str),
        ).with_context(|| format!("Failed to mark borrower as dirty: {}", address_str))?;
        
        debug!("Marked borrower {} as dirty at block {}", address_str, block_number);
        
        // Return connection to pool
        self.return_connection(conn)?;
        
        Ok(())
    }
    
    /// Gets all dirty borrowers that need reconciliation
    async fn get_dirty_borrowers(&self) -> Result<Vec<ethers::types::Address>> {
        let conn = self.get_connection()?;
        let mut dirty_addresses = Vec::new();
        
        {
            let mut rows = conn.prepare(
                "SELECT address FROM borrowers WHERE is_dirty = 1 ORDER BY last_touched_block ASC"
            ).with_context(|| "Failed to prepare dirty borrowers query")?;
            
            let mut iter = rows.query([])
                .with_context(|| "Failed to execute dirty borrowers query")?;
            
            while let Some(row) = iter.next()? {
                let address: String = row.get(0)?;
                let address = address.parse()
                    .with_context(|| format!("Invalid address format: {}", address))?;
                dirty_addresses.push(address);
            }
        } // rows and iter are dropped here, releasing the borrow
        
        // Return connection to pool
        self.return_connection(conn)?;
        
        Ok(dirty_addresses)
    }
    
    /// Marks a borrower as reconciled (no longer dirty)
    async fn mark_borrower_reconciled(&self, address: &ethers::types::Address, block_number: u64) -> Result<()> {
        let borrower: Option<Borrower> = self.get_borrower(address).await?;
        
        if let Some(_borrower) = borrower {
            let conn = self.get_connection()?;
            let address_str = format!("{:?}", address);
            let timestamp = Self::current_timestamp();
            
            conn.execute(
                "UPDATE borrowers SET is_dirty = 0, last_reconciled_block = ?, updated_at = ? WHERE address = ?",
                (block_number, timestamp, &address_str),
            ).with_context(|| format!("Failed to mark borrower as reconciled: {}", address_str))?;
            
            debug!("Marked borrower {} as reconciled at block {}", address_str, block_number);
            
            // Return connection to pool
            self.return_connection(conn)?;
        }
        
        Ok(())
    }
    
    /// Gets borrowers that are stale and should be pruned
    async fn get_stale_borrowers(&self, current_block: u64, ttl_blocks: u64) -> Result<Vec<ethers::types::Address>> {
        let conn = self.get_connection()?;
        let mut stale_addresses = Vec::new();
        
        let cutoff_block = current_block.saturating_sub(ttl_blocks);
        
        {
            let mut rows = conn.prepare(
                "SELECT address FROM borrowers WHERE last_touched_block < ? ORDER BY last_touched_block ASC"
            ).with_context(|| "Failed to prepare stale borrowers query")?;
            
            let mut iter = rows.query([cutoff_block])
                .with_context(|| "Failed to execute stale borrowers query")?;
            
            while let Some(row) = iter.next()? {
                let address: String = row.get(0)?;
                let address = address.parse()
                    .with_context(|| format!("Invalid address format: {}", address))?;
                stale_addresses.push(address);
        }
        } // rows and iter are dropped here, releasing the borrow
        
        // Return connection to pool
        self.return_connection(conn)?;
        
        Ok(stale_addresses)
    }
    
    /// Stores missed liquidation events for analysis
    async fn store_missed_liquidations(&self, events: &[MissedLiquidationEvent]) -> Result<()> {
        let mut conn = self.get_connection()?;
        
        // Start transaction for atomicity
        let tx = conn.transaction()
            .with_context(|| "Failed to start database transaction")?;
        
        for event in events {
            // Build the SQL with values directly to avoid parameter limit issues
            let sql = format!(
                "INSERT OR REPLACE INTO missed_liquidations (
                    id, tx_hash, block_number, block_timestamp, collateral_asset, debt_asset,
                    user_address, debt_to_cover, liquidated_collateral_amount, liquidator,
                    receive_a_token, log_index, gas_price, gas_used, tx_fee, base_fee,
                    priority_fee, was_tracked_borrower, our_health_factor, estimated_profit,
                    missed_reason, recorded_at
                ) VALUES (
                    '{}', '{}', {}, {}, '{}', '{}', '{}', '{}', '{}', '{}', {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, '{}', {}
                )",
                event.id,
                format!("{:?}", event.tx_hash),
                event.block_number,
                event.block_timestamp,
                format!("{:?}", event.collateral_asset),
                format!("{:?}", event.debt_asset),
                format!("{:?}", event.user),
                event.debt_to_cover,
                event.liquidated_collateral_amount,
                format!("{:?}", event.liquidator),
                event.receive_a_token as i64,
                event.log_index,
                event.gas_price.as_ref().map(|gp| format!("'{}'", gp)).unwrap_or_else(|| "NULL".to_string()),
                event.gas_used.as_ref().map(|gu| format!("'{}'", gu)).unwrap_or_else(|| "NULL".to_string()),
                event.tx_fee.as_ref().map(|tf| format!("'{}'", tf)).unwrap_or_else(|| "NULL".to_string()),
                event.base_fee.as_ref().map(|bf| format!("'{}'", bf)).unwrap_or_else(|| "NULL".to_string()),
                event.priority_fee.as_ref().map(|pf| format!("'{}'", pf)).unwrap_or_else(|| "NULL".to_string()),
                event.was_tracked_borrower as i64,
                event.our_health_factor.as_ref().map(|hf| format!("'{}'", hf)).unwrap_or_else(|| "NULL".to_string()),
                event.estimated_profit.as_ref().map(|ep| format!("'{}'", ep)).unwrap_or_else(|| "NULL".to_string()),
                event.missed_reason.as_ref().unwrap_or(&"Unknown".to_string()),
                event.recorded_at.timestamp()
            );
            
            tx.execute(&sql, [])
                .with_context(|| format!("Failed to insert missed liquidation event: {}", event.id))?;
        }
        
        // Commit transaction
        tx.commit()
            .with_context(|| "Failed to commit missed liquidations transaction")?;
        
        info!("Stored {} missed liquidation events", events.len());
        
        // Return connection to pool
        self.return_connection(conn)?;
        
        Ok(())
    }
    
    /// Gets missed liquidation events within a block range
    async fn get_missed_liquidations(&self, start_block: u64, end_block: u64) -> Result<Vec<MissedLiquidationEvent>> {
        let conn = self.get_connection()?;
        let mut events = Vec::new();
        
        {
            let mut rows = conn.prepare(
                "SELECT * FROM missed_liquidations WHERE block_number >= ? AND block_number <= ? ORDER BY block_number ASC"
            ).with_context(|| "Failed to prepare missed liquidations query")?;
            
            let mut iter = rows.query([start_block as i64, end_block as i64])
                .with_context(|| "Failed to execute missed liquidations query")?;
            
            while let Some(row) = iter.next()? {
                let event = self.parse_missed_liquidation_row(row)?;
                events.push(event);
            }
        }
        
        // Return connection to pool
        self.return_connection(conn)?;
        
        Ok(events)
    }
    
    /// Gets missed liquidation events for tracked borrowers only
    async fn get_missed_opportunities(&self, start_block: u64, end_block: u64) -> Result<Vec<MissedLiquidationEvent>> {
        let conn = self.get_connection()?;
        let mut events = Vec::new();
        
        {
            let mut rows = conn.prepare(
                "SELECT * FROM missed_liquidations WHERE block_number >= ? AND block_number <= ? AND was_tracked_borrower = 1 ORDER BY block_number ASC"
            ).with_context(|| "Failed to prepare missed opportunities query")?;
            
            let mut iter = rows.query([start_block as i64, end_block as i64])
                .with_context(|| "Failed to execute missed opportunities query")?;
            
            while let Some(row) = iter.next()? {
                let event = self.parse_missed_liquidation_row(row)?;
                events.push(event);
            }
        }
        
        // Return connection to pool
        self.return_connection(conn)?;
        
        Ok(events)
    }
    
    /// Gets statistics about missed liquidations
    async fn get_missed_liquidation_stats(&self, start_block: u64, end_block: u64) -> Result<MissedLiquidationStats> {
        let conn = self.get_connection()?;
        
        // Get total liquidations
        let total_liquidations: u64 = conn.query_row(
            "SELECT COUNT(*) FROM missed_liquidations WHERE block_number >= ? AND block_number <= ?",
            [start_block as i64, end_block as i64],
            |row| row.get(0),
        ).unwrap_or(0);
        
        // Get missed opportunities
        let missed_opportunities: u64 = conn.query_row(
            "SELECT COUNT(*) FROM missed_liquidations WHERE block_number >= ? AND block_number <= ? AND was_tracked_borrower = 1",
            [start_block as i64, end_block as i64],
            |row| row.get(0),
        ).unwrap_or(0);
        
        // Get total missed profit (rough estimate)
        let total_missed_profit: f64 = conn.query_row(
            "SELECT COALESCE(SUM(CAST(estimated_profit AS REAL)), 0) FROM missed_liquidations WHERE block_number >= ? AND block_number <= ? AND was_tracked_borrower = 1 AND estimated_profit IS NOT NULL",
            [start_block as i64, end_block as i64],
            |row| row.get(0),
        ).unwrap_or(0.0);
        
        // Calculate average
        let avg_missed_profit = if missed_opportunities > 0 {
            total_missed_profit / missed_opportunities as f64
        } else {
            0.0
        };
        
        // Get common missed reasons
        let mut common_missed_reasons = Vec::new();
        {
            let mut rows = conn.prepare(
                "SELECT missed_reason, COUNT(*) as count FROM missed_liquidations WHERE block_number >= ? AND block_number <= ? AND was_tracked_borrower = 1 AND missed_reason IS NOT NULL GROUP BY missed_reason ORDER BY count DESC LIMIT 10"
            ).with_context(|| "Failed to prepare missed reasons query")?;
            
            let mut iter = rows.query([start_block as i64, end_block as i64])
                .with_context(|| "Failed to execute missed reasons query")?;
            
            while let Some(row) = iter.next()? {
                let reason: String = row.get(0)?;
                let count: u64 = row.get(1)?;
                common_missed_reasons.push((reason, count));
            }
        }
        
        // Return connection to pool
        self.return_connection(conn)?;
        
        Ok(MissedLiquidationStats {
            total_liquidations,
            missed_opportunities,
            total_missed_profit,
            avg_missed_profit,
            common_missed_reasons,
        })
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
    
    /// Helper method to parse a missed liquidation row from the database
    fn parse_missed_liquidation_row(&self, row: &rusqlite::Row) -> Result<MissedLiquidationEvent> {
        use chrono::{DateTime, Utc};
        use std::str::FromStr;
        
        let id: String = row.get(0)?;
        let tx_hash_str: String = row.get(1)?;
        let tx_hash = ethers::types::H160::from_str(&tx_hash_str.trim_start_matches("0x"))
            .with_context(|| format!("Invalid tx hash format: {}", tx_hash_str))?;
        let block_number: i64 = row.get(2)?;
        let block_timestamp: i64 = row.get(3)?;
        let collateral_asset_str: String = row.get(4)?;
        let collateral_asset = ethers::types::Address::from_str(&collateral_asset_str.trim_start_matches("0x"))
            .with_context(|| format!("Invalid collateral asset format: {}", collateral_asset_str))?;
        let debt_asset_str: String = row.get(5)?;
        let debt_asset = ethers::types::Address::from_str(&debt_asset_str.trim_start_matches("0x"))
            .with_context(|| format!("Invalid debt asset format: {}", debt_asset_str))?;
        let user_str: String = row.get(6)?;
        let user = ethers::types::Address::from_str(&user_str.trim_start_matches("0x"))
            .with_context(|| format!("Invalid user address format: {}", user_str))?;
        let debt_to_cover_str: String = row.get(7)?;
        let debt_to_cover = ethers::types::U256::from_str(&debt_to_cover_str)
            .with_context(|| format!("Invalid debt_to_cover format: {}", debt_to_cover_str))?;
        let liquidated_collateral_amount_str: String = row.get(8)?;
        let liquidated_collateral_amount = ethers::types::U256::from_str(&liquidated_collateral_amount_str)
            .with_context(|| format!("Invalid liquidated_collateral_amount format: {}", liquidated_collateral_amount_str))?;
        let liquidator_str: String = row.get(9)?;
        let liquidator = ethers::types::Address::from_str(&liquidator_str.trim_start_matches("0x"))
            .with_context(|| format!("Invalid liquidator format: {}", liquidator_str))?;
        let receive_a_token: bool = row.get::<_, i64>(10)? != 0;
        let log_index: u64 = row.get::<_, i64>(11)? as u64;
        
        let gas_price = row.get::<_, Option<String>>(12)?
            .and_then(|s| ethers::types::U256::from_str(&s).ok());
        let gas_used = row.get::<_, Option<String>>(13)?
            .and_then(|s| ethers::types::U256::from_str(&s).ok());
        let tx_fee = row.get::<_, Option<String>>(14)?
            .and_then(|s| ethers::types::U256::from_str(&s).ok());
        let base_fee = row.get::<_, Option<String>>(15)?
            .and_then(|s| ethers::types::U256::from_str(&s).ok());
        let priority_fee = row.get::<_, Option<String>>(16)?
            .and_then(|s| ethers::types::U256::from_str(&s).ok());
        
        let was_tracked_borrower: bool = row.get::<_, i64>(17)? != 0;
        
        let our_health_factor = row.get::<_, Option<String>>(18)?
            .and_then(|s| ethers::types::U256::from_str(&s).ok());
        let estimated_profit = row.get::<_, Option<String>>(19)?
            .and_then(|s| ethers::types::I256::from_str(&s).ok());
        let missed_reason: Option<String> = row.get(20)?;
        let recorded_at_timestamp: i64 = row.get(21)?;
        let recorded_at = DateTime::from_timestamp(recorded_at_timestamp, 0)
            .unwrap_or_else(|| Utc::now());
        
        Ok(MissedLiquidationEvent {
            id,
            tx_hash,
            block_number: block_number as u64,
            block_timestamp: block_timestamp as u64,
            collateral_asset,
            debt_asset,
            user,
            debt_to_cover,
            liquidated_collateral_amount,
            liquidator,
            receive_a_token,
            log_index,
            gas_price,
            gas_used,
            tx_fee: tx_fee,
            base_fee,
            priority_fee,
            was_tracked_borrower,
            our_health_factor,
            estimated_profit,
            missed_reason,
            recorded_at,
        })
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
            is_dirty: true,
            last_touched_block: 0,
            last_health_factor: None,
            last_reconciled_block: None,
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
                    is_dirty: true,
                    last_touched_block: 0,
                    last_health_factor: None,
                    last_reconciled_block: None,
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
