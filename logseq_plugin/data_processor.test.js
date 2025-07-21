// Mock the global window object before importing
global.window = {};

// Import the module which will attach to window
require('./data_processor.js');

// Get reference to the data processor
const dataProcessor = window.KnowledgeGraphDataProcessor;

// Mock console.error to silence validation error messages
const originalConsoleError = console.error;
beforeAll(() => {
  console.error = jest.fn();
});

afterAll(() => {
  console.error = originalConsoleError;
});

describe('KnowledgeGraphDataProcessor', () => {
  describe('extractReferencesFromContent', () => {
    test('extracts page references', () => {
      const content = 'This links to [[Page A]] and [[Page B]]';
      const references = dataProcessor.extractReferencesFromContent(content);
      
      expect(references).toContainEqual({ type: 'page', name: 'Page A' });
      expect(references).toContainEqual({ type: 'page', name: 'Page B' });
      expect(references.filter(r => r.type === 'page')).toHaveLength(2);
    });

    test('extracts block references', () => {
      const content = 'This references ((block-123)) and ((block-456))';
      const references = dataProcessor.extractReferencesFromContent(content);
      
      expect(references).toContainEqual({ type: 'block', id: 'block-123' });
      expect(references).toContainEqual({ type: 'block', id: 'block-456' });
      expect(references.filter(r => r.type === 'block')).toHaveLength(2);
    });

    test('extracts hashtags', () => {
      const content = 'This has #tag1 and #tag-2 and #tag_3';
      const references = dataProcessor.extractReferencesFromContent(content);
      
      expect(references).toContainEqual({ type: 'tag', name: 'tag1' });
      expect(references).toContainEqual({ type: 'tag', name: 'tag-2' });
      expect(references).toContainEqual({ type: 'tag', name: 'tag_3' });
      expect(references.filter(r => r.type === 'tag')).toHaveLength(3);
    });

    test('extracts properties', () => {
      const content = 'author:: John Doe\ndate:: 2024-01-01';
      const references = dataProcessor.extractReferencesFromContent(content);
      
      expect(references).toContainEqual({ type: 'property', name: 'author' });
      expect(references).toContainEqual({ type: 'property', name: 'date' });
      expect(references.filter(r => r.type === 'property')).toHaveLength(2);
    });

    test('extracts mixed references', () => {
      const content = 'See [[Page A]] with #tag1 and ((block-123)) author:: John';
      const references = dataProcessor.extractReferencesFromContent(content);
      
      expect(references).toHaveLength(4);
      expect(references.filter(r => r.type === 'page')).toHaveLength(1);
      expect(references.filter(r => r.type === 'tag')).toHaveLength(1);
      expect(references.filter(r => r.type === 'block')).toHaveLength(1);
      expect(references.filter(r => r.type === 'property')).toHaveLength(1);
    });

    test('handles empty content', () => {
      expect(dataProcessor.extractReferencesFromContent('')).toEqual([]);
      expect(dataProcessor.extractReferencesFromContent(null)).toEqual([]);
      expect(dataProcessor.extractReferencesFromContent(undefined)).toEqual([]);
    });

    test('handles content with no references', () => {
      const content = 'This is plain text with no references.';
      const references = dataProcessor.extractReferencesFromContent(content);
      expect(references).toEqual([]);
    });
  });

  describe('validateBlockData', () => {
    test('validates valid block data', () => {
      const blockData = {
        id: 'block-123',
        content: 'Some content',
        created: '2024-01-01T00:00:00Z',
        updated: '2024-01-01T00:00:00Z',
        parent: null,
        children: [],
        references: []
      };

      const result = dataProcessor.validateBlockData(blockData);
      expect(result.valid).toBe(true);
      expect(result.errors).toEqual([]);
    });

    test('detects missing or invalid ID', () => {
      const blockData = {
        content: 'Some content',
        created: '2024-01-01T00:00:00Z',
        updated: '2024-01-01T00:00:00Z',
        parent: null,
        children: [],
        references: []
      };

      const result = dataProcessor.validateBlockData(blockData);
      expect(result.valid).toBe(false);
      expect(result.errors).toContain('Invalid block ID: undefined');
    });

    test('detects empty content', () => {
      const blockData = {
        id: 'block-123',
        content: '   ',
        created: '2024-01-01T00:00:00Z',
        updated: '2024-01-01T00:00:00Z',
        parent: null,
        children: [],
        references: []
      };

      const result = dataProcessor.validateBlockData(blockData);
      expect(result.valid).toBe(false);
      expect(result.errors).toContain('Block content is empty');
    });

    test('detects missing content field', () => {
      const blockData = {
        id: 'block-123',
        created: '2024-01-01T00:00:00Z',
        updated: '2024-01-01T00:00:00Z',
        parent: null,
        children: [],
        references: []
      };

      const result = dataProcessor.validateBlockData(blockData);
      expect(result.valid).toBe(false);
      expect(result.errors).toContain('Missing block content field');
    });

    test('detects invalid timestamps', () => {
      const blockData = {
        id: 'block-123',
        content: 'Some content',
        created: null,
        updated: 123,
        parent: null,
        children: [],
        references: []
      };

      const result = dataProcessor.validateBlockData(blockData);
      expect(result.valid).toBe(false);
      expect(result.errors).toContain('Invalid created timestamp: null');
      expect(result.errors).toContain('Invalid updated timestamp: 123');
    });

    test('detects invalid parent reference', () => {
      const blockData = {
        id: 'block-123',
        content: 'Some content',
        created: '2024-01-01T00:00:00Z',
        updated: '2024-01-01T00:00:00Z',
        parent: 123, // Should be string or null
        children: [],
        references: []
      };

      const result = dataProcessor.validateBlockData(blockData);
      expect(result.valid).toBe(false);
      expect(result.errors).toContain('Invalid parent reference: 123');
    });

    test('detects invalid children array', () => {
      const blockData = {
        id: 'block-123',
        content: 'Some content',
        created: '2024-01-01T00:00:00Z',
        updated: '2024-01-01T00:00:00Z',
        parent: null,
        children: 'not-an-array',
        references: []
      };

      const result = dataProcessor.validateBlockData(blockData);
      expect(result.valid).toBe(false);
      expect(result.errors).toContain('Children is not an array: not-an-array');
    });

    test('detects invalid child references', () => {
      const blockData = {
        id: 'block-123',
        content: 'Some content',
        created: '2024-01-01T00:00:00Z',
        updated: '2024-01-01T00:00:00Z',
        parent: null,
        children: ['child-1', 123, 'child-3'],
        references: []
      };

      const result = dataProcessor.validateBlockData(blockData);
      expect(result.valid).toBe(false);
      expect(result.errors).toContain('Invalid child reference at index 1: 123');
    });

    test('detects invalid references', () => {
      const blockData = {
        id: 'block-123',
        content: 'Some content',
        created: '2024-01-01T00:00:00Z',
        updated: '2024-01-01T00:00:00Z',
        parent: null,
        children: [],
        references: [{ name: 'missing-type' }, { type: 'page', name: 'valid' }]
      };

      const result = dataProcessor.validateBlockData(blockData);
      expect(result.valid).toBe(false);
      expect(result.errors).toContain('Missing reference type at index 0');
    });

    test('handles null block data', () => {
      const result = dataProcessor.validateBlockData(null);
      expect(result.valid).toBe(false);
      expect(result.errors).toContain('Block data is null or undefined');
    });
  });

  describe('validatePageData', () => {
    test('validates valid page data', () => {
      const pageData = {
        name: 'Test Page',
        created: '2024-01-01T00:00:00Z',
        updated: '2024-01-01T00:00:00Z',
        blocks: ['block-1', 'block-2']
      };

      const result = dataProcessor.validatePageData(pageData);
      expect(result.valid).toBe(true);
      expect(result.errors).toEqual([]);
    });

    test('detects missing or invalid name', () => {
      const pageData = {
        created: '2024-01-01T00:00:00Z',
        updated: '2024-01-01T00:00:00Z',
        blocks: []
      };

      const result = dataProcessor.validatePageData(pageData);
      expect(result.valid).toBe(false);
      expect(result.errors).toContain('Invalid page name: undefined');
    });

    test('detects invalid timestamps', () => {
      const pageData = {
        name: 'Test Page',
        created: null,
        updated: 123,
        blocks: []
      };

      const result = dataProcessor.validatePageData(pageData);
      expect(result.valid).toBe(false);
      expect(result.errors).toContain('Invalid created timestamp: null');
      expect(result.errors).toContain('Invalid updated timestamp: 123');
    });

    test('detects invalid blocks array', () => {
      const pageData = {
        name: 'Test Page',
        created: '2024-01-01T00:00:00Z',
        updated: '2024-01-01T00:00:00Z',
        blocks: 'not-an-array'
      };

      const result = dataProcessor.validatePageData(pageData);
      expect(result.valid).toBe(false);
      expect(result.errors).toContain('Blocks is not an array: not-an-array');
    });

    test('detects invalid block references', () => {
      const pageData = {
        name: 'Test Page',
        created: '2024-01-01T00:00:00Z',
        updated: '2024-01-01T00:00:00Z',
        blocks: ['block-1', 123, 'block-3']
      };

      const result = dataProcessor.validatePageData(pageData);
      expect(result.valid).toBe(false);
      expect(result.errors).toContain('Invalid block reference at index 1: 123');
    });

    test('handles null page data', () => {
      const result = dataProcessor.validatePageData(null);
      expect(result.valid).toBe(false);
      expect(result.errors).toContain('Page data is null or undefined');
    });
  });
});