import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createServer } from '../src/server.js';

const models = [{ id: 'opencode/test-free', name: 'Test', context: 1000, output: 500 }];

async function listen(server) {
  await new Promise(r => server.listen(0, '127.0.0.1', r));
  return `http://127.0.0.1:${server.address().port}`;
}

test('admin shutdown responds first, then triggers exactly one graceful stop', async () => {
  let stops = 0;
  const server = createServer({ key: 'test', getModels: () => models, status: () => ({}), onShutdown: () => { stops += 1; } });
  const base = await listen(server);
  try {
    const response = await fetch(base + '/admin/shutdown', { method: 'POST', headers: { Authorization: 'Bearer test' } });
    assert.deepEqual(await response.json(), { ok: true });
    await new Promise(r => setImmediate(() => setImmediate(r)));
    assert.equal(stops, 1, 'The shell stop request must reach the same shutdown path as a signal');
    const denied = await fetch(base + '/admin/shutdown', { method: 'POST', headers: { Authorization: 'Bearer wrong' } });
    assert.equal(denied.status, 401);
  } finally { server.closeAllConnections?.(); server.close(); }
});
