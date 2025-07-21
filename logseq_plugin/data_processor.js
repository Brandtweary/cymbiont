/**
 * @module data_processor
 * @description Data processing and validation layer for the Logseq Knowledge Graph Plugin
 * 
 * CRITICAL WARNING FOR LLM ASSISTANTS:
 * =====================================
 * This is a BROWSER-BASED module. DO NOT add Node.js features.
 * This file exposes window.KnowledgeGraphDataProcessor - do not change this pattern.
 * Breaking changes here will cause silent failures in Logseq.
 * 
 * This module is responsible for extracting, processing, and validating data from the Logseq
 * database before it's sent to the backend. It handles parsing Logseq's block and page data,
 * extracting references (links, tags, etc.), and ensuring data integrity.
 * 
 * The module exposes its functionality through the global `window.KnowledgeGraphDataProcessor` 
 * object, making these functions available to other parts of the plugin, particularly index.js.
 * 
 * Key responsibilities:
 * - Extracting references from block content (page links, block refs, tags, properties)
 * - Processing raw Logseq block and page data into structured formats for the backend
 * - Validating data integrity before transmission to the backend
 * - Tracking and categorizing validation issues for reporting
 * 
 * Public interfaces:
 * - extractReferencesFromContent(content): Extracts all references from text using regex
 * - processBlockData(block): Processes a Logseq block into a structured format
 * - processPageData(page): Processes a Logseq page into a structured format
 * - validateBlockData(blockData): Validates block data before sending to backend
 * - validatePageData(pageData): Validates page data before sending to backend
 * - validationIssues: Object for tracking and categorizing validation issues
 *   - addBlockIssue(blockId, pageName, issues): Adds block validation issues
 *   - addPageIssue(pageName, issues): Adds page validation issues
 *   - getSummary(): Gets a summary of all validation issues
 *   - reset(): Resets the validation issue tracker
 * 
 * Dependencies:
 * - Logseq API: For retrieving block and page data
 * 
 * BlockEntity Interface Reference:
 * ================================
 * The complete BlockEntity interface from the official Logseq plugin API includes these fields:
 * 
 * ```typescript
 * export interface BlockEntity {
 *   // Core fields
 *   id: number                    // Database ID
 *   uuid: string                  // Block UUID
 *   content: string              // Block content
 *   format: 'markdown' | 'org'   // Block format
 *   left: IEntityID              // Left reference
 *   parent: IEntityID            // Parent block reference
 *   page: IEntityID              // Page reference
 *   unordered: boolean           // Whether block is unordered
 *   
 *   // Optional fields
 *   anchor?: string
 *   body?: any
 *   children?: Array<BlockEntity | BlockUUIDTuple>
 *   container?: string
 *   file?: IEntityID
 *   level?: number
 *   title?: Array<any>
 *   properties?: Record<string, any>
 *   
 *   // Timestamp-related field
 *   meta?: {
 *     timestamps: any,           // Contains timestamp data (structure unspecified)
 *     properties: any,
 *     startPos: number,
 *     endPos: number
 *   }
 *   
 *   [key: string]: any          // Index signature for additional properties
 * }
 * ```
 * 
 * Note: The meta.timestamps field exists but is unreliable and poorly documented. 
 * We use custom properties for timestamp tracking instead.
 */

// Create a global object for data processing functions
window.KnowledgeGraphDataProcessor = {};

//=============================================================================
// DATA EXTRACTION
//=============================================================================

// Extract all references from content using regex
window.KnowledgeGraphDataProcessor.extractReferencesFromContent = function(content) {
  if (!content) return [];
  
  const references = [];
  
  // Extract page references [[Page Name]]
  const pageRefRegex = /\[\[(.*?)\]\]/g;
  let match;
  while ((match = pageRefRegex.exec(content)) !== null) {
    references.push({
      type: 'page',
      name: match[1].trim()
    });
  }
  
  // Extract block references ((block-id))
  const blockRefRegex = /\(\((.*?)\)\)/g;
  while ((match = blockRefRegex.exec(content)) !== null) {
    references.push({
      type: 'block',
      id: match[1].trim()
    });
  }
  
  // Extract hashtags #tag
  const tagRegex = /#([a-zA-Z0-9_-]+)/g;
  while ((match = tagRegex.exec(content)) !== null) {
    // Don't include the # symbol in the tag name
    references.push({
      type: 'tag',
      name: match[1].trim()
    });
  }
  
  // Extract properties key:: value
  const propRegex = /([a-zA-Z0-9_-]+)::\s*(.*?)($|\n)/g;
  while ((match = propRegex.exec(content)) !== null) {
    const propName = match[1].trim();
    // Note: propValue is not used here as we only extract the key as a reference
    // The full key-value pairs are sent via the structured properties field
    
    // The property key is treated as a page reference (e.g., "status" implies a "status" page)
    references.push({
      type: 'property',
      name: propName
    });
    
    // Check if the property value contains references
    // We don't need to extract these since they'll be caught by the other regex patterns
    // when we process the full content
  }
  
  return references;
};

//=============================================================================
// DATA PROCESSING
//=============================================================================

// Process block data and extract relevant information
window.KnowledgeGraphDataProcessor.processBlockData = async function(block) {
  try {
    // Get full block content and metadata with includeChildren option
    const blockEntity = await logseq.Editor.getBlock(block.uuid, { includeChildren: true });
    if (!blockEntity) {
      console.error(`Failed to get block with UUID: ${block.uuid}`);
      return null;
    }
    
    // Filter out empty blocks - they don't belong in a knowledge graph
    if (!blockEntity.content || blockEntity.content.trim() === '') {
      // Just skip this block entirely without adding to validation issues
      return null;
    }
    
    // Also filter out blocks that only contain properties (no actual content)
    // This handles blocks that only have our timestamp property
    const contentWithoutProperties = blockEntity.content.replace(/^[a-zA-Z0-9-]+::\s*[^\n]*\n?/gm, '').trim();
    if (contentWithoutProperties === '') {
      return null;
    }
    
    // Get the page that contains this block
    const page = blockEntity.page ? await logseq.Editor.getPage(blockEntity.page.id) : null;
    
    // Extract all references from the content using our unified regex approach
    const references = this.extractReferencesFromContent(blockEntity.content);
    
    // Get parent UUID instead of parent ID
    let parentUUID = null;
    if (blockEntity.parent) {
      // If parent is an object with a uuid property, use that
      if (blockEntity.parent.uuid) {
        parentUUID = blockEntity.parent.uuid;
      } 
      // If we only have the parent ID, try to get the block to get its UUID
      else if (blockEntity.parent.id) {
        try {
          const parentBlock = await logseq.Editor.getBlock(blockEntity.parent.id, { includeChildren: true });
          if (parentBlock) {
            parentUUID = parentBlock.uuid;
          }
        } catch (e) {
          // Only log actual errors
          console.error(`Could not resolve parent ID ${blockEntity.parent.id} to UUID for block ${blockEntity.uuid}`);
        }
      }
    }
    
    return {
      id: blockEntity.uuid,
      content: blockEntity.content,
      created: blockEntity.created || new Date().toISOString(),
      updated: blockEntity.updated || new Date().toISOString(),
      parent: parentUUID,
      children: blockEntity.children ? blockEntity.children.map(child => 
        typeof child === 'object' && child.uuid ? child.uuid : 
        typeof child === 'string' ? child : null
      ).filter(Boolean) : [],
      page: page ? page.name : null,
      properties: blockEntity.properties || {},
      references: references
    };
  } catch (error) {
    // Only log actual errors
    console.error('Error processing block data:', error);
    return null;
  }
};

// Process page data and extract relevant information
window.KnowledgeGraphDataProcessor.processPageData = async function(page) {
  try {
    // Skip pages without names
    if (!page.name || page.name.trim() === '') {
      this.validationIssues.addPageIssue('unknown', ['Nameless page - skipped']);
      return null; // Skip this page entirely
    }
    
    // Get page properties and metadata
    const pageEntity = await logseq.Editor.getPage(page.name);
    if (!pageEntity) {
      console.error(`Failed to get page with name: ${page.name}`);
      return null;
    }
    
    // Get all blocks in the page
    const blocks = await logseq.Editor.getPageBlocksTree(page.name);
    const blockIds = blocks ? blocks.map(block => block.uuid) : [];
    
    // Get page properties
    const properties = pageEntity.properties || {};
    
    return {
      name: page.name,
      normalized_name: page.name.toLowerCase(),
      created: pageEntity.created || new Date().toISOString(),
      updated: pageEntity.updated || new Date().toISOString(),
      properties: properties,
      blocks: blockIds
    };
  } catch (error) {
    console.error('Error processing page data:', error);
    return null;
  }
};

//=============================================================================
// DATA VALIDATION
//=============================================================================

// Validate block data before sending to backend
window.KnowledgeGraphDataProcessor.validateBlockData = function(blockData) {
  if (!blockData) {
    console.error('Block data is null or undefined');
    return { valid: false, errors: ['Block data is null or undefined'] };
  }
  
  const errors = [];
  
  // Check required fields
  if (!blockData.id || typeof blockData.id !== 'string') {
    errors.push(`Invalid block ID: ${blockData.id}`);
  }
  
  // Check for missing or empty content
  if (blockData.content === undefined) {
    errors.push('Missing block content field');
  } else if (blockData.content === null || blockData.content.trim() === '') {
    errors.push('Block content is empty');
  }
  
  // Validate created/updated timestamps
  if (!blockData.created || typeof blockData.created !== 'string') {
    errors.push(`Invalid created timestamp: ${blockData.created}`);
  }
  
  if (!blockData.updated || typeof blockData.updated !== 'string') {
    errors.push(`Invalid updated timestamp: ${blockData.updated}`);
  }
  
  // Validate parent (should be null or string UUID)
  if (blockData.parent !== null && typeof blockData.parent !== 'string') {
    errors.push(`Invalid parent reference: ${blockData.parent}`);
  }
  
  // Validate children (should be array of string UUIDs)
  if (!Array.isArray(blockData.children)) {
    errors.push(`Children is not an array: ${blockData.children}`);
  } else {
    for (let i = 0; i < blockData.children.length; i++) {
      const child = blockData.children[i];
      if (typeof child !== 'string') {
        errors.push(`Invalid child reference at index ${i}: ${child}`);
      }
    }
  }
  
  // Validate references
  if (!Array.isArray(blockData.references)) {
    errors.push(`References is not an array: ${blockData.references}`);
  } else {
    for (let i = 0; i < blockData.references.length; i++) {
      const ref = blockData.references[i];
      if (!ref.type) {
        errors.push(`Missing reference type at index ${i}`);
      }
    }
  }
  
  return { 
    valid: errors.length === 0,
    errors: errors
  };
};

// Validate page data before sending to backend
window.KnowledgeGraphDataProcessor.validatePageData = function(pageData) {
  if (!pageData) {
    console.error('Page data is null or undefined');
    return { valid: false, errors: ['Page data is null or undefined'] };
  }
  
  const errors = [];
  
  // Check required fields
  if (!pageData.name || typeof pageData.name !== 'string') {
    errors.push(`Invalid page name: ${pageData.name}`);
  }
  
  // Validate created/updated timestamps
  if (!pageData.created || typeof pageData.created !== 'string') {
    errors.push(`Invalid created timestamp: ${pageData.created}`);
  }
  
  if (!pageData.updated || typeof pageData.updated !== 'string') {
    errors.push(`Invalid updated timestamp: ${pageData.updated}`);
  }
  
  // Validate blocks (should be array of string UUIDs)
  if (!Array.isArray(pageData.blocks)) {
    errors.push(`Blocks is not an array: ${pageData.blocks}`);
  } else {
    for (let i = 0; i < pageData.blocks.length; i++) {
      const block = pageData.blocks[i];
      if (typeof block !== 'string') {
        errors.push(`Invalid block reference at index ${i}: ${block}`);
      }
    }
  }
  
  return { 
    valid: errors.length === 0,
    errors: errors
  };
};

//=============================================================================
// VALIDATION ISSUE TRACKING
//=============================================================================

// Global validation issue tracker
window.KnowledgeGraphDataProcessor.validationIssues = {
  blocks: {},
  pages: {},
  totalBlockIssues: 0,
  totalPageIssues: 0,
  
  // Add a block validation issue
  addBlockIssue(blockId, pageName, issues) {
    if (!this.blocks[pageName]) {
      this.blocks[pageName] = {};
    }
    
    // Parse issues into specific types
    if (typeof issues === 'string') {
      issues = [issues];
    }
    
    for (const issue of issues) {
      const issueType = this.categorizeIssue(issue);
      if (!this.blocks[pageName][issueType]) {
        this.blocks[pageName][issueType] = 0;
      }
      this.blocks[pageName][issueType]++;
      this.totalBlockIssues++;
    }
  },
  
  // Add a page validation issue
  addPageIssue(pageName, issues) {
    if (!this.pages[pageName]) {
      this.pages[pageName] = {};
    }
    
    // Parse issues into specific types
    if (typeof issues === 'string') {
      issues = [issues];
    }
    
    for (const issue of issues) {
      const issueType = this.categorizeIssue(issue);
      if (!this.pages[pageName][issueType]) {
        this.pages[pageName][issueType] = 0;
      }
      this.pages[pageName][issueType]++;
      this.totalPageIssues++;
    }
  },
  
  // Categorize an issue message into a specific type
  categorizeIssue(issue) {
    if (issue.includes('empty') || issue.includes('Empty')) {
      return 'empty_content';
    } else if (issue.includes('ID') || issue.includes('id')) {
      return 'invalid_id';
    } else if (issue.includes('timestamp')) {
      return 'invalid_timestamp';
    } else if (issue.includes('parent')) {
      return 'invalid_parent';
    } else if (issue.includes('child') || issue.includes('Children')) {
      return 'invalid_children';
    } else if (issue.includes('reference')) {
      return 'invalid_reference';
    } else {
      return 'other';
    }
  },
  
  // Get a summary of all validation issues
  getSummary() {
    const summary = {
      totalBlockIssues: this.totalBlockIssues,
      totalPageIssues: this.totalPageIssues,
      blockIssuesByPage: {},
      pageIssues: {}
    };
    
    // Summarize block issues by page with issue types
    for (const pageName in this.blocks) {
      const issueTypes = this.blocks[pageName];
      const totalPageIssues = Object.values(issueTypes).reduce((sum, count) => sum + count, 0);
      
      // Format as "pageName: count (type1: count1, type2: count2, ...)"
      const typeBreakdown = Object.entries(issueTypes)
        .map(([type, count]) => `${this.formatIssueType(type)}: ${count}`)
        .join(', ');
      
      summary.blockIssuesByPage[pageName] = {
        total: totalPageIssues,
        breakdown: typeBreakdown,
        types: issueTypes
      };
    }
    
    // Summarize page issues with types
    for (const pageName in this.pages) {
      const issueTypes = this.pages[pageName];
      const totalPageIssues = Object.values(issueTypes).reduce((sum, count) => sum + count, 0);
      
      const typeBreakdown = Object.entries(issueTypes)
        .map(([type, count]) => `${this.formatIssueType(type)}: ${count}`)
        .join(', ');
      
      summary.pageIssues[pageName] = {
        total: totalPageIssues,
        breakdown: typeBreakdown,
        types: issueTypes
      };
    }
    
    return summary;
  },
  
  // Format issue type for display
  formatIssueType(type) {
    switch (type) {
      case 'empty_content': return 'empty block content';
      case 'invalid_id': return 'invalid ID';
      case 'invalid_timestamp': return 'invalid timestamp';
      case 'invalid_parent': return 'invalid parent';
      case 'invalid_children': return 'invalid children';
      case 'invalid_reference': return 'invalid reference';
      case 'other': return 'other issues';
      default: return type;
    }
  },
  
  // Reset the tracker
  reset() {
    this.blocks = {};
    this.pages = {};
    this.totalBlockIssues = 0;
    this.totalPageIssues = 0;
  }
};
