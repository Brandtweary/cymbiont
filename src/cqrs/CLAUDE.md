# CQRS Module Guide 🎯

## Module Overview
Command Query Responsibility Segregation for sequential state management.

## Core Components

### File Structure
- **mod.rs**: Module exports
- **commands.rs**: Command enum definitions
- **queue.rs**: Public command submission API
- **processor.rs**: Command executor owning all state
- **router.rs**: Command routing with RouterToken

### Command Types
- **GraphCommand**: CreateBlock, UpdateBlock, DeleteBlock, CreatePage, DeletePage
- **AgentCommand**: AddMessage, ClearHistory, SetLLMConfig, SetSystemPrompt, SetDefaultGraph
- **RegistryCommand**: RegisterGraph, RemoveGraph, OpenGraph, CloseGraph
- **SystemCommand**: Shutdown

### Key Types
- **Command**: All possible mutations
- **CommandQueue**: Async command submission
- **CommandProcessor**: Sequential executor
- **RouterToken**: Zero-sized type created only in router.rs, enforcing CQRS routing
- **CommandLog**: Sled-based persistence

## API Methods

### CommandQueue
- `execute(command)` - Submit command and await response
- `shutdown()` - Graceful shutdown

### CommandProcessor
- `start()` - Initialize command processing
- `ensure_graph_loaded(id)` - Lazy load graph
- `ensure_agent_loaded(id)` - Lazy load agent

## Adding New Commands
1. Add variant to Command enum in commands.rs
2. Add handler in router.rs
3. Use RouterToken for authorized operations