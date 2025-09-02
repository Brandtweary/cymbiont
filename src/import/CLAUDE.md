# Import Module Guide 🚀

## Module Overview
Data import functionality for PKM systems, currently supporting Logseq markdown graphs.

## Core Components

### Data Structures
- **pkm_data.rs**: PKM-agnostic data types and helpers
  - `PKMBlockData`: Block content with hierarchy
  - `PKMPageData`: Page metadata and block lists
  - `PKMReference`: Cross-references between blocks
  - Helper functions for block reference resolution
  - `((block-id))` pattern expansion with circular reference prevention

### Import Pipeline
- **logseq.rs**: Logseq-specific parsing
  - Markdown file discovery
  - Frontmatter extraction
  - Block hierarchy parsing
  - Reference detection

- **import_utils.rs**: High-level coordination
  - `import_logseq_graph()` - Full import workflow
  - Prime agent authorization
  - Progress tracking

## Import Flow 🔄

1. **Discovery**: Find all .md files in Logseq directory
2. **Parse**: Extract frontmatter and block hierarchies
3. **Transform**: Convert to PKM data structures
4. **Create Graph**: Initialize new graph with metadata
5. **Apply Data**: Insert pages and blocks with edges (references resolved inline)
6. **Authorize Agent**: Grant prime agent access

## Key Functions

```rust
// Main entry point
import_logseq_graph(
    app_state,
    path,      // Logseq directory
    graph_name // Optional custom name
) -> Result<Uuid>

// Block reference resolution helper
resolve_block_references(
    content: &str,
    block_map: &HashMap<String, String>,
    visited: &mut HashSet<String>,
    current_block_id: Option<&str>
) -> String
```

## Edge Types 🔗
- `PageRef`: Page → Page connections
- `BlockRef`: Block → Block references
- `PageToBlock`: Page owns blocks
- `ParentChild`: Block hierarchies

## Error Handling
Import-specific errors with context:
- `ImportError::io(e)` - File system errors
- `ImportError::parse(line, msg)` - Malformed data
- `ImportError::reference(id)` - Invalid references

## Testing
Test graphs in `logseq_databases/`:
- `dummy_graph/` - Simple test data
- Validated via command log operations

## Future Extensions 🌟
- Additional PKM formats (Obsidian, Roam)
- Incremental imports
- Export functionality
- Migration between formats