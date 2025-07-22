# Feature Taskpad: Transaction Log with Write-Ahead Logging (WAL)

## Feature Description
Implement a transaction log system with write-ahead logging to provide ACID guarantees for all knowledge graph operations. This will enable proper coordination between LLM-created content and PKM synchronization, prevent race conditions, support crash recovery, and provide a complete audit trail of all graph mutations. The transaction log will serve as the foundation for distributed coordination in Cymbiont's multi-source data architecture.

## Specifications
- Write-ahead log for durability and crash recovery
- Transaction coordinator managing operation lifecycle (pending → committed → completed)
- Correlation ID system to prevent duplicate processing from real-time sync
- Support for multi-step atomic operations (create node → send to Logseq → update mapping)
- Configurable retention policy for log compaction
- Recovery mechanism on startup to replay incomplete transactions
- Integration points in graph_manager, websocket, and api modules
- Performance target: <5ms overhead per transaction
- Storage format: Binary log files with checksums for integrity

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

### 1. WAL Foundation
- [ ] Add WAL dependencies to Cargo.toml (likely `sled` for embedded DB or custom implementation)
- [ ] Create `src/wal.rs` module for write-ahead log operations
- [ ] Define log entry format (binary with version, checksum, payload)
- [ ] Implement core WAL operations:
  - [ ] `append_entry(entry: WalEntry) -> Result<LogSequence>`
  - [ ] `read_from(sequence: LogSequence) -> Result<Vec<WalEntry>>`
  - [ ] `truncate_before(sequence: LogSequence) -> Result<()>`
  - [ ] `sync() -> Result<()>` for fsync guarantee
- [ ] Add configuration for WAL directory and retention
- [ ] Write unit tests for WAL operations
- [ ] Test crash recovery scenarios

### 2. Transaction Model
- [ ] Create `src/transaction.rs` module
- [ ] Define transaction structures:
  ```rust
  enum TransactionState { Active, WaitingForAck, Committed, Aborted }
  struct Transaction { id, operations, state, created_at, correlation_id }
  enum Operation { CreateNode, UpdateNode, DeleteNode, SendWebSocket, ReceivedAck }
  ```
- [ ] Implement transaction state machine
- [ ] Create transaction coordinator:
  - [ ] `begin_transaction() -> TransactionId`
  - [ ] `add_operation(tx_id, operation) -> Result<()>`
  - [ ] `commit_transaction(tx_id) -> Result<()>`
  - [ ] `abort_transaction(tx_id, reason) -> Result<()>`
- [ ] Add transaction context to graph operations
- [ ] Implement operation rollback logic

### 3. Integration with Graph Manager
- [ ] Add transaction parameter to all mutation methods
- [ ] Wrap graph operations in transaction boundaries
- [ ] Log operations to WAL before applying to graph
- [ ] Implement two-phase commit:
  - [ ] Phase 1: Log intent, apply to graph
  - [ ] Phase 2: After acknowledgment, mark committed
- [ ] Update save/load to be transaction-aware
- [ ] Add recovery logic to replay uncommitted transactions

### 4. WebSocket Acknowledgment System
- [ ] Modify Command enum to include acknowledgment channels
- [ ] Update command handlers in plugin to send acknowledgments
- [ ] Implement correlation ID generation and tracking
- [ ] Add timeout handling for missing acknowledgments
- [ ] Create acknowledgment response types:
  - [ ] Success with Logseq UUID
  - [ ] Failure with error details
  - [ ] Timeout indication

### 5. Correlation and Deduplication
- [ ] Add pending operations tracking to AppState
- [ ] Implement correlation ID matching in real-time sync
- [ ] Create operation matching logic:
  - [ ] Content hash comparison
  - [ ] Timestamp window checking
  - [ ] UUID correlation after acknowledgment
- [ ] Add metrics for deduplication effectiveness

### 6. Recovery and Startup
- [ ] Implement WAL replay on startup
- [ ] Handle different transaction states:
  - [ ] Active → Abort (incomplete)
  - [ ] WaitingForAck → Retry or abort based on age
  - [ ] Committed → Ensure applied
- [ ] Add startup logging for recovery actions
- [ ] Test recovery with various failure scenarios

### 7. Configuration and Management
- [ ] Add transaction log settings to config.yaml
- [ ] Implement log rotation and compaction
- [ ] Create CLI commands for log inspection
- [ ] Add metrics and health checks
- [ ] Document operational procedures

### 8. Testing and Validation
- [ ] Unit tests for each module
- [ ] Integration tests for full transaction flow
- [ ] Chaos testing: kill process mid-transaction
- [ ] Performance benchmarks
- [ ] Load testing with concurrent operations

## Development Notes

**Storage Choice**: Evaluating between `sled` (embedded DB with transactions) vs custom binary format. Sled provides ACID guarantees out of the box but adds a dependency. Custom format gives full control but requires more implementation work.

**Correlation Strategy**: Using content hash + timestamp window for matching operations. This handles the case where we create a block, send to Logseq, and then see it come back via real-time sync before acknowledgment.

**Transaction Boundaries**: Each high-level operation (create_block, update_block) is one transaction. Future: support for multi-operation transactions for complex workflows.

**Performance Considerations**: WAL writes are sequential and can be batched. Graph operations remain in-memory. Only synchronous operation is WAL append, which should be fast on SSD.

**Recovery Philosophy**: Best effort - if a transaction can't be recovered, log it and continue. The system should be resilient to partial failures.

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
{To be completed when the feature is finished - will contain authoritative summary of what was built}