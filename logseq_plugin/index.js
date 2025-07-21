/**
 * @module index
 * @description Main entry point for the Logseq Knowledge Graph Plugin
 * 
 * CRITICAL WARNING FOR LLM ASSISTANTS:
 * =====================================
 * This is a BROWSER-BASED Logseq plugin. DO NOT use Node.js features like:
 * - require() or import statements
 * - module.exports
 * - fs, path, or any Node.js modules
 * 
 * All dependencies are loaded via <script> tags in index.html and exposed as globals.
 * Breaking this plugin makes it very difficult to debug due to Logseq's opaque error handling.
 * 
 * DO NOT make "improvements" or "modernizations" without explicit user request.
 * This code works as-is. Random changes have broken production systems before.
 * 
 * TODO: Consider freezing Logseq version to avoid breaking API changes
 * The onChanged API changed from accepting an array to an object structure,
 * breaking our real-time sync without warning. We should investigate:
 * - Pinning to a specific Logseq version
 * - Adding version detection and compatibility layers
 * - Monitoring Logseq release notes for API changes
 * 
 * This module orchestrates the entire plugin functionality, connecting Logseq to a Rust-based 
 * knowledge graph backend. It handles initialization, event registration, data synchronization,
 * and communication between the Logseq frontend and the Rust backend server.
 * 
 * Key responsibilities:
 * - Plugin initialization and setup
 * - Setting up event listeners for database changes and page navigation
 * - Managing real-time sync for individual changes
 * - Handling batch processing of blocks and pages
 * - Managing custom block timestamps queue
 * - Coordinating between sync module and real-time changes
 * - Exposing shared functions to other modules via window globals
 * 
 * API Communication (via window.KnowledgeGraphAPI):
 * - sendToBackend(data) - Send data to the backend server
 * - checkSyncStatus() - Check current sync status with backend
 * - getBackendUrl(endpoint) - Get the backend URL for an endpoint
 * - updateSyncTimestamp() - Update the last sync timestamp
 * - log.error/warn/info/debug/trace(message, details, source) - Send logs to backend
 * 
 * Message types sent to backend:
 * - type_: 'block' - Individual block data
 * - type_: 'blocks' - Batch of block data
 * - type_: 'page' - Individual page data
 * - type_: 'pages' - Batch of page data
 * - type_: 'plugin_initialized' - Plugin startup notification
 * 
 * The plugin automatically:
 * - Monitors database changes via logseq.DB.onChanged
 * - Tracks page navigation via logseq.App.onRouteChanged
 * - Checks if a full sync is needed on startup
 * 
 * Dependencies:
 * - api.js: Handles all HTTP communication with the backend (loaded as KnowledgeGraphAPI global)
 * - data_processor.js: Processes and validates Logseq data (loaded as KnowledgeGraphDataProcessor global)
 * 
 * INCREMENTAL SYNC SYSTEM:
 * =======================
 * The plugin implements an incremental sync system to dramatically improve performance for large
 * databases. Instead of syncing all content every 2 hours, it only syncs what has changed.
 * 
 * How it works:
 * 1. Pages use Logseq's built-in `updatedAt` field for change detection
 * 2. Blocks use custom `cymbiont-updated-ms` properties managed by this plugin
 * 3. On each sync, only pages/blocks modified since the last sync are processed
 * 
 * Block Timestamp Management:
 * - Since Logseq blocks don't have reliable built-in timestamps, we add custom properties
 * - The property name is converted from kebab-case to camelCase by Logseq: `cymbiontUpdatedMs`
 * - Timestamps are set when blocks are first synced or when changes are detected
 * - Empty blocks and blocks with only properties are filtered out to avoid clutter
 * 
 * Configuration Required:
 * Users must add the following to their Logseq config.edn to hide the timestamp property:
 * ```clojure
 * :block-hidden-properties #{:cymbiont-updated-ms}
 * ```
 * TODO: Implement programmatic config.edn editing to automate this
 * 
 * Performance Impact:
 * - Full sync of 4000 pages/40000 blocks: ~20+ seconds
 * - Incremental sync with minimal changes: <1 second
 * - Bottleneck: Thousands of sequential `getPageBlocksTree()` API calls
 * 
 * Known Limitations:
 * - Properties are visible until user adds config and restarts Logseq
 * - Logseq may update page timestamps on startup (contents, favorites, card pages)
 * - Block property persistence depends on Logseq not re-indexing the graph
 */

/**
 * Logseq Knowledge Graph Plugin
 * Connects Logseq to a Rust-based knowledge graph backend
 */

// The API and config are loaded via script tags in index.html
// They are available as global objects: KnowledgeGraphAPI and KnowledgeGraphDataProcessor

//=============================================================================
// LOGSEQ API INTERACTION
//=============================================================================

//=============================================================================
// BACKEND COMMUNICATION
// These functions now use the global KnowledgeGraphAPI object
//=============================================================================


// Check if backend server is available with retry logic
async function checkBackendAvailabilityWithRetry(maxRetries = 3, retryDelayMs = 1000) {
  // Use the global KnowledgeGraphAPI object's checkBackendAvailabilityWithRetry function
  return KnowledgeGraphAPI.checkBackendAvailabilityWithRetry(maxRetries, retryDelayMs);
}

//=============================================================================
// DATA PROCESSING & EXTRACTION
// These functions now use the global KnowledgeGraphDataProcessor object
//=============================================================================

// Process block data and extract relevant information
async function processBlockData(block) {
  return KnowledgeGraphDataProcessor.processBlockData(block);
}

// Process page data and extract relevant information
async function processPageData(page) {
  return KnowledgeGraphDataProcessor.processPageData(page);
}

//=============================================================================
// DATA VALIDATION
// These functions now use the global KnowledgeGraphDataProcessor object
//=============================================================================

// Validate block data before sending to backend
function validateBlockData(blockData) {
  return KnowledgeGraphDataProcessor.validateBlockData(blockData);
}

// Validate page data before sending to backend
function validatePageData(pageData) {
  return KnowledgeGraphDataProcessor.validatePageData(pageData);
}

//=============================================================================
// VALIDATION ISSUE TRACKING
// Now uses the global KnowledgeGraphDataProcessor.validationIssues object
//=============================================================================

// Global validation issue tracker - reference to the one in KnowledgeGraphDataProcessor
const validationIssues = KnowledgeGraphDataProcessor.validationIssues;

//=============================================================================
// REAL-TIME SYNC HANDLING
//=============================================================================

// Process a batch of pages or blocks
async function processBatch(type, items, graphName, batchSize = 100, source = 'Full Sync') {
  if (!items || items.length === 0) return;
  
  const batch = [];
  
  for (const item of items) {
    try {
      if (type === 'block') {
        // Skip file-level changes (they have path but no uuid)
        if (item.path && !item.uuid) {
          // This is a file change event, not a block change
          continue;
        }
        if (!item.uuid) {
          KnowledgeGraphAPI.log.error('Block missing UUID', {block: item});
          continue;
        }
        const blockData = await processBlockData(item);
        if (!blockData) {
          // Skip silently - empty blocks are normal
          continue;
        }
        const validation = validateBlockData(blockData);
        if (validation.valid) {
          batch.push(blockData);
        } else {
          KnowledgeGraphAPI.log.warn(`Invalid block data for UUID ${item.uuid}`, validation.errors);
          validationIssues.addBlockIssue(blockData.id, blockData.page, validation.errors);
        }
      } else if (type === 'page') {
        if (!item.name) {
          KnowledgeGraphAPI.log.error('Page missing name', {page: item});
          continue;
        }
        const pageData = await processPageData(item);
        if (!pageData) {
          // Skip silently
          continue;
        }
        const validation = validatePageData(pageData);
        if (validation.valid) {
          batch.push(pageData);
        } else {
          KnowledgeGraphAPI.log.warn(`Invalid page data for "${item.name}"`, validation.errors);
          validationIssues.addPageIssue(pageData.name, validation.errors);
        }
      }

      if (batch.length >= batchSize) {
        await sendBatchToBackend(type, batch, graphName, source);
        batch.length = 0;
      }
    } catch (error) {
      const identifier = type === 'block' ? item.uuid : `"${item.name}"`;
      KnowledgeGraphAPI.log.error(`Error processing ${type} ${identifier}`, {error: error.message});
    }
  }

  // Send any remaining items
  if (batch.length > 0) {
    await sendBatchToBackend(type, batch, graphName, source);
  }
}

// Global queue for timestamp updates to prevent race conditions
let timestampQueue = new Set();
let processingTimestamps = false;

// Expose timestampQueue globally for sync module
window.timestampQueue = timestampQueue;

// Process the timestamp queue in one batch
async function processTimestampQueue() {
  if (processingTimestamps || timestampQueue.size === 0) {
    return;
  }
  
  processingTimestamps = true;
  const currentTimestamp = Date.now();
  const blocksToUpdate = Array.from(timestampQueue);
  timestampQueue.clear();
  
  try {
    for (const blockUuid of blocksToUpdate) {
      try {
        await logseq.Editor.upsertBlockProperty(blockUuid, 'cymbiont-updated-ms', currentTimestamp);
      } catch (error) {
        KnowledgeGraphAPI.log.error(`Failed to update timestamp for block ${blockUuid}`, {error: error.message});
      }
    }
  } finally {
    processingTimestamps = false;
  }
}

// Handle database changes
async function handleDBChanges(changesData) {
  // Prevent infinite loops from our own timestamp property additions
  if (processingTimestamps) {
    return;
  }
  
  // The changes parameter is an object with blocks array, not an array itself
  if (!changesData || typeof changesData !== 'object') {
    return;
  }
  
  // Extract the blocks and pages from the changes object
  const changes = [{
    blocks: changesData.blocks || [],
    pages: changesData.pages || []
  }];
  
  // Only log if we have actual changes
  const hasChanges = (changesData.blocks && changesData.blocks.length > 0) || 
                    (changesData.pages && changesData.pages.length > 0);
  
  if (!hasChanges) {
    return;
  }
  
  
  // Queue blocks for timestamp updates (avoids race conditions)
  for (const change of changes) {
    if (change.blocks && change.blocks.length > 0) {
      for (const block of change.blocks) {
        if (block.uuid) {
          // Check if this change is just from our timestamp property update
          // If the block has our timestamp property and no other meaningful changes, skip it
          try {
            const fullBlock = await logseq.Editor.getBlock(block.uuid);
            if (fullBlock && fullBlock.properties && fullBlock.properties['cymbiontUpdatedMs']) {
              // Block already has our timestamp - this might be a change from our own timestamp update
              // Skip adding to queue to prevent infinite loops
              continue;
            } else {
              // This block doesn't have our timestamp yet
            }
          } catch (error) {
            // If we can't check, err on the side of processing
            KnowledgeGraphAPI.log.warn(`Could not check timestamp property for ${block.uuid}, processing anyway`);
          }
          
          timestampQueue.add(block.uuid);
        }
      }
    }
  }
  
  // Check if backend is available before processing changes (light retry for real-time)
  try {
    const backendAvailable = await checkBackendAvailabilityWithRetry(1, 500);
    if (!backendAvailable) {
      KnowledgeGraphAPI.log.warn('Backend server not available. Real-time changes will not be processed.');
      return;
    }
    
    // Get current graph name
    const graph = await logseq.App.getCurrentGraph();
    if (!graph || !graph.name) {
      KnowledgeGraphAPI.log.error('Failed to get current graph name.');
      return;
    }
    
    const graphName = graph.name;
    
    // Process each change
    for (const change of changes) {
      // Process block changes
      if (change.blocks && change.blocks.length > 0) {
        // Process blocks silently
        await processBatch('block', change.blocks, graphName, 100, 'Real-time Sync');
      }
      
      // Process page changes  
      if (change.pages && change.pages.length > 0) {
        // Process pages silently
        await processBatch('page', change.pages, graphName, 100, 'Real-time Sync');
      }
    }
    
    // Process any queued timestamp updates after handling the changes
    await processTimestampQueue();
  } catch (error) {
    KnowledgeGraphAPI.log.error('Error handling DB changes', {error: error.message, stack: error.stack});
  }
}

// Send a batch of data to the backend
async function sendBatchToBackend(type, batch, graphName, source = 'Full Sync') {
  // Use the global KnowledgeGraphAPI object's sendBatchToBackend function
  return KnowledgeGraphAPI.sendBatchToBackend(type, batch, graphName, source);
}

// Expose functions needed by sync module
window.processBatch = processBatch;
window.processTimestampQueue = processTimestampQueue;
window.sendBatchToBackend = sendBatchToBackend;
//=============================================================================
// SYNC MODULE INTEGRATION
//=============================================================================

//=============================================================================
// PLUGIN INITIALIZATION
//=============================================================================

// Main function for plugin logic
async function main() {
  // Check if required global objects are available
  if (typeof window.KnowledgeGraphAPI === 'undefined') {
    // Can't use our logging API if it doesn't exist!
    console.error('ERROR: KnowledgeGraphAPI not found! api.js may not have loaded properly.');
    logseq.App.showMsg('Plugin initialization failed: API module not loaded', 'error');
    return;
  }
  
  if (typeof window.KnowledgeGraphDataProcessor === 'undefined') {
    KnowledgeGraphAPI.log.error('KnowledgeGraphDataProcessor not found! data_processor.js may not have loaded properly.');
    logseq.App.showMsg('Plugin initialization failed: Data processor module not loaded', 'error');
    return;
  }
  
  if (typeof window.KnowledgeGraphSync === 'undefined') {
    KnowledgeGraphAPI.log.error('KnowledgeGraphSync not found! sync.js may not have loaded properly.');
    logseq.App.showMsg('Plugin initialization failed: Sync module not loaded', 'error');
    return;
  }
  

  // Set up DB change monitoring
  logseq.DB.onChanged(handleDBChanges);
  
  // Listen for page open events
  logseq.App.onRouteChanged(async ({ path }) => {
    if (path.startsWith('/page/')) {
      // const pageName = decodeURIComponent(path.substring(6));
      // Silent - we don't need to log every page navigation
      
      // You could trigger a sync here if needed
    }
  });
  
  // Send initialization signal to backend first
  try {
    const result = await KnowledgeGraphAPI.sendToBackend({
      source: 'PKM Plugin Startup',
      timestamp: Date.now().toString(),
      type_: 'plugin_initialized',
      payload: JSON.stringify({ message: 'PKM Knowledge Graph Plugin initialized successfully' })
    });
    
    // Show single UI notification for successful plugin load
    if (result) {
      logseq.App.showMsg('Cymbiont initialized', 'success');
    }
  } catch (error) {
    KnowledgeGraphAPI.log.error('Failed to send plugin initialization signal', {error: error.message});
  }
  
  // Check if we need to do any sync immediately
  const syncStatus = await KnowledgeGraphSync.checkSyncStatus();
  
  if (syncStatus.needsSync) {
    const success = await KnowledgeGraphSync.syncDatabase(syncStatus.syncType);
    
    if (success) {
      await KnowledgeGraphSync.updateSyncTimestamp(syncStatus.syncType);
      // Success message already shown by syncDatabase
    } else {
      // Error message already shown by syncDatabase
    }
    
    // Signal sync completion regardless of success/failure
    await KnowledgeGraphAPI.sendToBackend({
      source: 'PKM Plugin Sync',
      timestamp: Date.now().toString(),
      type_: 'sync_complete',
      payload: JSON.stringify({ success })
    });
  } else {
    // No sync needed - signal completion immediately
    await KnowledgeGraphAPI.sendToBackend({
      source: 'PKM Plugin Sync',
      timestamp: Date.now().toString(),
      type_: 'sync_complete', 
      payload: JSON.stringify({ syncSkipped: true })
    });
  }
}

// Initialize the plugin
logseq.ready(main).catch((error) => {
  // Can't use our logging API here if initialization fails
  console.error('Plugin initialization failed:', error);
});
