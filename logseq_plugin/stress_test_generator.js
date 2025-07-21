#!/usr/bin/env node

/**
 * Stress Test Data Generator for Logseq Knowledge Graph
 * 
 * Creates 2000 pages with 10 blocks each for performance testing.
 * Uses "STRESS_" prefix for easy cleanup.
 */

const fs = require('fs');
const path = require('path');

// Update this path to point to your test graph's pages directory
const GRAPH_DIR = path.join(__dirname, '..', 'logseq_databases', 'dummy_graph', 'pages');
const PAGE_COUNT = 2000;
const BLOCKS_PER_PAGE = 10;
const PREFIX = 'STRESS_';

function generateRandomText(length = 20) {
  const chars = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789 ';
  let result = '';
  for (let i = 0; i < length; i++) {
    result += chars.charAt(Math.floor(Math.random() * chars.length));
  }
  return result.trim();
}

function generatePageContent(pageNumber) {
  let content = `# ${PREFIX}Page ${pageNumber}\n\n`;
  content += `This is stress test page ${pageNumber} with ${BLOCKS_PER_PAGE} blocks.\n\n`;
  
  for (let blockNum = 1; blockNum <= BLOCKS_PER_PAGE; blockNum++) {
    const randomText = generateRandomText(Math.floor(Math.random() * 50) + 20);
    content += `- Block ${blockNum}: ${randomText}\n`;
    
    // Add some variety - every 3rd block gets a reference
    if (blockNum % 3 === 0 && pageNumber > 1) {
      const refPageNum = Math.max(1, pageNumber - Math.floor(Math.random() * 10));
      content += `  - References [[${PREFIX}Page ${refPageNum}]]\n`;
    }
    
    // Every 5th block gets a tag
    if (blockNum % 5 === 0) {
      content += `  - #${PREFIX}tag${Math.floor(Math.random() * 20) + 1}\n`;
    }
  }
  
  return content;
}

function createStressTestData() {
  console.log(`Creating ${PAGE_COUNT} stress test pages in ${GRAPH_DIR}`);
  
  if (!fs.existsSync(GRAPH_DIR)) {
    console.error(`Graph directory not found: ${GRAPH_DIR}`);
    process.exit(1);
  }
  
  const startTime = Date.now();
  let created = 0;
  
  for (let pageNum = 1; pageNum <= PAGE_COUNT; pageNum++) {
    const filename = `${PREFIX}Page_${pageNum.toString().padStart(4, '0')}.md`;
    const filepath = path.join(GRAPH_DIR, filename);
    
    const content = generatePageContent(pageNum);
    
    try {
      fs.writeFileSync(filepath, content, 'utf8');
      created++;
      
      if (created % 100 === 0) {
        console.log(`Created ${created}/${PAGE_COUNT} pages...`);
      }
    } catch (error) {
      console.error(`Error creating ${filename}:`, error.message);
    }
  }
  
  const duration = (Date.now() - startTime) / 1000;
  const totalBlocks = created * BLOCKS_PER_PAGE;
  
  console.log(`\n✅ Stress test data creation complete!`);
  console.log(`📄 Created: ${created} pages`);
  console.log(`📝 Total blocks: ${totalBlocks}`);
  console.log(`⏱️  Generation time: ${duration.toFixed(2)} seconds`);
  console.log(`\n🧹 To cleanup later, run:`);
  console.log(`   rm ${GRAPH_DIR}/${PREFIX}*.md`);
}

// Cleanup function
function cleanupStressTestData() {
  console.log(`Cleaning up stress test data from ${GRAPH_DIR}`);
  
  const files = fs.readdirSync(GRAPH_DIR)
    .filter(file => file.startsWith(PREFIX) && file.endsWith('.md'));
  
  let deleted = 0;
  for (const file of files) {
    try {
      fs.unlinkSync(path.join(GRAPH_DIR, file));
      deleted++;
    } catch (error) {
      console.error(`Error deleting ${file}:`, error.message);
    }
  }
  
  console.log(`🧹 Deleted ${deleted} stress test files`);
}

// Command line interface
const command = process.argv[2];

if (command === 'cleanup') {
  cleanupStressTestData();
} else if (command === 'create' || !command) {
  createStressTestData();
} else {
  console.log('Usage: node stress_test_generator.js [create|cleanup]');
  console.log('  create  - Generate stress test data (default)');
  console.log('  cleanup - Remove stress test data');
  process.exit(1);
}