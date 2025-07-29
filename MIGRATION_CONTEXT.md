# Cymbiont Evolution Migration Context

## Session Restart Context (2025-01-29)

### What Just Happened - Post Mortem

**Issue**: Multiple parallel subagents caused terminal emulator (Zed) to become unstable/buggy. All subagents had to be cancelled mid-execution.

**Root Cause**: Running 6 parallel subagents simultaneously in the same workspace created resource contention and UI conflicts in the terminal emulator.

**Immediate Actions Taken**:
- User cancelled all running subagents
- Session restart requested
- Need to switch to worktree-based development approach

### Current Git State
- Branch: `main` 
- Last commit: `1785de3` - "Add cymbiont-skeleton as submodule for terminal-first evolution"
- Status: Clean git state after successful merge of `aichat-integration` branch
- cymbiont-skeleton added as submodule but may need to be removed

### Critical Context Preserved

#### 1. AIChat-Agent Integration is ACTIVE
The `feature_taskpad_aichat_agent_integration.md` contains the complete plan for integrating aichat-agent. This is NOT being abandoned - we're adapting the approach from browser-based to terminal-first.

**Location**: `docs/active_integration_plans/feature_taskpad_aichat_agent_integration.md`

#### 2. Evolution Strategy Clarification
- **Original Plan**: Use cymbiont-skeleton as separate evolution target
- **New Approach**: Dismantle and rebuild in place on a feature branch
- **Reason**: Avoid complexity of submodules, work in clean environment

#### 3. Multi-Graph Registry Insight
`graph_registry.rs` is NOT Logseq-specific - it's a generic multi-graph management system that should be preserved and adapted, not deprecated.

### Subagent Damage Report

**Completed Successfully**:
1. **Deprecation Agent**: Successfully added deprecation notices to Logseq-specific modules in original cymbiont

**Interrupted/Incomplete**:
2. **Core Extraction Agent**: Was copying graph_manager.rs → cymbiont-skeleton/src/core/
3. **Registry Refactor Agent**: Was adapting graph_registry.rs for generic use
4. **Import Framework Agent**: Was designing PKM import trait system
5. **CLI Interface Agent**: Was implementing stdin/stdout pipe protocol
6. **TUI Skeleton Agent**: Was creating ratatui-based terminal interface

### New Development Strategy

#### Worktree-Based Approach
- Main development stays in primary worktree
- Research/prototyping happens in separate worktrees
- Subagents work independently without resource conflicts
- Results get merged back through controlled integration

#### Task Prioritization
- **High Priority**: Core architecture decisions (done in main tree)
- **Medium Priority**: Feature implementation (feature branch)  
- **Low Priority**: Research/prototyping (separate worktrees)

#### Git Workflow
1. Create feature branch `cymbiont-1.0-reconstruction` from main
2. Create separate worktrees for parallel research
3. Subagents work in worktrees, report findings
4. Primary development integrates findings into feature branch
5. Feature branch merges to main as Cymbiont 1.0

### Key Files to Reference

#### Active Plans
- `CYMBIONT_1.0_PLAN.md` - Overall vision and architecture
- `feature_taskpad_terminal_first_evolution.md` - Implementation roadmap
- `docs/active_integration_plans/feature_taskpad_aichat_agent_integration.md` - AIChat integration specs

#### Architecture
- `cymbiont_architecture.md` - Current system documentation
- `src/lib.rs` - Library interface (created during evolution)

#### Configuration
- `CLAUDE.local.md` - Local development context with evolution tracking
- `CLAUDE.md` - Project guidelines and build commands

### Immediate Next Steps for New Session

1. **Remove cymbiont-skeleton submodule** (no longer needed)
2. **Create worktree workflow documentation**
3. **Set up feature branch for main development**
4. **Create context management system for subagents**
5. **Triage and redistribute interrupted tasks**

### Context for Subagents

When deploying subagents in new session:
- Each works in separate worktree
- Focus on research/prototyping, not main implementation
- Report findings back for integration decisions
- No direct commits to main branch

### Success Metrics

The evolution is successful when:
- Terminal-first CLI interface working
- AIChat-agent integration functional via library
- Multi-graph support maintained
- Import-only PKM functionality replaces sync
- Clean Unix pipe interface implemented
- Performance targets met (<10ms queries)

### Technical Debt

Items that need resolution:
- Dead code removal from Logseq-specific modules
- Configuration simplification
- WebSocket protocol adaptation for agents
- HTTP API adaptation for programmatic access

---

## Usage Instructions for New Session

1. Read this document completely
2. Check current git status and branch
3. Review the three key planning documents listed above
4. Establish worktree workflow before deploying subagents
5. Focus on controlled, incremental evolution rather than parallel chaos

## Emergency Recovery

If something goes wrong:
- Git state is clean at commit `1785de3`
- All critical context preserved in docs/
- AIChat integration plan fully documented
- Can restart evolution from stable point