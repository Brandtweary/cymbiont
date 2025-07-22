/**
 * @module websocket
 * @description WebSocket command handlers for bidirectional communication
 * 
 * CRITICAL WARNING FOR LLM ASSISTANTS:
 * =====================================
 * This is a BROWSER-BASED module. DO NOT add Node.js features.
 * This file exposes window.KnowledgeGraphWebSocket - do not change this pattern.
 * 
 * This module provides WebSocket command handlers that execute Logseq operations
 * in response to commands from the Cymbiont backend. It bridges the gap between
 * the Rust backend's knowledge graph operations and Logseq's editor API.
 * 
 * Key responsibilities:
 * - Register command handlers for create/update/delete operations
 * - Execute Logseq API calls based on backend commands
 * - Handle errors gracefully and report back via logging
 * - Preserve block properties during updates (Logseq's updateBlock destroys them)
 * 
 * Command types handled:
 * - create_block: Create a new block with optional parent/page placement
 * - update_block: Update block content while preserving properties
 * - delete_block: Remove a block from the graph
 * - create_page: Create a new page with optional properties
 * 
 * KNOWN ISSUE - Redundant Real-time Sync:
 * ========================================
 * When WebSocket commands modify blocks (create/update/delete), Logseq's DB.onChanged
 * event fires multiple times (typically 3-5 times) for a single operation. This is a
 * known characteristic of the Logseq plugin API (see GitHub issue #5662).
 * 
 * Impact: Each WebSocket-triggered block operation causes redundant sync events that
 * send the same data back to Cymbiont. This is harmless because our sync system is
 * robust against identical changes - it simply updates the existing node with the
 * same data.
 * 
 * We chose NOT to implement throttling or workarounds because:
 * - The redundant syncs don't break anything or cause data corruption
 * - Adding complexity to work around Logseq's behavior isn't worth it
 * - The Logseq team considers this "correct behavior" (though suboptimal)
 * - Our sync system already handles duplicate updates gracefully
 * 
 * Dependencies:
 * - Logseq API: For all editor operations
 * - window.KnowledgeGraphAPI: For logging and WebSocket registration
 */

// Create a global WebSocket handler object
window.KnowledgeGraphWebSocket = {};

/**
 * Register all WebSocket command handlers
 * Called during plugin initialization
 */
window.KnowledgeGraphWebSocket.registerHandlers = function() {
  // Handler for creating blocks
  window.KnowledgeGraphAPI.websocket.registerHandler('create_block', async (command) => {
    try {
      const { content, parent_id, page_name } = command;
      
      let block;
      if (parent_id) {
        // Create as child of existing block
        block = await logseq.Editor.insertBlock(parent_id, content, {
          before: false,
          sibling: false
        });
      } else if (page_name) {
        // Create on specific page
        const page = await logseq.Editor.getPage(page_name);
        if (!page) {
          window.KnowledgeGraphAPI.log.error(`Page not found: ${page_name}`);
          return;
        }
        block = await logseq.Editor.appendBlockInPage(page.uuid, content);
      } else {
        // Default: create on current page
        const currentPage = await logseq.Editor.getCurrentPage();
        if (!currentPage) {
          window.KnowledgeGraphAPI.log.error('No current page to create block on');
          return;
        }
        block = await logseq.Editor.appendBlockInPage(currentPage.uuid, content);
      }
      
      if (block) {
        window.KnowledgeGraphAPI.log.debug(`Created block: ${block.uuid}`);
      }
    } catch (error) {
      window.KnowledgeGraphAPI.log.error('Failed to create block', {
        error: error.message,
        command
      });
    }
  });
  
  // Handler for updating blocks
  window.KnowledgeGraphAPI.websocket.registerHandler('update_block', async (command) => {
    try {
      const { block_id, content } = command;
      
      // Get the block first to preserve properties
      const block = await logseq.Editor.getBlock(block_id);
      if (!block) {
        window.KnowledgeGraphAPI.log.error(`Block not found: ${block_id}`);
        return;
      }
      
      // Update the block content
      await logseq.Editor.updateBlock(block_id, content);
      
      // Restore properties that updateBlock destroys
      if (block.properties && Object.keys(block.properties).length > 0) {
        for (const [key, value] of Object.entries(block.properties)) {
          await logseq.Editor.upsertBlockProperty(block_id, key, value);
        }
      }
      
      window.KnowledgeGraphAPI.log.debug(`Updated block: ${block_id}`);
    } catch (error) {
      window.KnowledgeGraphAPI.log.error('Failed to update block', {
        error: error.message,
        command
      });
    }
  });
  
  // Handler for deleting blocks
  window.KnowledgeGraphAPI.websocket.registerHandler('delete_block', async (command) => {
    try {
      const { block_id } = command;
      
      await logseq.Editor.removeBlock(block_id);
      window.KnowledgeGraphAPI.log.debug(`Deleted block: ${block_id}`);
    } catch (error) {
      window.KnowledgeGraphAPI.log.error('Failed to delete block', {
        error: error.message,
        command
      });
    }
  });
  
  // Handler for creating pages
  window.KnowledgeGraphAPI.websocket.registerHandler('create_page', async (command) => {
    try {
      const { name, properties } = command;
      
      // Create the page
      const page = await logseq.Editor.createPage(name, properties || {}, {
        createFirstBlock: false,
        redirect: false
      });
      
      if (page) {
        window.KnowledgeGraphAPI.log.debug(`Created page: ${name}`);
      }
    } catch (error) {
      window.KnowledgeGraphAPI.log.error('Failed to create page', {
        error: error.message,
        command
      });
    }
  });
};

/**
 * Initialize WebSocket connection
 * @returns {Promise<boolean>} - Whether connection was successful
 */
window.KnowledgeGraphWebSocket.connect = async function() {
  return window.KnowledgeGraphAPI.websocket.connect();
};

/**
 * Disconnect WebSocket
 */
window.KnowledgeGraphWebSocket.disconnect = function() {
  window.KnowledgeGraphAPI.websocket.disconnect();
};