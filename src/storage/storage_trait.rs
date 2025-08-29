use crate::strategies::aave_strategy::{Borrower, StateCache};
use anyhow::Result;
use async_trait::async_trait;

/// Trait defining the storage interface for borrower state persistence
/// 
/// This trait provides a clean abstraction for different storage backends,
/// allowing easy switching between JSON, SQLite, or other storage solutions.
#[async_trait]
pub trait Storage: Send + Sync {
    /// Loads the complete state cache from storage
    /// 
    /// # Returns
    /// * `Result<StateCache>` - Success or error from loading
    async fn load_cache(&self) -> Result<StateCache>;
    
    /// Saves the complete state cache to storage
    /// 
    /// # Arguments
    /// * `cache` - The state cache to save
    /// 
    /// # Returns
    /// * `Result<()>` - Success or error from saving
    async fn save_cache(&self, cache: &StateCache) -> Result<()>;
    
    /// Adds a new borrower to storage
    /// 
    /// # Arguments
    /// * `borrower` - The borrower to add
    /// 
    /// # Returns
    /// * `Result<()>` - Success or error from adding
    async fn add_borrower(&self, borrower: &Borrower) -> Result<()>;
    
    /// Updates an existing borrower in storage
    /// 
    /// # Arguments
    /// * `borrower` - The borrower to update
    /// 
    /// # Returns
    /// * `Result<()>` - Success or error from updating
    async fn update_borrower(&self, borrower: &Borrower) -> Result<()>;
    
    /// Removes a borrower from storage
    /// 
    /// # Arguments
    /// * `address` - The address of the borrower to remove
    /// 
    /// # Returns
    /// * `Result<()>` - Success or error from removing
    async fn remove_borrower(&self, address: &ethers::types::Address) -> Result<()>;
    
    /// Gets a borrower by address
    /// 
    /// # Arguments
    /// * `address` - The address of the borrower to retrieve
    /// 
    /// # Returns
    /// * `Result<Option<Borrower>>` - Success or error from retrieval, with optional borrower
    async fn get_borrower(&self, address: &ethers::types::Address) -> Result<Option<Borrower>>;
    
    /// Gets all borrowers from storage
    /// 
    /// # Returns
    /// * `Result<HashMap<Address, Borrower>>` - Success or error from retrieval, with all borrowers
    async fn get_all_borrowers(&self) -> Result<std::collections::HashMap<ethers::types::Address, Borrower>>;
    
    /// Gets the last processed block number
    /// 
    /// # Returns
    /// * `Result<u64>` - Success or error from retrieval, with block number
    async fn get_last_block_number(&self) -> Result<u64>;
    
    /// Sets the last processed block number
    /// 
    /// # Arguments
    /// * `block_number` - The block number to set
    /// 
    /// # Returns
    /// * `Result<()>` - Success or error from setting
    async fn set_last_block_number(&self, block_number: u64) -> Result<()>;
    
    /// Marks a borrower as dirty (needs reconciliation)
    /// 
    /// # Arguments
    /// * `address` - The address of the borrower to mark as dirty
    /// * `block_number` - The current block number
    /// 
    /// # Returns
    /// * `Result<()>` - Success or error from marking as dirty
    async fn mark_borrower_dirty(&self, address: &ethers::types::Address, block_number: u64) -> Result<()>;
    
    /// Gets all dirty borrowers that need reconciliation
    /// 
    /// # Returns
    /// * `Result<Vec<Address>>` - List of dirty borrower addresses
    async fn get_dirty_borrowers(&self) -> Result<Vec<ethers::types::Address>>;
    
    /// Marks a borrower as reconciled (no longer dirty)
    /// 
    /// # Arguments
    /// * `address` - The address of the borrower to mark as reconciled
    /// * `block_number` - The current block number
    /// 
    /// # Returns
    /// * `Result<()>` - Success or error from marking as reconciled
    async fn mark_borrower_reconciled(&self, address: &ethers::types::Address, block_number: u64) -> Result<()>;
    
    /// Gets borrowers that are stale and should be pruned
    /// 
    /// # Arguments
    /// * `current_block` - The current block number
    /// * `ttl_blocks` - The TTL in blocks for considering borrowers stale
    /// 
    /// # Returns
    /// * `Result<Vec<Address>>` - List of stale borrower addresses
    async fn get_stale_borrowers(&self, current_block: u64, ttl_blocks: u64) -> Result<Vec<ethers::types::Address>>;
}
