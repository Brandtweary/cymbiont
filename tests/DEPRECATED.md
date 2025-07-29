# Integration Tests Deprecated

As of the Cymbiont 1.0 pivot, these integration tests are deprecated.

## Why?

The integration tests were primarily focused on testing the Logseq plugin integration:
- Graph switching via URL schemes
- WebSocket bidirectional sync
- Plugin initialization and lifecycle
- Multi-graph session management

Since we're pivoting to a terminal-first architecture without Logseq integration, these tests are no longer relevant.

## What's Next?

New tests will focus on:
- Unix pipe interface
- Agent-oriented JSON protocol
- Import functionality
- Pure knowledge graph operations

The unit tests in `src/` remain valid and continue to pass.