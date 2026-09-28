const assert = require('assert');
const fs = require('fs');
const path = require('path');

// --- Webapp Utility Implementations Under Test (mirrors src/js/custom.js) ---

// 1. String.prototype.format utility
if (!String.prototype.format) {
  Object.assign(String.prototype, {
    format() {
      const args = arguments;
      return this.replace(/{(\d+)}/g, function (match, number) {
        return typeof args[number] !== 'undefined' ? args[number] : match;
      });
    },
  });
}

// 2. isEnabled utility
function isEnabled(val) {
  return val != undefined && typeof val === 'string' && Boolean(val.match('[Yy1]'));
}

// 3. concatenateOptions utility
function concatenateOptions(options) {
  let commandLine = ' ';
  for (const [option, value] of Object.entries(options)) {
    if (option !== 'n' && option !== 'o') {
      commandLine += `-${option} `;
      if (value !== true) {
        commandLine += `${value} `;
      }
    }
  }
  return commandLine;
}

// 4. JSON parsing and decoding helpers
function parseCommandResponse(response) {
  return typeof response === 'string' ? JSON.parse(response) : response;
}

function parseStatsMessage(rawMessage) {
  const outer = typeof rawMessage === 'string' ? JSON.parse(rawMessage) : rawMessage;
  if (!outer.message) {
    throw new Error('Missing inner message property');
  }
  const inner = JSON.parse(outer.message);
  return {
    type: outer.type,
    class: outer.class,
    sent_time: outer.sent_time,
    free_iram: inner.free_iram,
    free_spiram: inner.free_spiram,
    ntasks: inner.ntasks,
    tasks: inner.tasks || [],
  };
}

// Test Runner Harness
let testsPassed = 0;

function runTest(name, fn) {
  try {
    fn();
    console.log(`  PASSED: ${name}`);
    testsPassed++;
  } catch (err) {
    console.error(`  FAILED: ${name}`);
    console.error(err);
    process.exit(1);
  }
}

console.log('Running webapp unit test suite...');

// Basic Array assertion (preserving original harness intent)
runTest('Array #indexOf returns -1 when element is not present', () => {
  assert.strictEqual([1, 2, 3].indexOf(4), -1);
});

// Unit Test: String formatting
runTest('String.prototype.format replaces positional tokens correctly', () => {
  assert.strictEqual('Device: {0}, Port: {1}'.format('Squeezelite', 80), 'Device: Squeezelite, Port: 80');
  assert.strictEqual('Unmatched token {2} remains {2}'.format('first'), 'Unmatched token {2} remains {2}');
});

// Unit Test: isEnabled configuration flag checker
runTest('isEnabled validates configuration truthy and falsy strings', () => {
  assert.strictEqual(isEnabled('Y'), true);
  assert.strictEqual(isEnabled('y'), true);
  assert.strictEqual(isEnabled('1'), true);
  assert.strictEqual(isEnabled('yes'), true);
  assert.strictEqual(isEnabled('0'), false);
  assert.strictEqual(isEnabled('N'), false);
  assert.strictEqual(isEnabled('no'), false);
  assert.strictEqual(isEnabled(''), false);
  assert.strictEqual(isEnabled(undefined), false);
  assert.strictEqual(isEnabled(null), false);
  assert.strictEqual(isEnabled(123), false);
});

// Unit Test: concatenateOptions CLI generator
runTest('concatenateOptions serializes options into CLI arguments', () => {
  const opts = { b: '115200', d: true, n: 'ignored', o: 'skip' };
  const cli = concatenateOptions(opts);
  assert.strictEqual(cli, ' -b 115200 -d ');
});

// Unit Test: JSON command response parsing
runTest('parseCommandResponse parses JSON strings and handles pre-parsed objects', () => {
  const jsonStr = '{"command":"restart","status":"ok","code":0}';
  const parsed = parseCommandResponse(jsonStr);
  assert.strictEqual(parsed.command, 'restart');
  assert.strictEqual(parsed.status, 'ok');
  assert.strictEqual(parsed.code, 0);

  const directObj = { result: 'success' };
  assert.deepStrictEqual(parseCommandResponse(directObj), directObj);
});

// Unit Test: Mock stats message JSON parsing
runTest('parseStatsMessage decodes nested stats JSON payload', () => {
  const mockPath = path.join(__dirname, '..', 'mock', 'messages_testing.json');
  if (fs.existsSync(mockPath)) {
    const rawData = fs.readFileSync(mockPath, 'utf8');
    const messages = JSON.parse(rawData);
    assert(Array.isArray(messages), 'Messages payload should be an array');
    assert(messages.length > 0, 'Messages payload should not be empty');

    const firstStat = parseStatsMessage(messages[0]);
    assert.strictEqual(firstStat.type, 'MESSAGING_INFO');
    assert.strictEqual(firstStat.class, 'MESSAGING_CLASS_STATS');
    assert(typeof firstStat.free_iram === 'number');
    assert(typeof firstStat.free_spiram === 'number');
    assert(Array.isArray(firstStat.tasks));
    assert(firstStat.tasks.length > 0);
  }
});

console.log(`\nAll ${testsPassed} webapp unit tests passed successfully.`);
process.exit(0);