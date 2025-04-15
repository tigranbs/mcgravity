#!/usr/bin/env bun

import { readdirSync } from 'fs';
import { join } from 'path';
import { spawnSync } from 'child_process';

const TEST_TIMEOUT = '30000'; // 30 seconds timeout for integration tests

// Get test files
const testDir = join(import.meta.dir, 'integration');
const testFiles = readdirSync(testDir)
  .filter((file) => file.endsWith('.test.ts'))
  .map((file) => join(testDir, file));

console.log(`Running ${testFiles.length} integration tests...\n`);

let failedTests = 0;

for (const testFile of testFiles) {
  console.log(`Running test: ${testFile}`);
  const result = spawnSync('bun', ['test', testFile], {
    stdio: 'inherit', // Show output directly
    env: {
      ...process.env,
      BUN_TEST_TIMEOUT: TEST_TIMEOUT,
    },
  });

  if (result.status !== 0) {
    console.error(`Test failed: ${testFile}`);
    failedTests++;
  }
}

// Exit with error if any tests failed
if (failedTests > 0) {
  console.error(`\n${failedTests} test(s) failed!`);
  process.exit(1);
}

console.log('\nAll integration tests passed!');
