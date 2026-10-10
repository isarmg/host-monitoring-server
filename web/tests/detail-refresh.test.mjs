import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { test } from 'node:test';
import { transform } from 'rolldown/experimental';

// Exercise the actual component's request/effect and rendered-tree decisions without
// a browser. Child components stay opaque: their stable presence and key determine
// whether React can retain the selected monitoring category and history window.
const source = await readFile(new URL('../src/InstanceDetails.tsx', import.meta.url), 'utf8');
const { code } = await transform('InstanceDetails.tsx', source, { jsx: { runtime: 'classic' } });
const componentBody = code.replace(/^import[\s\S]*?;\n/gm, '').replace('export function InstanceDetails', 'function InstanceDetails');
const uiNames = ['Button', 'EmptyState', 'ErrorState', 'FormField', 'LoadingState', 'TextField', 'HostDetails'];
const ui = Object.fromEntries(uiNames.map(name => [name, name]));
const createElement = (type, props, ...children) => ({ type, props: { ...props, children } });
const t = (_zh, en) => en;

function harness() {
  const states = [], refs = [], effects = [], requests = [];
  let stateIndex = 0, refIndex = 0, effectIndex = 0, tree, dirty = true;
  const client = { request(_path, _validator, options) {
    return new Promise((resolve, reject) => requests.push({ resolve, reject, signal: options.signal }));
  } };
  const useState = initial => {
    const index = stateIndex++;
    if (!(index in states)) states[index] = typeof initial === 'function' ? initial() : initial;
    return [states[index], value => {
      const next = typeof value === 'function' ? value(states[index]) : value;
      if (!Object.is(next, states[index])) { states[index] = next; dirty = true; }
    }];
  };
  const useRef = initial => refs[refIndex++] ??= { current: initial };
  const useEffect = (callback, deps) => {
    const index = effectIndex++, previous = effects[index];
    if (!previous || deps.some((value, i) => !Object.is(value, previous.deps[i]))) {
      previous?.cleanup?.();
      effects[index] = { deps, callback };
    }
  };
  const names = ['React', 't', ...uiNames, 'useState', 'useRef', 'useEffect', 'useAdminApplication', 'errorRequestId', 'isFocusedInstance', 'isNoContent', 'LIST_REQUEST_BUDGET'];
  const Component = new Function(...names, `${componentBody}\nreturn InstanceDetails;`)(
    { createElement, Fragment: 'Fragment' }, t, ...Object.values(ui), useState, useRef, useEffect,
    () => ({ client, notify() {} }), error => error.requestId, () => true, () => true, {},
  );
  const props = { instanceId: 'instance-a', refreshSignal: 0, changed() {}, removed() {} };
  async function flush() {
    for (let iteration = 0; iteration < 10; iteration++) {
      if (dirty) {
        dirty = false; stateIndex = 0; refIndex = 0; effectIndex = 0;
        tree = Component(props);
        for (const effect of effects) if (effect.callback) {
          const callback = effect.callback; effect.callback = null; effect.cleanup = callback();
        }
      }
      await new Promise(resolve => setImmediate(resolve));
      if (!dirty) return tree;
    }
    throw new Error('component did not settle');
  }
  return { requests, flush, async refresh() { props.refreshSignal++; dirty = true; return flush(); } };
}

function nodes(tree, type, path = 'root') {
  if (!tree || typeof tree !== 'object') return [];
  const matches = tree.type === type ? [{ ...tree, path }] : [];
  return matches.concat((tree.props?.children ?? []).flatMap((child, index) =>
    nodes(child, type, `${path}/${index}`)));
}
const response = {
  instances: [{ request_id: 'request-a', instance_id: 'instance-a', display_name: 'Test machine', status: 'active' }],
  hosts: [{ id: 'instance-a', data_error: null }],
};

test('failed details refresh keeps the monitoring subtree mounted and visibly reports the failure', async () => {
  const h = harness();
  await h.flush(); h.requests[0].resolve(response);
  const initial = nodes(await h.flush(), 'HostDetails');
  assert.equal(initial.length, 1);
  const pending = nodes(await h.refresh(), 'HostDetails');
  assert.equal(pending.length, 1);
  assert.equal(pending[0].path, initial[0].path);
  h.requests[1].reject({ requestId: 'refresh-offline' });
  const failed = await h.flush();
  const retained = nodes(failed, 'HostDetails');
  assert.equal(retained.length, 1, 'a transient refresh failure must retain the last-good monitoring subtree');
  assert.equal(retained[0].props.key, initial[0].props.key);
  assert.equal(retained[0].path, initial[0].path);
  assert.equal(nodes(failed, 'ErrorState')[0].props.requestId, 'refresh-offline');
  assert.match(nodes(failed, 'ErrorState')[0].props.children.join(''), /last successful/i);
  await h.refresh(); h.requests[2].resolve(response);
  const recovered = await h.flush();
  assert.equal(nodes(recovered, 'HostDetails')[0].path, initial[0].path);
  assert.equal(nodes(recovered, 'ErrorState').length, 0);
});

test('initial details failure shows an error without fabricating cached monitoring data', async () => {
  const h = harness();
  await h.flush(); h.requests[0].reject({ requestId: 'first-offline' });
  const failed = await h.flush();
  assert.equal(nodes(failed, 'HostDetails').length, 0);
  assert.equal(nodes(failed, 'ErrorState')[0].props.requestId, 'first-offline');
  await h.refresh(); h.requests[1].resolve(response);
  assert.equal(nodes(await h.flush(), 'HostDetails').length, 1);
});
