import assert from 'node:assert/strict';
import { test } from 'node:test';
import { displayLabel } from '../src/display-labels.ts';

test('unknown fields and vendors cannot resolve inherited object properties', () => {
  assert.equal(displayLabel('online'), 'Online');
  assert.equal(displayLabel('gpu.nvidia'), 'NVIDIA GPU collection');
  assert.equal(displayLabel('hardware.memory'), 'Memory module inventory');
  assert.equal(displayLabel('hardware.audio'), 'Audio device inventory');
  for (const value of ['__proto__', 'constructor', 'toString', 'gpu.constructor', 'system.__proto__']) {
    assert.equal(displayLabel(value), 'Unrecognized item');
  }
});
