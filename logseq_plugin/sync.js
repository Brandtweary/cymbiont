/**
 * @module sync
 * @description Synchronization orchestration for the Logseq Knowledge Graph Plugin
 * 
 * This module handles all database synchronization logic including:
 * - Full and incremental sync orchestration
 * - Block and page processing with batching
 * - Sync status management
 * - Tree traversal utilities
 * 
 * CRITICAL: This is a BROWSER-BASED module. NO Node.js features allowed.
 * 
 * Dependencies (must be loaded before this module):
 * - api.js (window.KnowledgeGraphAPI)
 * - data_processor.js (window.KnowledgeGraphDataProcessor) 
 * - Logseq API (global logseq object)
 * 
 * The module exposes its functionality via window.KnowledgeGraphSync
 */

(function() {
  'use strict';

  // Reference to validation issues from data_processor.js  
  const validationIssues = window.KnowledgeGraphDataProcessor.validationIssues;

  /**
   * Check what type of sync is needed by querying the backend
   * @returns {Promise<{needsSync: boolean, syncType?: string}>}
   */
  async function checkSyncStatus() {
    try {
      const response = await fetch(await KnowledgeGraphAPI.getBackendUrl('/sync/status'), {
        method: 'GET',
        headers: {
          'Content-Type': 'application/json',
        },
      });
      
      if (!response.ok) {
        KnowledgeGraphAPI.log.error('Failed to get sync status from backend');
        return { needsSync: false };
      }
      
      const status = await response.json();
      
      // Check sync needs in priority order
      // Force flags override config settings
      if (status.force_full_sync || (status.true_full_sync_needed && status.sync_config?.enable_full_sync)) {
        return { needsSync: true, syncType: 'full' };
      } else if (status.force_incremental_sync || status.incremental_sync_needed) {
        return { needsSync: true, syncType: 'incremental' };
      } else {
        return { needsSync: false };
      }
    } catch (error) {
      KnowledgeGraphAPI.log.error('Error checking sync status', {error: error.message});
      return { needsSync: false };
    }
  }

  /**
   * Update the sync timestamp on the backend
   * @param {string} syncType - Type of sync ('incremental' or 'full')
   * @returns {Promise<boolean>}
   */
  async function updateSyncTimestamp(syncType = 'incremental') {
    // Use the global KnowledgeGraphAPI object's updateSyncTimestamp function
    return KnowledgeGraphAPI.updateSyncTimestamp(syncType);
  }

  /**
   * Sync all pages and blocks in the database
   * @param {string} syncType - 'incremental' (default) or 'full'
   * @returns {Promise<boolean>} Success status
   */
  async function syncDatabase(syncType = 'incremental') {
    const syncTypeDisplay = syncType === 'full' ? 'full database' : 'incremental';
    KnowledgeGraphAPI.log.info(`🔄 Starting ${syncTypeDisplay} sync`);
    
    
    // Check if backend is available with retry logic for critical full sync
    const backendAvailable = await KnowledgeGraphAPI.checkBackendAvailabilityWithRetry(3, 2000);
    if (!backendAvailable) {
      KnowledgeGraphAPI.log.error('Backend server not available after retries. Sync aborted.');
      logseq.App.showMsg('Backend server not available after retries. Start the server first.', 'error');
      return false;
    }
    
    try {
      // Get last sync timestamp from backend
      let lastSyncDate = null;
      if (syncType === 'incremental') {
        try {
          const response = await fetch(await KnowledgeGraphAPI.getBackendUrl('/sync/status'), {
            method: 'GET',
            headers: {
              'Content-Type': 'application/json',
            },
          });
          
          if (response.ok) {
            const status = await response.json();
            if (status.last_incremental_sync_iso) {
              lastSyncDate = new Date(status.last_incremental_sync_iso);
              KnowledgeGraphAPI.log.debug(`Last incremental sync: ${status.last_incremental_sync_iso}`);
            } else {
              KnowledgeGraphAPI.log.debug('No previous incremental sync found');
            }
          }
        } catch (error) {
          KnowledgeGraphAPI.log.warn('Failed to get sync status, treating as first sync', {error: error.message});
        }
      } else {
        KnowledgeGraphAPI.log.info('🔄 Performing full database sync - no timestamp filtering');
      }
      
      // Reset validation issues tracker
      validationIssues.reset();
      
      // Get current graph
      const graph = await logseq.App.getCurrentGraph();
      if (!graph) {
        KnowledgeGraphAPI.log.error('Failed to get current graph.');
        logseq.App.showMsg('Failed to get current graph.', 'error');
        return false;
      }
      
      const graphName = graph.name;
      
      // Get all pages
      const allPages = await logseq.Editor.getAllPages();
      
      if (!allPages || !Array.isArray(allPages)) {
        KnowledgeGraphAPI.log.error('Failed to fetch pages from database.');
        logseq.App.showMsg('Failed to fetch pages from database.', 'error');
        return false;
      }
      
      
      // Filter pages based on last sync timestamp if doing incremental sync
      let pagesToSync = allPages;
      if (syncType === 'incremental' && lastSyncDate) {
        pagesToSync = allPages.filter(page => {
          // If page has updated timestamp, check if it's newer than last sync
          if (page.updatedAt) {
            const pageUpdated = new Date(page.updatedAt);
            return pageUpdated > lastSyncDate;
          }
          // If no updated timestamp, include it to be safe
          return true;
        });
        
        KnowledgeGraphAPI.log.info(`🔄 Incremental sync: ${pagesToSync.length} of ${allPages.length} pages modified`);
      } else {
        KnowledgeGraphAPI.log.info(`🔄 Full sync: processing all ${allPages.length} pages`);
      }
      
      // Track progress
      let pagesProcessed = 0;
      let blocksProcessed = 0;
      
      // Track block sync stats for debugging
      // let blocksSkipped = 0;
      // let blocksModified = 0;
      // let blocksWithoutTimestamp = 0;
      
      // Track all PKM IDs for deletion detection
      const allPkmIds = {
        blocks: [],
        pages: []
      };
      
      // Shared block batch for efficient processing across all pages
      const globalBlockBatch = [];
      
      // Collect ALL page names for deletion detection (not just modified ones)
      for (const page of allPages) {
        if (page.name) {
          allPkmIds.pages.push(page.name);
        }
      }
      
      // Process pages in batches
      for (let i = 0; i < pagesToSync.length; i += 100) {
        const pageBatch = pagesToSync.slice(i, i + 100);
        
        await processBatch('page', pageBatch, graphName);
        pagesProcessed += pageBatch.length;
        
        // Process blocks for these pages
        for (const page of pageBatch) {
          const pageBlocksTree = await logseq.Editor.getPageBlocksTree(page.name);
          if (pageBlocksTree) {
            
            const blockStats = { skipped: 0, modified: 0, noTimestamp: 0 };
            const syncDateForBlocks = syncType === 'incremental' ? lastSyncDate : null;
            await processBlocksRecursively(pageBlocksTree, graphName, globalBlockBatch, 100, syncDateForBlocks, blockStats);
            const pageBlockCount = countBlocksInTree(pageBlocksTree);
            blocksProcessed += pageBlockCount;
            // blocksSkipped += blockStats.skipped;
            // blocksModified += blockStats.modified;
            // blocksWithoutTimestamp += blockStats.noTimestamp;
            
            // Silent progress - no UI spam
          }
        }
      }
      
      // Now collect ALL block IDs for deletion detection (separate pass)
      for (const page of allPages) {
        const pageBlocksTree = await logseq.Editor.getPageBlocksTree(page.name);
        if (pageBlocksTree) {
          collectBlockIds(pageBlocksTree, allPkmIds.blocks);
        }
      }
      
      // Send any remaining blocks in the final batch
      if (globalBlockBatch.length > 0) {
        await sendBatchToBackend('block', globalBlockBatch.slice(), graphName);
        globalBlockBatch.splice(0); // Clear for consistency
      }

      // Display validation summary if there were issues
      const summary = validationIssues.getSummary();
      if (summary.totalBlockIssues > 0 || summary.totalPageIssues > 0) {
        KnowledgeGraphAPI.log.warn('Validation issues during sync', summary);
        
        // Show a user-friendly message with counts
        logseq.App.showMsg(
          `Sync completed with issues: ${summary.totalBlockIssues} block issues, ${summary.totalPageIssues} page issues.`, 
          'warning'
        );
      } else {
        // Show success message
        const displayType = syncType === 'full' ? 'Full database' : 'Incremental';
        logseq.App.showMsg(`${displayType} sync completed successfully!`, 'success');
      }
      
      // Log summary at info level - this is one of our few info logs
      KnowledgeGraphAPI.log.info(`✅ ${syncType} sync completed`, {
        pages: pagesProcessed,
        blocks: blocksProcessed,
        pageErrors: summary.totalPageIssues || 0,
        blockErrors: summary.totalBlockIssues || 0,
        syncType: syncType
      });
      
      // Process any queued timestamp updates before finishing sync
      await processTimestampQueue();
      
      // Send all PKM IDs to backend for deletion detection
      try {
        const response = await fetch(await KnowledgeGraphAPI.getBackendUrl('/sync/verify'), {
          method: 'POST',
          headers: KnowledgeGraphAPI.buildHeaders(),
          body: JSON.stringify({
            pages: allPkmIds.pages,
            blocks: allPkmIds.blocks
          })
        });
        
        if (!response.ok) {
          KnowledgeGraphAPI.log.warn('Failed to verify PKM IDs with backend');
        }
      } catch (error) {
        KnowledgeGraphAPI.log.warn('Failed to send PKM IDs for deletion detection', {error: error.message});
      }
      
      return true;
    } catch (error) {
      KnowledgeGraphAPI.log.error('Error during full database sync', {error: error.message, stack: error.stack});
      logseq.App.showMsg('Error during full database sync.', 'error');
      return false;
    }
  }

  /**
   * Process blocks recursively with batching
   * @private
   */
  async function processBlocksRecursively(blocks, graphName, blockBatch, batchSize, lastSyncDate = null, stats = null) {
    if (!blocks || !Array.isArray(blocks)) return;
    
    for (const block of blocks) {
      try {
        // Skip blocks without UUIDs
        if (!block.uuid) {
          KnowledgeGraphAPI.log.error('Block missing UUID in recursive processing', {block});
          continue;
        }
        
        // Get the full block data to check for our custom timestamp property
        const fullBlock = await logseq.Editor.getBlock(block.uuid);
        if (!fullBlock) {
          KnowledgeGraphAPI.log.error(`Could not fetch full block data for ${block.uuid}`);
          continue;
        }
        
        // Check for our custom timestamp property
        let blockUpdatedMs = fullBlock.properties?.['cymbiontUpdatedMs'];
        let shouldSync = true;
        
        if (lastSyncDate) {
          const lastSyncMs = lastSyncDate.getTime();
          
          if (blockUpdatedMs) {
            // Block has timestamp - compare with last sync
            const blockUpdatedTime = parseInt(blockUpdatedMs);
            if (blockUpdatedTime <= lastSyncMs) {
              shouldSync = false;
              if (stats) stats.skipped++;
            } else {
              if (stats) stats.modified++;
            }
          } else {
            // Block missing timestamp - initialize it and treat as modified
            // Queue block for timestamp initialization
            window.timestampQueue.add(block.uuid);
            if (stats) stats.noTimestamp++;
          }
        } else {
          // Full sync - ensure all blocks have timestamps
          if (!blockUpdatedMs) {
            // Queue block for timestamp initialization
            window.timestampQueue.add(block.uuid);
            if (stats) stats.noTimestamp++;
          } else {
            if (stats) stats.modified++;
          }
        }
        
        // Only process if we should sync this block
        if (shouldSync) {
          // Process this block
          const blockData = await KnowledgeGraphDataProcessor.processBlockData(block);
          if (!blockData) {
            // Skip silently - empty blocks are normal
            continue;
          }
          
          const validation = KnowledgeGraphDataProcessor.validateBlockData(blockData);
          if (validation.valid) {
            // Add to block batch instead of sending immediately
            blockBatch.push(blockData);
            
            // Send batch if it reaches the batch size
            if (blockBatch.length >= batchSize) {
              await sendBatchToBackend('block', blockBatch.slice(), graphName);
              blockBatch.splice(0); // Clear array safely
            }
          } else {
            KnowledgeGraphAPI.log.warn(`Invalid block data for ${block.uuid}`, validation.errors);
            validationIssues.addBlockIssue(blockData.id, blockData.page, validation.errors);
          }
        }
        
        // Process children recursively
        if (block.children && block.children.length > 0) {
          await processBlocksRecursively(block.children, graphName, blockBatch, batchSize, lastSyncDate, stats);
        }
      } catch (blockError) {
        KnowledgeGraphAPI.log.error(`Error processing block ${block.uuid}`, {error: blockError.message});
        // Continue with other blocks even if one fails
      }
    }
  }

  /**
   * Count blocks in a tree (for progress reporting)
   * @param {Array} blocks - Block tree structure
   * @returns {number} Total count of blocks
   */
  function countBlocksInTree(blocks) {
    if (!blocks || !Array.isArray(blocks)) return 0;
    
    let count = blocks.length;
    
    for (const block of blocks) {
      if (block.children && block.children.length > 0) {
        count += countBlocksInTree(block.children);
      }
    }
    
    return count;
  }

  /**
   * Collect all block IDs from a tree recursively
   * @param {Array} blocks - Block tree structure
   * @param {Array} idArray - Array to collect IDs into
   */
  function collectBlockIds(blocks, idArray) {
    if (!blocks || !Array.isArray(blocks)) return;
    
    for (const block of blocks) {
      if (block.uuid) {
        idArray.push(block.uuid);
      }
      
      if (block.children && block.children.length > 0) {
        collectBlockIds(block.children, idArray);
      }
    }
  }

  // Expose the sync module API
  window.KnowledgeGraphSync = {
    checkSyncStatus,
    updateSyncTimestamp,
    syncDatabase,
    countBlocksInTree,
    collectBlockIds
  };

})();