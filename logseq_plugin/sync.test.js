// Create minimal window object
global.window = {};

// Mock logseq global
global.logseq = {
  App: {
    showMsg: jest.fn(),
    getCurrentGraph: jest.fn()
  },
  Editor: {
    getAllPages: jest.fn(),
    getPageBlocksTree: jest.fn(),
    getBlock: jest.fn(),
    upsertBlockProperty: jest.fn()
  }
};

// Mock fetch globally
global.fetch = jest.fn();

// Load data_processor.js first (sync.js depends on it)
require('./data_processor.js');

// Now mock the API module that sync.js also depends on
window.KnowledgeGraphAPI = {
  getBackendUrl: jest.fn((endpoint) => `http://localhost:3000${endpoint}`),
  log: {
    error: jest.fn(),
    warn: jest.fn(),
    info: jest.fn(),
    debug: jest.fn()
  },
  checkBackendAvailabilityWithRetry: jest.fn(),
  sendBatchToBackend: jest.fn(),
  updateSyncTimestamp: jest.fn()
};

// Mock the global functions that sync.js depends on
window.processBatch = jest.fn();
window.processTimestampQueue = jest.fn();
window.sendBatchToBackend = jest.fn();
window.timestampQueue = new Set();

// Now load sync.js
require('./sync.js');

// Get reference to the sync module
const syncModule = window.KnowledgeGraphSync;

// Mock console.error to silence error messages
const originalConsoleError = console.error;
beforeAll(() => {
  console.error = jest.fn();
});

afterAll(() => {
  console.error = originalConsoleError;
});

// Reset mocks before each test
beforeEach(() => {
  jest.clearAllMocks();
  fetch.mockClear();
});

describe('KnowledgeGraphSync', () => {
  describe('countBlocksInTree', () => {
    test('counts blocks in a simple tree', () => {
      const blocks = [
        { uuid: '1' },
        { uuid: '2' },
        { uuid: '3' }
      ];
      
      expect(syncModule.countBlocksInTree(blocks)).toBe(3);
    });

    test('counts blocks in a nested tree', () => {
      const blocks = [
        { 
          uuid: '1',
          children: [
            { uuid: '1.1' },
            { 
              uuid: '1.2',
              children: [
                { uuid: '1.2.1' },
                { uuid: '1.2.2' }
              ]
            }
          ]
        },
        { 
          uuid: '2',
          children: [
            { uuid: '2.1' }
          ]
        }
      ];
      
      expect(syncModule.countBlocksInTree(blocks)).toBe(7);
    });

    test('handles empty tree', () => {
      expect(syncModule.countBlocksInTree([])).toBe(0);
    });

    test('handles null or undefined input', () => {
      expect(syncModule.countBlocksInTree(null)).toBe(0);
      expect(syncModule.countBlocksInTree(undefined)).toBe(0);
    });

    test('handles blocks without children property', () => {
      const blocks = [
        { uuid: '1' },
        { uuid: '2', children: null },
        { uuid: '3', children: [] }
      ];
      
      expect(syncModule.countBlocksInTree(blocks)).toBe(3);
    });
  });

  describe('collectBlockIds', () => {
    test('collects IDs from a simple tree', () => {
      const blocks = [
        { uuid: 'id1' },
        { uuid: 'id2' },
        { uuid: 'id3' }
      ];
      const idArray = [];
      
      syncModule.collectBlockIds(blocks, idArray);
      
      expect(idArray).toEqual(['id1', 'id2', 'id3']);
    });

    test('collects IDs from a nested tree', () => {
      const blocks = [
        { 
          uuid: 'id1',
          children: [
            { uuid: 'id1.1' },
            { 
              uuid: 'id1.2',
              children: [
                { uuid: 'id1.2.1' }
              ]
            }
          ]
        },
        { uuid: 'id2' }
      ];
      const idArray = [];
      
      syncModule.collectBlockIds(blocks, idArray);
      
      expect(idArray).toEqual(['id1', 'id1.1', 'id1.2', 'id1.2.1', 'id2']);
    });

    test('handles empty tree', () => {
      const idArray = [];
      syncModule.collectBlockIds([], idArray);
      expect(idArray).toEqual([]);
    });

    test('handles null or undefined input', () => {
      const idArray = [];
      syncModule.collectBlockIds(null, idArray);
      expect(idArray).toEqual([]);
      
      syncModule.collectBlockIds(undefined, idArray);
      expect(idArray).toEqual([]);
    });

    test('skips blocks without uuid', () => {
      const blocks = [
        { uuid: 'id1' },
        { name: 'no-uuid' },
        { uuid: 'id2' },
        { uuid: null },
        { uuid: 'id3' }
      ];
      const idArray = [];
      
      syncModule.collectBlockIds(blocks, idArray);
      
      expect(idArray).toEqual(['id1', 'id2', 'id3']);
    });

    test('appends to existing array', () => {
      const blocks = [
        { uuid: 'id3' },
        { uuid: 'id4' }
      ];
      const idArray = ['id1', 'id2'];
      
      syncModule.collectBlockIds(blocks, idArray);
      
      expect(idArray).toEqual(['id1', 'id2', 'id3', 'id4']);
    });
  });

  describe('checkSyncStatus', () => {
    test('returns needsSync false when backend returns error', async () => {
      fetch.mockResolvedValueOnce({
        ok: false,
        status: 500
      });

      const result = await syncModule.checkSyncStatus();
      
      expect(result).toEqual({ needsSync: false });
      expect(window.KnowledgeGraphAPI.log.error).toHaveBeenCalledWith('Failed to get sync status from backend');
    });

    test('returns needsSync false when fetch throws', async () => {
      fetch.mockRejectedValueOnce(new Error('Network error'));

      const result = await syncModule.checkSyncStatus();
      
      expect(result).toEqual({ needsSync: false });
      expect(window.KnowledgeGraphAPI.log.error).toHaveBeenCalledWith(
        'Error checking sync status', 
        { error: 'Network error' }
      );
    });

    test('returns full sync when force_full_sync is true', async () => {
      fetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({
          force_full_sync: true,
          force_incremental_sync: false,
          incremental_sync_needed: false,
          true_full_sync_needed: false,
          sync_config: { enable_full_sync: false }
        })
      });

      const result = await syncModule.checkSyncStatus();
      
      expect(result).toEqual({ needsSync: true, syncType: 'full' });
    });

    test('returns full sync when true_full_sync_needed and enable_full_sync are true', async () => {
      fetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({
          force_full_sync: false,
          force_incremental_sync: false,
          incremental_sync_needed: false,
          true_full_sync_needed: true,
          sync_config: { enable_full_sync: true }
        })
      });

      const result = await syncModule.checkSyncStatus();
      
      expect(result).toEqual({ needsSync: true, syncType: 'full' });
    });

    test('returns incremental sync when force_incremental_sync is true', async () => {
      fetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({
          force_full_sync: false,
          force_incremental_sync: true,
          incremental_sync_needed: false,
          true_full_sync_needed: false,
          sync_config: { enable_full_sync: false }
        })
      });

      const result = await syncModule.checkSyncStatus();
      
      expect(result).toEqual({ needsSync: true, syncType: 'incremental' });
    });

    test('returns incremental sync when incremental_sync_needed is true', async () => {
      fetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({
          force_full_sync: false,
          force_incremental_sync: false,
          incremental_sync_needed: true,
          true_full_sync_needed: false,
          sync_config: { enable_full_sync: false }
        })
      });

      const result = await syncModule.checkSyncStatus();
      
      expect(result).toEqual({ needsSync: true, syncType: 'incremental' });
    });

    test('returns no sync needed when all flags are false', async () => {
      fetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({
          force_full_sync: false,
          force_incremental_sync: false,
          incremental_sync_needed: false,
          true_full_sync_needed: false,
          sync_config: { enable_full_sync: false }
        })
      });

      const result = await syncModule.checkSyncStatus();
      
      expect(result).toEqual({ needsSync: false });
    });

    test('prioritizes force flags over regular flags', async () => {
      fetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({
          force_full_sync: true,
          force_incremental_sync: true, // Both force flags true
          incremental_sync_needed: true,
          true_full_sync_needed: false,
          sync_config: { enable_full_sync: false }
        })
      });

      const result = await syncModule.checkSyncStatus();
      
      // Full sync takes priority
      expect(result).toEqual({ needsSync: true, syncType: 'full' });
    });

    test('ignores true_full_sync_needed when enable_full_sync is false', async () => {
      fetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({
          force_full_sync: false,
          force_incremental_sync: false,
          incremental_sync_needed: true,
          true_full_sync_needed: true, // This would trigger full sync
          sync_config: { enable_full_sync: false } // But it's disabled
        })
      });

      const result = await syncModule.checkSyncStatus();
      
      // Falls back to incremental sync
      expect(result).toEqual({ needsSync: true, syncType: 'incremental' });
    });
  });

  describe('updateSyncTimestamp', () => {
    test('delegates to KnowledgeGraphAPI', async () => {
      window.KnowledgeGraphAPI.updateSyncTimestamp.mockResolvedValue(true);
      
      const result = await syncModule.updateSyncTimestamp('incremental');
      
      expect(window.KnowledgeGraphAPI.updateSyncTimestamp).toHaveBeenCalledWith('incremental');
      expect(result).toBe(true);
    });

    test('uses incremental as default sync type', async () => {
      window.KnowledgeGraphAPI.updateSyncTimestamp.mockResolvedValue(true);
      
      await syncModule.updateSyncTimestamp();
      
      expect(window.KnowledgeGraphAPI.updateSyncTimestamp).toHaveBeenCalledWith('incremental');
    });
  });
});