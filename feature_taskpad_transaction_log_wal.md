# Feature Taskpad: Transaction Log with Write-Ahead Logging (WAL)

## Feature Description
Implement a transaction log system with write-ahead logging to provide ACID guarantees for all knowledge graph operations. This will enable proper coordination between LLM-created content and PKM synchronization, prevent race conditions, support crash recovery, and provide a complete audit trail of all graph mutations. The transaction log will serve as the foundation for distributed coordination in Cymbiont's multi-source data architecture.

## Specifications
- Write-ahead log for durability and crash recovery using sled embedded database
- Transaction coordinator managing operation lifecycle (pending → committed → completed)
- Content hash-based correlation to prevent duplicate processing from real-time sync
- Saga pattern for multi-step workflows (individual transactions with coordination)
- Configurable retention policy for log compaction
- Recovery mechanism on startup with automatic retry of incomplete transactions
- "Combine and keep" merge strategy for conflicts (concatenate content, union properties)
- Integration points in graph_manager, websocket, and api modules
- Performance target: <5ms overhead per transaction
- Storage format: sled's ACID-compliant embedded database

## Relevant Components

### Graph Manager
- `src/graph_manager.rs`: Current persistence layer
- Key methods: `save_graph()`, `load_graph()`, `create_or_update_node_from_pkm_*`
- Current usage: JSON-based full graph serialization, no incremental updates

### AppState
- `src/main.rs`: Application state management
- Current pattern: Arc<AppState> with Mutex/RwLock fields
- Need to add: Transaction coordinator, WAL writer, pending operations map

### WebSocket Module
- `src/websocket.rs`: Command broadcast system
- Key functions: `broadcast_command()`, command handlers
- Need to add: Acknowledgment channels, correlation ID generation

### Real-time Sync
- `logseq_plugin/index.js`: `handleDBChanges()` function
- Current behavior: Processes all incoming changes
- Need to add: Transaction awareness, skip pending operations

### Serialization Infrastructure
- Throughout: Serde-based JSON serialization
- `src/pkm_data.rs`: Core data structures
- Can reuse: Existing serialization traits

## Development Plan

### 1. WAL Foundation (using sled)
- [x] Add sled dependency to Cargo.toml
- [x] Create `src/transaction_log.rs` module wrapping sled operations
- [x] Configure sled for our use case:
  - [x] Set flush_every_ms(100) for frequent durability
  - [x] ~~Configure snapshot_after_ops for regular checkpoints~~ (deprecated in sled)
  - [x] Set appropriate cache_capacity
- [x] Implement transaction log operations:
  - [x] `append_transaction(tx: Transaction) -> Result<TransactionId>`
  - [x] `get_transaction(id: TransactionId) -> Result<Transaction>`
  - [x] `list_pending_transactions() -> Result<Vec<Transaction>>`
  - [x] `update_transaction_state(id: TransactionId, state: TransactionState) -> Result<()>`
- [x] Add configuration for sled directory and retention
- [x] Write unit tests for transaction operations
- [ ] Test crash recovery scenarios with kill -9

### 2. Transaction Model
- [x] Create `src/transaction.rs` module
- [x] Define transaction structures:
  ```rust
  enum TransactionState { Active, WaitingForAck, Committed, Aborted }
  struct Transaction { id, operation, state, created_at, content_hash }
  enum Operation { CreateNode, UpdateNode, DeleteNode, SendWebSocket, ReceivedAck }
  ```
- [x] Create `src/saga.rs` module for multi-step workflows:
  ```rust
  struct Saga { id, transactions: Vec<TransactionId>, state: SagaState }
  enum SagaState { InProgress, Completed, Failed, Compensating }
  ```
- [x] Implement transaction state machine
- [x] Create transaction coordinator:
  - [x] `begin_transaction(op: Operation) -> TransactionId`
  - [x] `commit_transaction(tx_id) -> Result<()>`
  - [x] `abort_transaction(tx_id, reason) -> Result<()>`
  - [x] `begin_saga() -> SagaId`
  - [x] `add_transaction_to_saga(saga_id, tx_id) -> Result<()>`
- [x] Add transaction context to graph operations (via kg_api module)
- [x] Implement saga compensation logic for rollbacks

### 3. Integration with Graph Manager
- [x] Created `kg_api.rs` module as public API layer
- [x] Wrap graph operations in transaction boundaries
- [x] Log operations to WAL before applying to graph
- [x] Complete the create block workflow:
  - [x] Phase 1: Log intent, apply to graph with temp ID
  - [x] Phase 2: Wait for WebSocket acknowledgment with UUID
  - [x] Phase 3: Update node PKM ID from temp to real UUID
- [x] Add single-node archive method to graph_manager (archive_nodes handles both single and batch)
- [x] Update save/load to be transaction-aware
- [x] Add recovery logic to replay uncommitted transactions (in main.rs startup)

### 4. WebSocket Acknowledgment System
- [x] Modify Command enum to include correlation IDs
- [x] Update command handlers in plugin to send acknowledgments
- [x] Implement acknowledgment flow in kg_api
- [ ] Add timeout handling for missing acknowledgments
- [x] Create acknowledgment response types:
  - [x] BlockCreated with Logseq UUID
  - [x] BlockUpdated/Deleted/PageCreated with success/error
  - [ ] Timeout indication

### 5. Correlation and Deduplication
- [x] Add pending operations tracking to TransactionCoordinator
- [x] Implement content hash checking in kg_api
- [x] Create operation matching logic:
  - [x] Compute content hash for all mutations
  - [x] Check pending transactions for matching hash
  - [x] Correlate UUID with transaction after acknowledgment (for create_block)
- [x] Add to real-time sync handler in api.rs
- [ ] Add correlation tracking for update operations
- [ ] Add correlation tracking for delete operations
- [ ] Add correlation tracking for page operations
- [ ] Add metrics for deduplication effectiveness

### 6. Recovery and Startup
- [x] Implement WAL replay on startup (basic recovery in main.rs)
- [x] Handle different transaction states:
  - [x] Active → Keep for retry
  - [x] WaitingForAck → Timeout after 30s
  - [x] Committed → Already done
- [x] Add startup logging for recovery actions
- [ ] Test recovery with various failure scenarios

### 7. Configuration and Management
- [ ] Add transaction log settings to config.yaml
- [ ] Implement log rotation and compaction
- [ ] Create CLI commands for log inspection
- [ ] Add metrics and health checks
- [ ] Document operational procedures

### 8. Testing and Validation
- [x] Unit tests for each module (transaction_log, transaction, saga all have tests)
- [ ] Integration tests for full transaction flow
- [ ] Chaos testing: kill process mid-transaction
- [ ] Performance benchmarks
- [ ] Load testing with concurrent operations

### 9. Multi-Graph Support for Testing
- [ ] Add multi-graph support (research how Logseq identifies graphs, use that ID to map PKMs to separate knowledge graphs) #research
  - [ ] Create mapping system for PKM databases (only logseq supported currently) and knowledge graphs
  - [ ] Correlate archived nodes with their source graph (once multi-graph support is implemented, archived nodes should track which graph they came from)
  - [ ] Create full e2e integration test using dummy Logseq graph (single test: trigger full sync from dummy graph, verify all pages/blocks reached a separate test KG)
  - [ ] Add comprehensive integration tests for 3-tiered sync behavior (test real-time, incremental, and full sync scenarios - requires multi-graph support for clean test isolation)

## Development Notes

**Storage Choice**: Using `sled` embedded database - a reputable, production-ready crate with ACID transactions, MVCC, lock-free concurrent access, and built-in crash safety. This aligns with our "trust your baby's life with it" requirement for a personal knowledge graph agent.

**Correlation Strategy**: Content hash-based matching without time windows. When real-time sync events arrive, check if the content hash exists in any pending transaction. This definitively identifies echoes vs genuine new content.

**Transaction Model**: 
- Individual operations (create_block, update_block) are single transactions
- Multi-step workflows use Saga pattern: each step is a transaction with a saga coordinator tracking the overall workflow
- Example: LLM create → send to Logseq → update mapping = 3 transactions in 1 saga

**Conflict Resolution**: "Combine and keep" strategy - if concurrent updates occur, concatenate differing content with conflict markers and union all properties. This ensures no data loss and makes conflicts visible in the UI.

**Performance Considerations**: Sled is optimized for write-heavy workloads with sequential log writes. Graph operations remain in-memory. Configure frequent fsyncs (100ms) for durability.

**Recovery Philosophy**: Automatic retry with exponential backoff. The self-organizing system should "work itself out" without manual intervention. Failed operations after max retries go to a dead letter queue for debugging.

## Future Tasks
- Distributed transaction support for multi-instance Cymbiont
- Streaming replication to secondary instances
- Point-in-time recovery ("show me the graph as of timestamp X")
- Transaction history UI in a web interface
- Prometheus metrics export for monitoring
- Compression for old log segments
- Encryption at rest for log files
- Multi-version concurrency control (MVCC) for read consistency
- Saga pattern implementation for long-running workflows

## Final Implementation

### What Was Built

The transaction log with Write-Ahead Logging (WAL) has been successfully implemented using sled as the embedded database. The system provides ACID guarantees for all knowledge graph operations and enables proper coordination between LLM-created content and Logseq synchronization.

#### Core Components Implemented:

1. **Transaction Log Module (`src/transaction_log.rs`)**
   - Wraps sled embedded database for ACID operations
   - Manages three trees: transactions, content_hash_index, pending_index
   - Provides append, get, update, and query operations
   - Content hash indexing for deduplication

2. **Transaction Coordinator (`src/transaction.rs`)**
   - Manages transaction lifecycle (Active → WaitingForAck → Committed/Aborted)
   - Tracks pending operations by content hash
   - Handles acknowledgments and recovery
   - Automatic recovery on startup

3. **Saga Coordinator (`src/saga.rs`)**
   - Implements saga pattern for multi-step workflows
   - WorkflowSagas for create_block workflow
   - Handles compensation/rollback
   - Tracks saga state (InProgress → Completed/Failed)

4. **Knowledge Graph API (`src/kg_api.rs`)**
   - Public API for all graph operations
   - Transaction boundaries for all mutations
   - WebSocket sync integration
   - Correlation ID tracking for acknowledgments
   - Currently marked with `#![allow(dead_code)]` until aichat-agent integration

5. **WebSocket Acknowledgment System**
   - Added correlation_id to all mutation commands
   - Acknowledgment message types (BlockCreated, BlockUpdated, etc.)
   - Full bidirectional flow implemented
   - JavaScript plugin sends acknowledgments with UUIDs

6. **Backend Deduplication**
   - Content hash checking in api.rs real-time sync handler
   - Prevents processing of content already in transaction
   - TODO comment added for future client-side filtering

7. **Transaction-Aware Persistence**
   - Graph saves independently to knowledge_graph.json
   - Transactions persist in sled with ACID guarantees
   - Recovery happens automatically on startup
   - Version field added to graph for future compatibility

#### Key Features:

- **Race Condition Prevention**: Content hash deduplication prevents duplicate processing
- **Asynchronous Acknowledgments**: Correlation IDs track multi-step workflows
- **UUID Mapping**: Temp IDs are updated to real Logseq UUIDs when acknowledged
- **Crash Recovery**: Sled persists transactions, automatic retry on startup
- **Clean Console**: All code compiles without warnings, tests pass cleanly

#### What's Left:

- Timeout handling for missing acknowledgments
- Correlation tracking for update/delete/page operations (only create_block has full flow)
- Integration tests and performance benchmarks
- Configuration in config.yaml
- Multi-graph support for testing