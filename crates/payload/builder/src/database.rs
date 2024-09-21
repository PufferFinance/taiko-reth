//! Database adapters for payload building.

use reth_primitives::{
    revm_primitives::{
        db::{SyncDatabase, DatabaseRef},
        AccountInfo, Address, Bytecode, B256,
    }, U256,
};
use reth_revm::revm::{primitives::ChainAddress, Database, SyncDatabaseRef};
use std::{
    cell::RefCell,
    collections::{hash_map::Entry, HashMap},
};

/// A container type that caches reads from an underlying [`DatabaseRef`].
///
/// This is intended to be used in conjunction with `revm::db::State`
/// during payload building which repeatedly accesses the same data.
///
/// # Example
///
/// ```
/// use reth_payload_builder::database::CachedReads;
/// use revm::db::{DatabaseRef, State};
///
/// fn build_payload<DB: DatabaseRef>(db: DB) {
///     let mut cached_reads = CachedReads::default();
///     let db_ref = cached_reads.as_db(db);
///     // this is `Database` and can be used to build a payload, it never writes to `CachedReads` or the underlying database, but all reads from the underlying database are cached in `CachedReads`.
///     // Subsequent payload build attempts can use cached reads and avoid hitting the underlying database.
///     let db = State::builder().with_database_ref(db_ref).build();
/// }
/// ```
#[derive(Debug, Clone, Default)]
pub struct CachedReads {
    accounts: HashMap<Address, CachedAccount>,
    contracts: HashMap<B256, Bytecode>,
    block_hashes: HashMap<u64, B256>,
}

// === impl CachedReads ===

impl CachedReads {
    /// Gets a [`DatabaseRef`] that will cache reads from the given database.
    pub fn as_db<DB>(&mut self, db: DB) -> CachedReadsDBRef<'_, DB> {
        CachedReadsDBRef { inner: RefCell::new(self.as_db_mut(db)) }
    }

    fn as_db_mut<DB>(&mut self, db: DB) -> CachedReadsDbMut<'_, DB> {
        CachedReadsDbMut { cached: self, db }
    }

    /// Inserts an account info into the cache.
    pub fn insert_account(
        &mut self,
        address: Address,
        info: AccountInfo,
        storage: HashMap<U256, U256>,
    ) {
        self.accounts.insert(address, CachedAccount { info: Some(info), storage });
    }
}

/// A [Database] that caches reads inside [`CachedReads`].
#[derive(Debug)]
pub struct CachedReadsDbMut<'a, DB> {
    /// The cache of reads.
    pub cached: &'a mut CachedReads,
    /// The underlying database.
    pub db: DB,
}

impl<'a, DB: DatabaseRef> Database for CachedReadsDbMut<'a, DB> {
    type Error = <DB as DatabaseRef>::Error;

    fn basic(&mut self, address: Address) -> Result<Option<AccountInfo>, Self::Error> {
        let basic = match self.cached.accounts.entry(address) {
            Entry::Occupied(entry) => entry.get().info.clone(),
            Entry::Vacant(entry) => {
                entry.insert(CachedAccount::new(self.db.basic_ref(address)?)).info.clone()
            }
        };
        Ok(basic)
    }

    fn code_by_hash(&mut self, code_hash: B256) -> Result<Bytecode, Self::Error> {
        let code = match self.cached.contracts.entry(code_hash) {
            Entry::Occupied(entry) => entry.get().clone(),
            Entry::Vacant(entry) => entry.insert(self.db.code_by_hash_ref(code_hash)?).clone(),
        };
        Ok(code)
    }

    fn storage(&mut self, address: Address, index: U256) -> Result<U256, Self::Error> {
        match self.cached.accounts.entry(address) {
            Entry::Occupied(mut acc_entry) => match acc_entry.get_mut().storage.entry(index) {
                Entry::Occupied(entry) => Ok(*entry.get()),
                Entry::Vacant(entry) => Ok(*entry.insert(self.db.storage_ref(address, index)?)),
            },
            Entry::Vacant(acc_entry) => {
                // acc needs to be loaded for us to access slots.
                let info = self.db.basic_ref(address)?;
                let (account, value) = if info.is_some() {
                    let value = self.db.storage_ref(address, index)?;
                    let mut account = CachedAccount::new(info);
                    account.storage.insert(index, value);
                    (account, value)
                } else {
                    (CachedAccount::new(info), U256::ZERO)
                };
                acc_entry.insert(account);
                Ok(value)
            }
        }
    }

    fn block_hash(&mut self, number: u64) -> Result<B256, Self::Error> {
        let code = match self.cached.block_hashes.entry(number) {
            Entry::Occupied(entry) => *entry.get(),
            Entry::Vacant(entry) => *entry.insert(self.db.block_hash_ref(number)?),
        };
        Ok(code)
    }
}

/// A [`DatabaseRef`] that caches reads inside [`CachedReads`].
///
/// This is intended to be used as the [`DatabaseRef`] for
/// `revm::db::State` for repeated payload build jobs.
#[derive(Debug)]
pub struct CachedReadsDBRef<'a, DB> {
    /// The inner cache reads db mut.
    pub inner: RefCell<CachedReadsDbMut<'a, DB>>,
}

impl<'a, DB: DatabaseRef> DatabaseRef for CachedReadsDBRef<'a, DB> {
    type Error = <DB as DatabaseRef>::Error;

    fn basic_ref(&self, address: Address) -> Result<Option<AccountInfo>, Self::Error> {
        self.inner.borrow_mut().basic(address)
    }

    fn code_by_hash_ref(&self, code_hash: B256) -> Result<Bytecode, Self::Error> {
        self.inner.borrow_mut().code_by_hash(code_hash)
    }

    fn storage_ref(&self, address: Address, index: U256) -> Result<U256, Self::Error> {
        self.inner.borrow_mut().storage(address, index)
    }

    fn block_hash_ref(&self, number: u64) -> Result<B256, Self::Error> {
        self.inner.borrow_mut().block_hash(number)
    }
}

impl From<SyncCachedReads> for CachedReads {
    fn from(sync: SyncCachedReads) -> Self {
        let accounts = sync.accounts.into_iter().map(|(k, v)| (k.1, v)).collect();
        let contracts = sync.contracts.into_iter().map(|(((a, b), v))| (b, v)).collect();
        let block_hashes = sync.block_hashes.into_iter().map(|(((a, b), v))| (b, v)).collect();
        CachedReads { accounts, contracts, block_hashes }
    }
}

pub fn to_sync_cached_reads(cache_reads: CachedReads, chain_id: u64) -> SyncCachedReads {
    let accouts = cache_reads
        .accounts
        .into_iter()
        .map(|(k, v)| (ChainAddress(chain_id, k), v))
        .collect();
    let contracts = cache_reads
        .contracts
        .into_iter()
        .map(|(k, v)| ((chain_id, k), v)).collect();
    let block_hashes = cache_reads
        .block_hashes
        .into_iter()
        .map(|(k, v)| ((chain_id, k), v))
        .collect();
    SyncCachedReads { accounts: accouts, contracts, block_hashes }
}

#[derive(Debug, Clone, Default)]
pub struct SyncCachedReads {
    accounts: HashMap<ChainAddress, CachedAccount>,
    contracts: HashMap<(u64, B256), Bytecode>,
    block_hashes: HashMap<(u64, u64), B256>,
}

impl SyncCachedReads {
    pub fn as_db<DB>(&mut self, db: DB) -> SyncCachedReadsDBRef<'_, DB> {
        SyncCachedReadsDBRef { inner: RefCell::new(self.as_db_mut(db)) }
    }

    fn as_db_mut<DB>(&mut self, db: DB) -> SyncCachedReadsDbMut<'_, DB> {
        SyncCachedReadsDbMut { cached: self, db }
    }

    pub fn insert_account(
        &mut self,
        address: ChainAddress,
        info: AccountInfo,
        storage: HashMap<U256, U256>,
    ) {
        self.accounts.insert(address, CachedAccount { info: Some(info), storage });
    }
}

#[derive(Debug)]
pub struct SyncCachedReadsDbMut<'a, DB> {
    /// The cache of reads.
    pub cached: &'a mut SyncCachedReads,
    /// The underlying database.
    pub db: DB,
}

impl<'a, DB: SyncDatabaseRef> SyncDatabase for SyncCachedReadsDbMut<'a, DB> {
    type Error = <DB as SyncDatabaseRef>::Error;

    fn basic(&mut self,address: ChainAddress) -> Result<Option<AccountInfo>, Self::Error>  {
        let basic = match self.cached.accounts.entry(address) {
            Entry::Occupied(entry) => entry.get().info.clone(),
            Entry::Vacant(entry) => {
                entry.insert(CachedAccount::new(self.db.basic_ref(address)?)).info.clone()
            }
        };
        Ok(basic)
    }

    fn code_by_hash(&mut self,chain_id: u64, code_hash: B256) -> Result<Bytecode, Self::Error>  {
        let code = match self.cached.contracts.entry((chain_id, code_hash)) {
            Entry::Occupied(entry) => entry.get().clone(),
            Entry::Vacant(entry) => entry.insert(self.db.code_by_hash_ref(chain_id, code_hash)?).clone(),
        };
        Ok(code)
    }

    fn storage(&mut self,address: ChainAddress, index: U256) -> Result<U256, Self::Error>  {
        match self.cached.accounts.entry(address) {
            Entry::Occupied(mut acc_entry) => match acc_entry.get_mut().storage.entry(index) {
                Entry::Occupied(entry) => Ok(*entry.get()),
                Entry::Vacant(entry) => Ok(*entry.insert(self.db.storage_ref(address, index)?)),
            },
            Entry::Vacant(acc_entry) => {
                // acc needs to be loaded for us to access slots.
                let info = self.db.basic_ref(address)?;
                let (account, value) = if info.is_some() {
                    let value = self.db.storage_ref(address, index)?;
                    let mut account = CachedAccount::new(info);
                    account.storage.insert(index, value);
                    (account, value)
                } else {
                    (CachedAccount::new(info), U256::ZERO)
                };
                acc_entry.insert(account);
                Ok(value)
            }
        }
    }

    fn block_hash(&mut self,chain_id: u64, number: u64) -> Result<B256, Self::Error>  {
                let code = match self.cached.block_hashes.entry((chain_id, number)) {
            Entry::Occupied(entry) => *entry.get(),
            Entry::Vacant(entry) => *entry.insert(self.db.block_hash_ref(chain_id, number)?),
        };
        Ok(code)
    }
}


#[derive(Debug)]
pub struct SyncCachedReadsDBRef<'a, DB> {
    /// The inner cache reads db mut.
    pub inner: RefCell<SyncCachedReadsDbMut<'a, DB>>,
}

impl<'a, DB: SyncDatabaseRef> SyncDatabaseRef for SyncCachedReadsDBRef<'a, DB> {
    type Error = <DB as SyncDatabaseRef>::Error;

    fn basic_ref(&self,address: ChainAddress) -> Result<Option<AccountInfo> ,Self::Error>  {
        self.inner.borrow_mut().basic(address)
    }

    fn code_by_hash_ref(&self,chain_id: u64, code_hash: B256) -> Result<Bytecode,Self::Error>  {
        self.inner.borrow_mut().code_by_hash(chain_id, code_hash)
    }

    fn storage_ref(&self,address: ChainAddress, index: U256) -> Result<U256,Self::Error>  {
        self.inner.borrow_mut().storage(address, index)
    }

    fn block_hash_ref(&self,chain_id: u64, number: u64) -> Result<B256,Self::Error>  {
        self.inner.borrow_mut().block_hash(chain_id, number)
    }
}


#[derive(Debug, Clone)]
struct CachedAccount {
    info: Option<AccountInfo>,
    storage: HashMap<U256, U256>,
}

impl CachedAccount {
    fn new(info: Option<AccountInfo>) -> Self {
        Self { info, storage: HashMap::new() }
    }
}
