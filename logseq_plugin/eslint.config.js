module.exports = [
  {
    // Main source files (browser-based)
    files: ["index.js", "api.js", "data_processor.js", "sync.js"],
    languageOptions: {
      ecmaVersion: 2021,
      sourceType: "script",
      globals: {
        // Browser globals
        window: "readonly",
        document: "readonly",
        console: "readonly",
        fetch: "readonly",
        setTimeout: "readonly",
        AbortSignal: "readonly",
        
        // Logseq globals
        logseq: "readonly",
        
        // Our module globals
        KnowledgeGraphAPI: "readonly",
        KnowledgeGraphDataProcessor: "readonly", 
        KnowledgeGraphSync: "readonly",
        
        // Functions exposed by modules
        processBatch: "readonly",
        processTimestampQueue: "readonly",
        sendBatchToBackend: "readonly",
        timestampQueue: "readonly",
        
        // Wrappers in index.js
        processBlockData: "readonly",
        processPageData: "readonly",
        validateBlockData: "readonly",
        validatePageData: "readonly",
        checkBackendAvailabilityWithRetry: "readonly"
      }
    },
    rules: {
      "no-unused-vars": ["error", {
        "vars": "all",
        "args": "after-used",
        "argsIgnorePattern": "^_",
        "caughtErrors": "none"
      }],
      "no-undef": "error"
    }
  },
  {
    // Test files
    files: ["*.test.js"],
    languageOptions: {
      ecmaVersion: 2021,
      sourceType: "script",
      globals: {
        // Jest globals
        global: "readonly",
        require: "readonly",
        jest: "readonly",
        describe: "readonly",
        test: "readonly",
        expect: "readonly",
        beforeAll: "readonly",
        afterAll: "readonly",
        beforeEach: "readonly",
        afterEach: "readonly",
        console: "readonly",
        
        // Our test setup
        window: "writable",
        fetch: "writable",
        logseq: "writable"
      }
    },
    rules: {
      "no-unused-vars": ["error", {
        "vars": "all",
        "args": "after-used"
      }],
      "no-undef": "error"
    }
  },
  {
    // Node.js files (config and tools)
    files: ["eslint.config.js", "jest.config.js", "stress_test_generator.js"],
    languageOptions: {
      ecmaVersion: 2021,
      sourceType: "commonjs",
      globals: {
        // Node.js globals
        module: "readonly",
        require: "readonly",
        process: "readonly",
        console: "readonly",
        __dirname: "readonly",
        __filename: "readonly"
      }
    },
    rules: {
      "no-unused-vars": ["error", {
        "vars": "all",
        "args": "after-used"
      }],
      "no-undef": "error"
    }
  }
];