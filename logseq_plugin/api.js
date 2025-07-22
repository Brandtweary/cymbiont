/**
 * @module api
 * @description Communication layer for the Logseq Knowledge Graph Plugin
 * 
 * CRITICAL WARNING FOR LLM ASSISTANTS:
 * =====================================
 * This is a BROWSER-BASED module. DO NOT add Node.js features.
 * This file exposes window.KnowledgeGraphAPI - do not change this pattern.
 * Breaking changes here will cause silent failures in Logseq.
 * 
 * LOGGING STANDARD:
 * - console.error() and console.warn() are acceptable for errors/warnings
 * - DO NOT use console.log() for info/debug logging - we avoid UI spam
 * - Use the HTTP logging API (KnowledgeGraphAPI.log.*) from other modules
 * - This module can't use HTTP logging for its own errors (chicken-egg problem)
 * 
 * This module provides a comprehensive API for all communication between the Logseq frontend
 * and the Rust backend server. It handles constructing API endpoints, sending data, checking
 * server availability, and managing sync operations.
 * 
 * The module exposes its functionality through the global `window.KnowledgeGraphAPI` object,
 * making these functions available to other parts of the plugin, particularly index.js.
 * 
 * Key responsibilities:
 * - Constructing backend URLs for various endpoints
 * - Sending data (blocks, pages, diagnostics) to the backend
 * - Checking backend server availability
 * - Managing sync status and operations
 * - Handling batch operations for efficient data transfer
 * - Error handling and reporting for network operations
 * 
 * Public interfaces:
 * - getBackendUrl(endpoint): Constructs a complete backend URL for a given endpoint
 * - sendToBackend(data): Sends data to the backend's /data endpoint
 * - log: Logging system with error(), warn(), info(), debug(), trace() methods
 * - checkBackendAvailability(): Verifies if the backend server is running
 * - checkIfFullSyncNeeded(): Determines if a full database sync is required
 * - updateSyncTimestamp(): Updates the last sync timestamp on the backend
 * - sendBatchToBackend(type, batch, graphName): Sends a batch of blocks or pages
 * 
 * Dependencies:
 * - Logseq API: For displaying messages and getting graph information
 * 
 * Note: Port configuration is discovered dynamically to match the Rust server's port discovery logic
 */

// Create a global API object to hold all the functions
window.KnowledgeGraphAPI = {};

// TODO: Implement localStorage logging for errors/warnings in this module
// Since this is the logging API itself, we can't use HTTP logging for internal errors.
// Consider writing errors/warnings to localStorage with timestamps for debugging.

// Cache for server info
let serverInfoCache = null;
let serverInfoLastChecked = 0;
const SERVER_INFO_CACHE_MS = 5000; // Cache for 5 seconds

/**
 * Read server info from the JSON file written by the backend
 * @returns {Object|null} - Server info or null if not found
 */
/**
 * Discover the backend server port by trying the same ports the Rust server uses
 * This duplicates the server's port discovery logic to ensure they find the same port
 * @returns {Promise<Object|null>} - Server info or null if not found
 */
async function discoverServerPort() {
  // Check cache first
  const now = Date.now();
  if (serverInfoCache && (now - serverInfoLastChecked) < SERVER_INFO_CACHE_MS) {
    return serverInfoCache;
  }
  
  try {
    // Use the same port discovery logic as the Rust server:
    // - Start with default port 3000
    // - Try up to 10 additional ports (3000-3010)
    // - This matches the server's max_port_attempts configuration
    const defaultPort = 3000;
    const maxPortAttempts = 10;
    
    for (let i = 0; i <= maxPortAttempts; i++) {
      const port = defaultPort + i;
      
      try {
        const response = await fetch(`http://127.0.0.1:${port}/`, {
          method: 'GET',
          signal: AbortSignal.timeout(500) // Quick timeout to try multiple ports
        });
        
        if (response.ok) {
          // Server is responding, cache this port info
          const serverInfo = {
            host: '127.0.0.1',
            port: port,
            discovered: true
          };
          serverInfoCache = serverInfo;
          serverInfoLastChecked = now;
          return serverInfo;
        }
      } catch {
        // Try next port
        continue;
      }
    }
    
    console.warn('No backend server found on any expected port (3000-3010)');
  } catch (error) {
    console.error('Error discovering server port:', error);
  }
  
  return null;
}

/**
 * Get the backend URL for a specific endpoint
 * @param {string} endpoint - The endpoint path (e.g., '/data', '/')
 * @returns {string} - The complete backend URL
 */
window.KnowledgeGraphAPI.getBackendUrl = async function(endpoint) {
  // Try to discover the server port
  const serverInfo = await discoverServerPort();
  
  if (serverInfo) {
    return `http://${serverInfo.host}:${serverInfo.port}${endpoint}`;
  }
  
  // Fall back to default port (most common case)
  return `http://127.0.0.1:3000${endpoint}`;
};

/**
 * Send data to the backend server
 * @param {Object} data - Data to send to the backend
 * @returns {Promise<boolean>} - Whether the data was sent successfully
 */
window.KnowledgeGraphAPI.sendToBackend = async function(data) {
  const backendUrl = await window.KnowledgeGraphAPI.getBackendUrl('/data');
  
  try {
    const response = await fetch(backendUrl, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
      },
      body: JSON.stringify(data),
    });

    if (response.ok) {
      return true;
    } else {
      console.error(`Backend server responded with status: ${response.status}`);
      return false;
    }
  } catch (error) {
    console.error('Failed to send data to backend:', error);
    return false;
  }
}

/**
 * Logging system matching Rust tracing levels
 * @namespace
 */
window.KnowledgeGraphAPI.log = {
  /**
   * Send a log message to the backend server
   * @param {string} level - Log level (error, warn, info, debug, trace)
   * @param {string} message - Log message
   * @param {Object} details - Optional additional details
   * @param {string} source - Optional source identifier
   * @returns {Promise<boolean>} - Whether the log was sent successfully
   */
  async send(level, message, details = null, source = null) {
    const logUrl = await window.KnowledgeGraphAPI.getBackendUrl('/log');
    
    try {
      const response = await fetch(logUrl, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
        },
        body: JSON.stringify({
          level,
          message,
          details,
          source
        }),
      });
      
      return response.ok;
    } catch (error) {
      // Fallback to console if backend is unavailable
      console.error('Failed to send log to backend:', error);
      // Still output the original log to console as fallback
      if (level === 'error') {
        console.error(`[${level}] ${message}`, details);
      } else if (level === 'warn') {
        console.warn(`[${level}] ${message}`, details);
      }
      return false;
    }
  },
  
  /**
   * Log an error message
   * @param {string} message - Error message
   * @param {Object} details - Optional error details
   * @param {string} source - Optional source identifier
   */
  async error(message, details = null, source = null) {
    console.error(message, details); // Also log to console
    return this.send('error', message, details, source);
  },
  
  /**
   * Log a warning message
   * @param {string} message - Warning message
   * @param {Object} details - Optional warning details
   * @param {string} source - Optional source identifier
   */
  async warn(message, details = null, source = null) {
    console.warn(message, details); // Also log to console
    return this.send('warn', message, details, source);
  },
  
  /**
   * Log an info message
   * @param {string} message - Info message
   * @param {Object} details - Optional info details
   * @param {string} source - Optional source identifier
   */
  async info(message, details = null, source = null) {
    return this.send('info', message, details, source);
  },
  
  /**
   * Log a debug message
   * @param {string} message - Debug message
   * @param {Object} details - Optional debug details
   * @param {string} source - Optional source identifier
   */
  async debug(message, details = null, source = null) {
    return this.send('debug', message, details, source);
  },
  
  /**
   * Log a trace message
   * @param {string} message - Trace message
   * @param {Object} details - Optional trace details
   * @param {string} source - Optional source identifier
   */
  async trace(message, details = null, source = null) {
    return this.send('trace', message, details, source);
  }
};

/**
 * Check if backend server is available (single attempt)
 * @returns {Promise<boolean>} - Whether the backend server is available
 */
window.KnowledgeGraphAPI.checkBackendAvailability = async function() {
  try {
    const response = await fetch(await window.KnowledgeGraphAPI.getBackendUrl('/'), {
      method: 'GET',
      headers: {
        'Content-Type': 'application/json',
      },
    });
    
    return response.ok;
  } catch (error) {
    console.error('Error checking backend availability:', error);
    return false;
  }
}

/**
 * Check if backend server is available with retry logic
 * @param {number} maxRetries - Maximum number of retry attempts (default: 3)
 * @param {number} retryDelayMs - Delay between retries in milliseconds (default: 1000)
 * @returns {Promise<boolean>} - Whether the backend server is available
 */
window.KnowledgeGraphAPI.checkBackendAvailabilityWithRetry = async function(maxRetries = 3, retryDelayMs = 1000) {
  for (let attempt = 0; attempt <= maxRetries; attempt++) {
    const isAvailable = await this.checkBackendAvailability();
    if (isAvailable) {
      return true;
    }
    
    if (attempt < maxRetries) {
      await new Promise(resolve => setTimeout(resolve, retryDelayMs));
    }
  }
  
  console.error(`Backend not available after ${maxRetries} retry attempts`);
  return false;
}

/**
 * Check if a full sync is needed by querying the backend
 * @returns {Promise<boolean>} - Whether a full sync is needed
 */
window.KnowledgeGraphAPI.checkIfFullSyncNeeded = async function() {
  try {
    // Check if backend is available
    const backendAvailable = await window.KnowledgeGraphAPI.checkBackendAvailability();
    if (!backendAvailable) {
      return false;
    }
    
    // Query the backend for sync status
    const response = await fetch(await window.KnowledgeGraphAPI.getBackendUrl('/sync/status'), {
      method: 'GET',
      headers: {
        'Content-Type': 'application/json',
      },
    });
    
    if (!response.ok) {
      console.error('Error getting sync status from backend');
      return false;
    }
    
    const status = await response.json();
    
    // Return whether a full sync is needed
    return status.full_sync_needed === true;
  } catch (error) {
    console.error('Error checking if full sync is needed:', error);
    await window.KnowledgeGraphAPI.log.error('Error checking if full sync needed', { 
      error: error.message,
      stack: error.stack
    });
    return false;
  }
}

/**
 * Update the sync timestamp on the backend
 * @param {string} syncType - The type of sync ('incremental' or 'full')
 * @returns {Promise<boolean>} - Whether the update was successful
 */
window.KnowledgeGraphAPI.updateSyncTimestamp = async function(syncType = 'incremental') {
  try {
    const response = await fetch(await window.KnowledgeGraphAPI.getBackendUrl('/sync'), {
      method: 'PATCH',
      headers: {
        'Content-Type': 'application/json',
      },
      body: JSON.stringify({
        sync_type: syncType
      })
    });
    
    if (!response.ok) {
      console.error(`Error updating ${syncType} sync timestamp on backend`);
      return false;
    }
    
    const result = await response.json();
    
    return result.success === true;
  } catch (error) {
    console.error(`Error updating ${syncType} sync timestamp:`, error);
    await window.KnowledgeGraphAPI.log.error(`Error updating ${syncType} sync timestamp`, { 
      error: error.message,
      stack: error.stack
    });
    return false;
  }
}

/**
 * Send a batch of data to the backend
 * @param {string} type - Type of data (block or page)
 * @param {Array} batch - Array of data items
 * @param {string} graphName - Name of the graph
 * @param {string} source - Source of the sync (default: 'Full Sync')
 */
window.KnowledgeGraphAPI.sendBatchToBackend = async function(type, batch, graphName, source = 'Full Sync') {
  if (batch.length === 0) return;
  
  try {
    await window.KnowledgeGraphAPI.sendToBackend({
      source: source,
      timestamp: new Date().toISOString(),
      graphName: graphName,
      type_: `${type}_batch`,
      payload: JSON.stringify(batch)
    });
  } catch (error) {
    console.error(`Error sending ${type} batch:`, error);
  }
}

/**
 * WebSocket Client for bidirectional communication
 * @namespace
 */
window.KnowledgeGraphAPI.websocket = {
  ws: null,
  reconnectInterval: null,
  reconnectDelay: 1000, // Start with 1 second
  maxReconnectDelay: 30000, // Max 30 seconds
  authenticated: false,
  commandQueue: [],
  commandHandlers: {},
  
  /**
   * Connect to WebSocket server
   * @returns {Promise<boolean>} - Whether connection was successful
   */
  async connect() {
    try {
      // Get server info first
      const serverInfo = await discoverServerPort();
      if (!serverInfo) {
        console.error('WebSocket: No backend server found');
        return false;
      }
      
      const wsUrl = `ws://${serverInfo.host}:${serverInfo.port}/ws`;
      
      this.ws = new WebSocket(wsUrl);
      
      this.ws.onopen = () => {
        console.info('WebSocket: Connected to backend');
        this.reconnectDelay = 1000; // Reset reconnect delay
        this.authenticated = false;
        
        // Send auth command immediately
        this.send({ type: 'auth', token: 'dummy-token' }); // TODO: Real auth token
        
        // Process queued commands
        while (this.commandQueue.length > 0 && this.ws.readyState === WebSocket.OPEN) {
          const cmd = this.commandQueue.shift();
          this.ws.send(JSON.stringify(cmd));
        }
      };
      
      this.ws.onmessage = async (event) => {
        try {
          const response = JSON.parse(event.data);
          await this.handleResponse(response);
        } catch (error) {
          console.error('WebSocket: Error parsing message:', error);
        }
      };
      
      this.ws.onerror = (error) => {
        console.error('WebSocket: Error:', error);
      };
      
      this.ws.onclose = () => {
        console.info('WebSocket: Disconnected');
        this.authenticated = false;
        this.scheduleReconnect();
      };
      
      return true;
    } catch (error) {
      console.error('WebSocket: Connection error:', error);
      this.scheduleReconnect();
      return false;
    }
  },
  
  /**
   * Schedule reconnection with exponential backoff
   */
  scheduleReconnect() {
    if (this.reconnectInterval) {
      clearTimeout(this.reconnectInterval);
    }
    
    this.reconnectInterval = setTimeout(() => {
      console.info(`WebSocket: Attempting reconnect (delay: ${this.reconnectDelay}ms)`);
      this.connect();
      
      // Exponential backoff
      this.reconnectDelay = Math.min(this.reconnectDelay * 2, this.maxReconnectDelay);
    }, this.reconnectDelay);
  },
  
  /**
   * Send a command to the backend
   * @param {Object} command - Command to send
   */
  send(command) {
    if (this.ws && this.ws.readyState === WebSocket.OPEN) {
      this.ws.send(JSON.stringify(command));
    } else {
      // Queue command for when connection is restored
      this.commandQueue.push(command);
      // Ensure we're trying to connect
      if (!this.reconnectInterval) {
        this.scheduleReconnect();
      }
    }
  },
  
  /**
   * Handle response from backend
   * @param {Object} response - Response from backend
   */
  async handleResponse(response) {
    switch (response.type) {
      case 'heartbeat':
        // Send pong
        this.send({ type: 'heartbeat' });
        break;
        
      case 'success':
        if (!this.authenticated) {
          this.authenticated = true;
          console.info('WebSocket: Authenticated');
        }
        break;
        
      case 'error':
        console.error('WebSocket: Command error:', response.message);
        break;
        
      case 'create_block':
      case 'update_block':
      case 'delete_block':
      case 'create_page':
        // Forward to command handlers
        const handler = this.commandHandlers[response.type];
        if (handler) {
          await handler(response);
        } else {
          console.warn(`WebSocket: No handler for command type: ${response.type}`);
        }
        break;
        
      case 'test':
        // Handle test command - echo it back to test bidirectional flow
        console.info('WebSocket: Test command received:', response);
        window.KnowledgeGraphAPI.log.info('Test command received', response);
        
        // Send a test command back to the server
        if (response.message) {
          this.send({
            type: 'test',
            message: `Echo from client: ${response.message}`
          });
          window.KnowledgeGraphAPI.log.info('Sent test response back to server');
        }
        break;
        
      default:
        console.warn('WebSocket: Unknown response type:', response.type);
    }
  },
  
  /**
   * Register a command handler
   * @param {string} type - Command type
   * @param {Function} handler - Handler function
   */
  registerHandler(type, handler) {
    this.commandHandlers[type] = handler;
  },
  
  /**
   * Disconnect WebSocket
   */
  disconnect() {
    if (this.reconnectInterval) {
      clearTimeout(this.reconnectInterval);
      this.reconnectInterval = null;
    }
    
    if (this.ws) {
      this.ws.close();
      this.ws = null;
    }
    
    this.authenticated = false;
    this.commandQueue = [];
  }
};
