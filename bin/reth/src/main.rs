#![allow(missing_docs)]

// We use jemalloc for performance reasons.
#[cfg(all(feature = "jemalloc", unix))]
#[global_allocator]
static ALLOC: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;

/*#[cfg(all(feature = "optimism", not(test)))]
compile_error!("Cannot build the `reth` binary with the `optimism` feature flag enabled. Did you mean to build `op-reth`?");

#[cfg(not(feature = "optimism"))]
fn main() {
    use reth::cli::Cli;
    use reth_node_ethereum::EthereumNode;

    reth_cli_util::sigsegv_handler::install();

    // Enable backtraces unless a RUST_BACKTRACE value has already been explicitly provided.
    if std::env::var_os("RUST_BACKTRACE").is_none() {
        std::env::set_var("RUST_BACKTRACE", "1");
    }

    if let Err(err) = Cli::parse_args().run(|builder, _| async {
        let handle = builder.launch_node(EthereumNode::default()).await?;
        handle.node_exit_future.await
    }) {
        eprintln!("Error: {err:?}");
        std::process::exit(1);
    }
}*/

use alloy_sol_types::{sol, SolEventInterface, SolInterface};
use db::Database;
use execution::execute_block;
use eyre::Error;
use network::NetworkTestContext;
use node::NodeTestContext;
use once_cell::sync::Lazy;
use payload::PayloadTestContext;
use reth::args::{DiscoveryArgs, NetworkArgs, RpcServerArgs};
use reth_chainspec::{ChainSpec, ChainSpecBuilder, MAINNET};
use reth_consensus::Consensus;
use reth_db::{test_utils::TempDatabase, DatabaseEnv};
use reth_execution_types::Chain;
use reth_exex::{ExExContext, ExExEvent};
use reth_node_api::{FullNodeTypesAdapter, NodeAddOns};
//use reth_node_api::{EngineTypes, FullNodeComponents, NodeAddOns};
use reth_node_builder::{components::Components, rpc::EthApiBuilderProvider, AddOns, FullNode, Node, NodeAdapter, NodeBuilder, NodeComponentsBuilder, NodeConfig, NodeHandle, RethFullAdapter};
use reth_node_ethereum::{node::EthereumAddOns, EthEvmConfig, EthExecutorProvider, EthereumNode};
use reth_primitives::{address, alloy_primitives, Address, Genesis, SealedBlockWithSenders, TransactionSigned, B256, SealedBlock, transaction::WithEncoded, BlockBody};
use reth_provider::{providers::BlockchainProvider, CanonStateSubscriptions};
use reth_rpc_api::{eth::{helpers::AddDevSigners, FullEthApiServer}, EngineApiClient};
use reth_tasks::TaskManager;
use reth_tracing::tracing::{error, info};
use reth_transaction_pool::{blobstore::DiskFileBlobStore, CoinbaseTipOrdering, EthPooledTransaction, EthTransactionValidator, Pool, TransactionValidationTaskExecutor};
use rpc::RpcTestContext;
use rusqlite::Connection;
use transaction::TransactionTestContext;
use wallet::Wallet;
use std::{future::Future, marker::PhantomData, pin::Pin, sync::Arc};
use crate::execution::decode_transactions;

//use alloy_primitives::{Address, B256};
use reth::rpc::types::engine::PayloadAttributes;
//use reth_e2e_test_utils::NodeHelperType;
//use reth_node_ethereum::{node::EthereumAddOns, EthereumNode};
use reth_payload_builder::{EthBuiltPayload, EthPayloadBuilderAttributes};

mod db;
mod execution;
mod network;
mod payload;
mod rpc;
mod node;
mod transaction;
mod wallet;
mod engine_api;
mod traits;


/// Ethereum Node Helper type
//pub(crate) type EthNode = NodeHelperType<EthereumNode, EthereumAddOns>;

/// Helper function to create a new eth payload attributes
pub(crate) fn eth_payload_attributes(timestamp: u64) -> EthPayloadBuilderAttributes {
    let attributes = PayloadAttributes {
        timestamp,
        prev_randao: B256::ZERO,
        suggested_fee_recipient: Address::ZERO,
        withdrawals: Some(vec![]),
        parent_beacon_block_root: Some(B256::ZERO),
    };
    EthPayloadBuilderAttributes::new(B256::ZERO, attributes)
}


sol!(RollupContract, "TaikoL1.json");
use RollupContract::{BlockProposed, RollupContractCalls, RollupContractEvents};

const DATABASE_PATH: &str = "rollup.db";
const ROLLUP_CONTRACT_ADDRESS: Address = address!("9fCF7D13d10dEdF17d0f24C62f0cf4ED462f65b7");
const ROLLUP_SUBMITTER_ADDRESS: Address = address!("8943545177806ED17B9F23F0a21ee5948eCaa776");
const CHAIN_ID: u64 = 167010;
static CHAIN_SPEC: Lazy<Arc<ChainSpec>> = Lazy::new(|| {
    Arc::new(
        ChainSpecBuilder::default()
            .chain(CHAIN_ID.into())
            .genesis(Genesis::clique_genesis(CHAIN_ID, ROLLUP_SUBMITTER_ADDRESS))
            .shanghai_activated()
            .build(),
    )
});

fn print_block_data(block: &SealedBlock) {
    println!("Printing Block Data:");
    println!("Header:");
    println!("  Parent Hash: {:?}", block.header.parent_hash);
    println!("  Ommers Hash: {:?}", block.header.ommers_hash);
    println!("  Beneficiary: {:?}", block.header.beneficiary);
    println!("  State Root: {:?}", block.header.state_root);
    println!("  Transactions Root: {:?}", block.header.transactions_root);
    println!("  Receipts Root: {:?}", block.header.receipts_root);
    println!("  Logs Bloom: {:?}", block.header.logs_bloom);
    println!("  Difficulty: {:?}", block.header.difficulty);
    println!("  Number: {:?}", block.header.number);
    println!("  Gas Limit: {:?}", block.header.gas_limit);
    println!("  Gas Used: {:?}", block.header.gas_used);
    println!("  Timestamp: {:?}", block.header.timestamp);
    println!("  Extra Data: {:?}", block.header.extra_data);
    println!("  Mix Hash: {:?}", block.header.mix_hash);
    println!("  Nonce: {:?}", block.header.nonce);
    println!("  Base Fee Per Gas: {:?}", block.header.base_fee_per_gas);
    println!("  Withdrawals Root: {:?}", block.header.withdrawals_root);
    println!("  Blob Gas Used: {:?}", block.header.blob_gas_used);
    println!("  Excess Blob Gas: {:?}", block.header.excess_blob_gas);
    println!("  Parent Beacon Block Root: {:?}", block.header.parent_beacon_block_root);

    println!("Body:");
    println!("  Number of Transactions: {}", block.body.len());
    for (i, tx) in block.body.iter().enumerate() {
        println!("  Transaction {}:", i);
        println!("    Hash: {:?}", tx.hash());
        println!("    Nonce: {:?}", tx.nonce());
        // Add more transaction fields as needed
    }

    println!("Ommers:");
    println!("  Number of Ommers: {}", block.ommers.len());
    for (i, ommer) in block.ommers.iter().enumerate() {
        println!("  Ommer {}:", i);
        println!("    Hash: {:?}", ommer.ommers_hash);
        println!("    Number: {:?}", ommer.number);
        // Add more ommer fields as needed
    }

    println!("Withdrawals:");
    if let Some(withdrawals) = &block.withdrawals {
        println!("  Number of Withdrawals: {}", withdrawals.len());
        for (i, withdrawal) in withdrawals.iter().enumerate() {
            println!("  Withdrawal {}:", i);
            println!("    Index: {:?}", withdrawal.index);
            println!("    Validator Index: {:?}", withdrawal.validator_index);
            println!("    Address: {:?}", withdrawal.address);
            println!("    Amount: {:?}", withdrawal.amount);
        }
    } else {
        println!("  No withdrawals");
    }

    println!("Requests:");
    if let Some(requests) = &block.requests {
        println!("  Number of Requests: {}", requests.0.len());
        // Add more details about requests if needed
    } else {
        println!("  No requests");
    }
}

// Modify the TXN list
fn modify_payload_block(block: &mut SealedBlock, new_transactions: Vec<(TransactionSigned, Address)>) -> SealedBlock {
    // Unseal the header
    let mut header = block.header.clone().unseal();

    // Create a new body with the new transactions
    let txns: Vec<TransactionSigned> = new_transactions.into_iter().map(|(tx, _)| tx).collect();

    // Update other relevant header fields
    header.gas_used = txns.iter().map(|tx| tx.gas_limit()).sum();

    let body = BlockBody {
        transactions: txns,
        ommers: block.ommers.clone(),
        withdrawals: None,
        requests: None,
    };
    
    // Recalculate the block hash
    // Create a new sealed header
    let new_sealed_header = header.seal_slow();

    // Create and return a new sealed block
    SealedBlock::new(new_sealed_header, body)
}

struct Rollup<Node: reth_node_api::FullNodeComponents> {
    ctx: ExExContext<Node>,
    node: TestNodeContext,
}

impl<Node: reth_node_api::FullNodeComponents> Rollup<Node> {
    fn new(ctx: ExExContext<Node>, node: TestNodeContext) -> eyre::Result<Self> {
        Ok(Self { ctx, node })
    }

    async fn start(mut self) -> eyre::Result<()> {
        // Process all new chain state notifications
        while let Some(notification) = self.ctx.notifications.recv().await {
            if let Some(reverted_chain) = notification.reverted_chain() {
                self.revert(&reverted_chain)?;
            }

            if let Some(committed_chain) = notification.committed_chain() {
                self.commit(&committed_chain).await?;
                self.ctx.events.send(ExExEvent::FinishedHeight(committed_chain.tip().number))?;
            }
        }

        Ok(())
    }

    /// Process a new chain commit.
    ///
    /// This function decodes all transactions to the rollup contract into events, executes the
    /// corresponding actions and inserts the results into the database.
    async fn commit(&mut self, chain: &Chain) -> eyre::Result<()> {
        let events = decode_chain_into_rollup_events(chain);
        println!("Found {:?} events", events.len());


        // let (mut nodes, _tasks, _wallet) = setup::<EthereumNode>(
        //     1,
        //     Arc::new(
        //         ChainSpecBuilder::default()
        //             .chain(MAINNET.chain)
        //             .genesis(serde_json::from_str(include_str!("../../../crates/ethereum/node/tests/assets/genesis.json")).unwrap())
        //             .cancun_activated()
        //             .build(),
        //     ),
        //     false,
        // )
        // .await?;
    
        // let node = nodes.pop().unwrap();

        for (_, tx, event) in events {
            match event {
                // A new block is submitted to the rollup contract.
                // The block is executed on top of existing rollup state and committed into the
                // database.
                RollupContractEvents::BlockProposed(BlockProposed {
                    blockId: block_number,
                    meta: block_metadata,
                    txList: tx_list,
                }) => {
                    println!("block_number: {:?}", block_number);
                    println!("tx_list: {:?}", tx_list);
                    let _call = RollupContractCalls::abi_decode(tx.input(), true)?;

                    let wallet = Wallet::default();
                    let raw_tx = TransactionTestContext::transfer_tx_bytes(1, wallet.inner).await;
                
                    // make the node advance
                    //let tx_hash = self.node.rpc.inject_tx(raw_tx).await?;


                    println!("tx_hash start");
                    let copy_tx_list_bytes: reth_primitives::Bytes = tx_list.clone();

                    let tx_hash = tokio::task::block_in_place(|| {
                        tokio::runtime::Handle::current().block_on({
                            self.node.rpc.inject_tx(raw_tx)
                        })
                    }).unwrap();

                    println!("tx_hash start");
                
                    // make the node advance
                    //let (payload, _): (EthBuiltPayload, _) = self.node.advance_block(vec![], eth_payload_attributes).await?;
                
                    // let block_hash = payload.block().hash();
                    // let block_number = payload.block().number;

                    // println!("L2 block number: {}", block_number);

                    //let tx_hash = B256::default();
                    //let block_hash = B256::default();
                    //let block_number = 10;
                
                    // // assert the block has been committed to the blockchain
                    //self.node.assert_new_block(tx_hash, block_hash, block_number).await?;

                    println!("payload start");

                    let (payload, _): (EthBuiltPayload, _) = tokio::task::block_in_place(|| {
                        tokio::runtime::Handle::current().block_on({
                            self.node.advance_block(vec![], eth_payload_attributes)
                        })
                    }).unwrap();

                    println!("payload end");

                    let block_hash = payload.block().hash();
                    let block_number = payload.block().number;

                    let mut payload_clone = payload.clone();
                    let block = payload_clone.mut_block(); 
                    println!("Printing payload data start");
                    print_block_data(block);
                    // println!("block_hash: {:?}", block_hash);
                    // println!("block_number: {:?}", block_number);

                    println!("Printing payload data end");

                    let decoded_l2_transaction: Vec<(TransactionSigned, Address)> = decode_transactions(
                        self.ctx.pool(),
                        tx,
                        tx_list
                    ).await?;

                    println!("Modify payload block");
                    let new_block = modify_payload_block(block, decoded_l2_transaction);
                    
                    println!("Modify payload blockend");

                    println!("Printing payload data again");
                    // println!("block_hash: {:?}", block_hash);
                    // println!("block_number: {:?}", block_number);

                    println!("Printing payload data end");

                    let res = tokio::task::block_in_place(|| {
                        tokio::runtime::Handle::current().block_on({
                            // do something async
                            println!("assert_new_block");
                            self.node.assert_new_block(tx_hash, block_hash, block_number)
                        })
                    });

                    println!("assert_new_block done: {:?}", res);

                    //let tx_hash = self.node.rpc.inject_tx(raw_tx).await?;

                    println!("{:?}", self.node.inner.evm_config);

                    /*if let RollupContractCalls::submitBlock(RollupContract::submitBlockCall {
                        header,
                        blockData,
                        ..
                    }) = call
                    {*/
                        match execute_block(
                            //&mut self.db,
                            self.ctx.pool(),
                            tx,
                            &block_metadata,
                            copy_tx_list_bytes,
                            //blockDataHash,
                        )
                        .await
                        {
                            Ok((block/*, bundle*/, _, _)) => {
                                let block = block.seal_slow();
                                //self.db.insert_block_with_bundle(&block, bundle)?;
                                info!(
                                    tx_hash = %tx.recalculate_hash(),
                                    chain_id = %CHAIN_ID,
                                    sequence = %block_metadata.l2BlockNumber,
                                    transactions = block.body.len(),
                                    "Block submitted, executed and inserted into database"
                                );
                            }
                            Err(err) => {
                                error!(
                                    %err,
                                    tx_hash = %tx.recalculate_hash(),
                                    chain_id = %CHAIN_ID,
                                    sequence = %block_metadata.l2BlockNumber,
                                    "Failed to execute block"
                                );
                            }
                        }
                    //}
                }
                // A deposit of ETH to the rollup contract. The deposit is added to the recipient's
                // balance and committed into the database.
                /*RollupContractEvents::Enter(RollupContract::Enter {
                    rollupChainId,
                    token,
                    rollupRecipient,
                    amount,
                }) => {
                    if rollupChainId != U256::from(CHAIN_ID) {
                        error!(tx_hash = %tx.recalculate_hash(), "Invalid rollup chain ID");
                        continue;
                    }
                    if token != Address::ZERO {
                        error!(tx_hash = %tx.recalculate_hash(), "Only ETH deposits are supported");
                        continue;
                    }

                    self.db.upsert_account(rollupRecipient, |account| {
                        let mut account = account.unwrap_or_default();
                        account.balance += amount;
                        Ok(account)
                    })?;

                    info!(
                        tx_hash = %tx.recalculate_hash(),
                        %amount,
                        recipient = %rollupRecipient,
                        "Deposit",
                    );
                }*/
                _ => (),
            }
        }

        Ok(())
    }

    /// Process a chain revert.
    ///
    /// This function decodes all transactions to the rollup contract into events, reverts the
    /// corresponding actions and updates the database.
    fn revert(&mut self, chain: &Chain) -> eyre::Result<()> {
        let mut events = decode_chain_into_rollup_events(chain);
        // Reverse the order of events to start reverting from the tip
        events.reverse();

        /*for (_, tx, event) in events {
            match event {
                // The block is reverted from the database.
                RollupContractEvents::BlockSubmitted(_) => {
                    let call = RollupContractCalls::abi_decode(tx.input(), true)?;

                    if let RollupContractCalls::submitBlock(RollupContract::submitBlockCall {
                        header,
                        ..
                    }) = call
                    {
                        self.db.revert_tip_block(header.sequence)?;
                        info!(
                            tx_hash = %tx.recalculate_hash(),
                            chain_id = %header.rollupChainId,
                            sequence = %header.sequence,
                            "Block reverted"
                        );
                    }
                }
                // The deposit is subtracted from the recipient's balance.
                RollupContractEvents::Enter(RollupContract::Enter {
                    rollupChainId,
                    token,
                    rollupRecipient,
                    amount,
                }) => {
                    if rollupChainId != U256::from(CHAIN_ID) {
                        error!(tx_hash = %tx.recalculate_hash(), "Invalid rollup chain ID");
                        continue;
                    }
                    if token != Address::ZERO {
                        error!(tx_hash = %tx.recalculate_hash(), "Only ETH deposits are supported");
                        continue;
                    }

                    self.db.upsert_account(rollupRecipient, |account| {
                        let mut account = account.ok_or(eyre::eyre!("account not found"))?;
                        account.balance -= amount;
                        Ok(account)
                    })?;

                    info!(
                        tx_hash = %tx.recalculate_hash(),
                        %amount,
                        recipient = %rollupRecipient,
                        "Deposit reverted",
                    );
                }
                _ => (),
            }
        }*/

        Ok(())
    }
}

/// Decode chain of blocks into a flattened list of receipt logs, filter only transactions to the
/// Rollup contract [ROLLUP_CONTRACT_ADDRESS] and extract [RollupContractEvents].
fn decode_chain_into_rollup_events(
    chain: &Chain,
) -> Vec<(&SealedBlockWithSenders, &TransactionSigned, RollupContractEvents)> {
    chain
        // Get all blocks and receipts
        .blocks_and_receipts()
        // Get all receipts
        .flat_map(|(block, receipts)| {
            block
                .body
                .iter()
                .zip(receipts.iter().flatten())
                .map(move |(tx, receipt)| (block, tx, receipt))
        })
        // Get all logs from rollup contract
        .flat_map(|(block, tx, receipt)| {
            receipt
                .logs
                .iter()
                .filter(|log| { println!("log: {:?}", log); log.address == ROLLUP_CONTRACT_ADDRESS } )
                .map(move |log| (block, tx, log))
        })
        // Decode and filter rollup events
        .filter_map(|(block, tx, log)| {
            RollupContractEvents::decode_raw_log(log.topics(), &log.data.data, true)
                .ok()
                .map(|event| (block, tx, event))
        })
        .collect()
}

// Type aliases

type TmpDB = Arc<TempDatabase<DatabaseEnv>>;
type TmpNodeAdapter<N> = FullNodeTypesAdapter<N, TmpDB, BlockchainProvider<TmpDB>>;

type Adapter<N> = NodeAdapter<
    RethFullAdapter<TmpDB, N>,
    <<N as Node<TmpNodeAdapter<N>>>::ComponentsBuilder as NodeComponentsBuilder<
        RethFullAdapter<TmpDB, N>,
    >>::Components,
>;

type TestNodeContext = NodeTestContext<NodeAdapter<FullNodeTypesAdapter<EthereumNode, Arc<TempDatabase<DatabaseEnv>>, BlockchainProvider<Arc<TempDatabase<DatabaseEnv>>>>, Components<FullNodeTypesAdapter<EthereumNode, Arc<TempDatabase<DatabaseEnv>>, BlockchainProvider<Arc<TempDatabase<DatabaseEnv>>>>, Pool<TransactionValidationTaskExecutor<EthTransactionValidator<BlockchainProvider<Arc<TempDatabase<DatabaseEnv>>>, EthPooledTransaction>>, CoinbaseTipOrdering<EthPooledTransaction>, DiskFileBlobStore>, EthEvmConfig, EthExecutorProvider, Arc<dyn Consensus>>>, EthereumAddOns>;

/// Type alias for a type of NodeHelper
pub type NodeHelperType<N, AO> = NodeTestContext<Adapter<N>, AO>;

/// Creates the initial setup with `num_nodes` started and interconnected.
pub async fn setup<N>(
    num_nodes: usize,
    chain_spec: Arc<ChainSpec>,
    is_dev: bool,
) -> eyre::Result<(Vec<NodeHelperType<N, N::AddOns>>, TaskManager, Wallet)>
where
    N: Default + Node<TmpNodeAdapter<N>>,
    <N::AddOns as NodeAddOns<Adapter<N>>>::EthApi:
        FullEthApiServer + AddDevSigners + EthApiBuilderProvider<Adapter<N>>,
{
    let tasks = TaskManager::current();
    let exec = tasks.executor();

    let network_config = NetworkArgs {
        discovery: DiscoveryArgs { disable_discovery: true, ..DiscoveryArgs::default() },
        ..NetworkArgs::default()
    };

    // Create nodes and peer them
    let mut nodes: Vec<NodeTestContext<_, _>> = Vec::with_capacity(num_nodes);

    for idx in 0..num_nodes {
        let node_config = NodeConfig::test()
            .with_chain(chain_spec.clone())
            .with_network(network_config.clone())
            .with_unused_ports()
            .with_rpc(RpcServerArgs::default().with_unused_ports().with_http())
            .set_dev(is_dev);

        let NodeHandle { node, node_exit_future: _ } = NodeBuilder::new(node_config.clone())
            .testing_node(exec.clone())
            .node(Default::default())
            .launch()
            .await?;

        //node.state_by_block_id(block_id)

        let mut node = NodeTestContext::new(node).await?;

        // Connect each node in a chain.
        if let Some(previous_node) = nodes.last_mut() {
            previous_node.connect(&mut node).await;
        }

        // Connect last node with the first if there are more than two
        if idx + 1 == num_nodes && num_nodes > 2 {
            if let Some(first_node) = nodes.first_mut() {
                node.connect(first_node).await;
            }
        }

        nodes.push(node);
    }

    Ok((nodes, tasks, Wallet::default().with_chain_id(chain_spec.chain().into())))
}

fn main() -> eyre::Result<()> {
    println!("Brecht");
    reth::cli::Cli::parse_args().run(|builder, _| async move {

        // let (mut nodes, _tasks, _wallet) = setup::<EthereumNode>(
        //     1,
        //     Arc::new(
        //         ChainSpecBuilder::default()
        //             .chain(MAINNET.chain)
        //             .genesis(serde_json::from_str(include_str!("../../../crates/ethereum/node/tests/assets/genesis.json")).unwrap())
        //             .cancun_activated()
        //             .build(),
        //     ),
        //     false,
        // )
        // .await?;
    
        // let node = nodes.pop().unwrap();

    let tasks = TaskManager::current();
    let exec = tasks.executor();

    let network_config = NetworkArgs {
        discovery: DiscoveryArgs { disable_discovery: true, ..DiscoveryArgs::default() },
        ..NetworkArgs::default()
    };

    let chain_spec = ChainSpecBuilder::default()
             .chain(MAINNET.chain)
             .genesis(serde_json::from_str(include_str!("../../../crates/ethereum/node/tests/assets/genesis.json")).unwrap())
             .cancun_activated()
             .build();

    let node_config = NodeConfig::test()
        .with_chain(chain_spec.clone())
        .with_network(network_config.clone())
        .with_unused_ports()
        .with_rpc(RpcServerArgs::default().with_unused_ports().with_http())
        .set_dev(false);

    let NodeHandle { node, node_exit_future: _ } = NodeBuilder::new(node_config.clone())
        .testing_node(exec.clone())
        .node(Default::default())
        .launch()
        .await?;

    //node.state_by_block_id(block_id)

    let node = NodeTestContext::new(node).await?;

        let handle = builder
            .node(EthereumNode::default())
            .install_exex("Rollup", move |ctx| async {
                //let connection = Connection::open(DATABASE_PATH)?;

                // let network_config = NetworkArgs {
                //     discovery: DiscoveryArgs { disable_discovery: true, ..DiscoveryArgs::default() },
                //     ..NetworkArgs::default()
                // };

                // //let tasks = TaskManager::current();
                // let exec = tasks.executor();

                // let node_config = NodeConfig::test()
                //     .with_chain(CHAIN_SPEC.clone())
                //     .with_network(network_config.clone())
                //     .with_unused_ports()
                //     .with_rpc(RpcServerArgs::default().with_unused_ports().with_http())
                //     .set_dev(true);

                // let node_handle: = NodeBuilder::new(node_config.clone())
                //     .testing_node(exec.clone())
                //     .node(Default::default())
                //     .launch()
                //     .await?;

                // let mut node = NodeTestContext::new(node_handle.node).await?;
            
                

                //Ok((nodes, tasks, Wallet::default().with_chain_id(chain_spec.chain().into())))

                // let wallet = Wallet::default();
                // let raw_tx = TransactionTestContext::transfer_tx_bytes(1, wallet.inner).await;
            
                // // make the node advance
                // let tx_hash = node.rpc.inject_tx(raw_tx).await?;
            
                // // make the node advance
                // let (payload, _): (EthBuiltPayload, _) = node.advance_block(vec![], eth_payload_attributes).await?;
            
                // let block_hash = payload.block().hash();
                // let block_number = payload.block().number;
            
                // // assert the block has been committed to the blockchain
                // node.assert_new_block(tx_hash, block_hash, block_number).await?;
            

                // let wallet = Wallet::default();
                // let raw_tx = TransactionTestContext::transfer_tx_bytes(1, wallet.inner).await;
            
                // // make the node advance
                // let tx_hash = node.rpc.inject_tx(raw_tx).await?;
            
                // // make the node advance
                // let (payload, _) = node.advance_block(vec![], eth_payload_attributes).await?;
            
                // let block_hash = payload.block().hash();
                // let block_number = payload.block().number;
            
                // // assert the block has been committed to the blockchain
                // node.assert_new_block(tx_hash, block_hash, block_number).await?;

                // // setup payload for submission
                // let envelope_v3: <E as EngineTypes>::ExecutionPayloadV3 = payload.into();

                // // submit payload to engine api
                // let submission = EngineApiClient::<E>::new_payload_v3(
                //     &self.engine_api_client,
                //     envelope_v3.execution_payload(),
                //     versioned_hashes,
                //     payload_builder_attributes.parent_beacon_block_root().unwrap(),
                // )
                // .await?;
                
                //let f: Pin<Box<dyn Future<Output = Result<(), Error>> + Send>> =
                //        Box::pin(Rollup::new(ctx, connection, node)?.start());
                //f

                Ok(Rollup::new(ctx, node)?.start())
            })
            .launch()
            .await?;

        handle.wait_for_node_exit().await
    })
}
