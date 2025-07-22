# Logseq Plugin API Research: Block and Page Manipulation

## Executive Summary

This document contains comprehensive research on Logseq's plugin API for block and page manipulation, conducted to support Cymbiont's bidirectional PKM synchronization feature. Key findings include critical API limitations (notably `updateBlock` destroying properties) and recommended patterns for safe block/page operations.

## Critical API Quirks and Limitations

### 1. **updateBlock Destroys Properties** ⚠️
- **Issue**: `logseq.Editor.updateBlock()` removes ALL existing block properties
- **GitHub Issues**: #5298, #8686
- **Impact**: Cannot use for updating content while preserving metadata
- **Workaround**: Include properties in content string or use only `upsertBlockProperty`

### 2. **Race Conditions with Combined Operations**
- **Issue**: Using `updateBlock` followed by `upsertBlockProperty` can cause block reversion
- **Impact**: Properties may not persist, content may revert
- **Workaround**: Avoid combining these operations; use one or the other

### 3. **Custom UUID Limitations**
- **`appendBlockInPage`**: Does NOT support custom UUIDs
- **`insertBlock`**: Supports custom UUIDs via `customUUID` option
- **`insertBatchBlock`**: Supports custom UUIDs via `keepUUID: true` but has visual bugs

### 4. **Timestamp Reliability**
- Logseq blocks lack reliable native timestamps
- Plugin must manage custom timestamps via properties
- Custom properties visible until manually hidden in config.edn

### 5. **API Breaking Changes**
- Logseq's plugin API has history of breaking changes without warning
- Currently using `@logseq/libs` version 0.0.14
- Must be prepared for API evolution

## Updated Understanding

Based on further discussion, several concerns are simplified:

1. **Property Loss with updateBlock**: Not an issue since we only use the `cymbiont-updated-ms` timestamp property, which should be updated anyway when content changes.

2. **Custom UUID Support**: Not needed. Logseq manages its own UUIDs. Cymbiont generates node IDs for the knowledge graph, and the plugin returns Logseq UUIDs after block creation.

3. **Workflow**: 
   - Graph Manager creates node (KG-only at this point)
   - Backend sends HTTP request to plugin to create block
   - Plugin creates block and returns Logseq UUID
   - Backend stores Logseq UUID in graph node for future operations

4. **Real-time Sync Interaction**: Block creation via API may trigger change detection. Need to test and potentially implement temporary disable flag.

## Recommended API Patterns

### Creating Blocks

#### Single Block with Custom UUID
```javascript
await logseq.Editor.insertBlock(
  targetBlockUUID,  // Parent or sibling block
  "Block content here",
  {
    customUUID: "your-custom-uuid",
    sibling: true,  // Insert as sibling (false = child)
    properties: {
      "source": "cymbiont",
      "created": Date.now()
    }
  }
);
```

#### Appending to Page (No Custom UUID)
```javascript
// Simple append to end of page
await logseq.Editor.appendBlockInPage(
  pageNameOrUUID,
  "Block content",
  {
    properties: {
      "source": "cymbiont"
    }
  }
);
```

#### Batch Creation with UUIDs
```javascript
await logseq.Editor.insertBatchBlock(
  parentUUID,
  {
    content: "Parent block",
    properties: { id: "custom-parent-uuid" },
    children: [{
      content: "Child block 1",
      properties: { id: "custom-child-1-uuid" }
    }, {
      content: "Child block 2", 
      properties: { id: "custom-child-2-uuid" }
    }]
  },
  { keepUUID: true }
);
```

### Updating Blocks

#### Safe Property Update (Preserves Content)
```javascript
await logseq.Editor.upsertBlockProperty(
  blockUUID,
  'last-modified',
  Date.now()
);
```

#### Content Update (Properties Lost!)
```javascript
// ⚠️ WARNING: This removes all properties!
await logseq.Editor.updateBlock(blockUUID, "New content");

// If properties must be preserved, include them:
await logseq.Editor.updateBlock(blockUUID, `:PROPERTIES:
:key:: value
:another:: value2
:END:
New content here`);
```

### Page Operations

#### Check Page Existence
```javascript
const page = await logseq.Editor.getPage(pageName);
if (!page) {
  // Page doesn't exist
}
```

#### Create Page
```javascript
const newPage = await logseq.Editor.createPage(
  "cymbiont-knowledge-graph",
  {
    "source": "cymbiont",
    "created": Date.now()
  },
  {
    createFirstBlock: true,
    redirect: false,
    journal: false
  }
);
```

#### Append to Existing Page
```javascript
// Get page reference first
const page = await logseq.Editor.getPage("cymbiont-knowledge-graph");
if (page) {
  await logseq.Editor.appendBlockInPage(
    page.uuid,  // Use UUID for reliability
    "New block content",
    { properties: { "added": Date.now() } }
  );
}
```

## Implementation Strategy for Cymbiont

### 1. Block Creation Flow
```javascript
async function createBlockInPKM(blockData) {
  // Determine target page
  const pageName = blockData.page || "cymbiont-knowledge-graph";
  
  // Ensure page exists
  let page = await logseq.Editor.getPage(pageName);
  if (!page) {
    page = await logseq.Editor.createPage(pageName, {}, {
      createFirstBlock: false,
      redirect: false
    });
  }
  
  // Append block to page (no custom UUID support)
  const newBlock = await logseq.Editor.appendBlockInPage(
    page.uuid,
    blockData.content,
    {
      properties: {
        "cymbiont-id": blockData.id,
        "cymbiont-created": Date.now()
      }
    }
  );
  
  return newBlock;
}
```

### 2. Block Update Flow
```javascript
async function updateBlockInPKM(blockId, updates) {
  // Find block by cymbiont-id property
  // Note: Logseq doesn't have direct property search
  // May need to maintain UUID mapping
  
  // Update only properties (safe)
  await logseq.Editor.upsertBlockProperty(
    blockUUID,
    'cymbiont-updated',
    Date.now()
  );
  
  // If content update needed, must include all properties
  if (updates.content) {
    const currentBlock = await logseq.Editor.getBlock(blockUUID);
    const properties = currentBlock.properties || {};
    
    // Build content with properties
    let fullContent = "";
    if (Object.keys(properties).length > 0) {
      fullContent = ":PROPERTIES:\n";
      for (const [key, value] of Object.entries(properties)) {
        fullContent += `:${key}:: ${value}\n`;
      }
      fullContent += ":END:\n";
    }
    fullContent += updates.content;
    
    await logseq.Editor.updateBlock(blockUUID, fullContent);
  }
}
```

### 3. Error Handling Pattern
```javascript
async function safeBlockOperation(operation) {
  try {
    return await operation();
  } catch (error) {
    // Log to Cymbiont backend
    await sendLogToBackend({
      level: 'error',
      message: `Block operation failed: ${error.message}`,
      error: error.stack
    });
    
    // Fallback handling
    if (error.message.includes('Block not found')) {
      // Handle missing block
    } else if (error.message.includes('Page not found')) {
      // Handle missing page
    }
    
    throw error;  // Re-throw for caller handling
  }
}
```

## Plugin HTTP Endpoint Design

### Proposed Endpoints for Cymbiont → Plugin Communication

```javascript
// POST /api/blocks/create
{
  "content": "Block content",
  "page": "page-name",  // Optional, defaults to "cymbiont-knowledge-graph"
  "properties": {
    "cymbiont-id": "unique-id"
  }
}

// PATCH /api/blocks/:cymbiontId
{
  "content": "Updated content",  // Optional
  "properties": {  // Optional
    "key": "value"
  }
}

// DELETE /api/blocks/:cymbiontId
// No body required

// POST /api/pages/create
{
  "name": "page-name",
  "properties": {}
}
```

## Testing Considerations

1. **Mock Logseq API**: Create mock implementations for testing
2. **Property Preservation**: Test that properties survive operations
3. **Race Conditions**: Test rapid sequential operations
4. **Error Cases**: Test with non-existent blocks/pages
5. **Large Batches**: Test performance with many blocks
6. **Special Characters**: Test with various content formats

## Conclusion

The Logseq plugin API provides sufficient functionality for bidirectional sync, but with significant caveats around property handling. The recommended approach is:

1. Use `insertBlock` for precise control (not `appendBlockInPage` if UUIDs needed)
2. Never use `updateBlock` for content updates if properties must be preserved
3. Maintain a mapping between Cymbiont IDs and Logseq UUIDs
4. Always include error handling and retry logic
5. Test thoroughly with the specific Logseq version in use