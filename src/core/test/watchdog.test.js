import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { spawn } from 'node:child_process';
import net from 'node:net';
import { fileURLToPath } from 'node:url';

async function freePort() {
  const socket = net.createServer();
  await new Promise(resolve => socket.listen(0, '127.0.0.1', resolve));
  const port = socket.address().port;
  await new Promise(resolve => socket.close(resolve));
  return port;
}

// 看门狗契约：壳进程（BUDDY_PARENT_PID）消失后，sidecar 必须自行优雅退出，
// 而不是变成占着端口的孤儿进程。用立刻退出的占位进程模拟「已死的壳」。
test('sidecar exits by itself when the shell process is gone', { timeout: 30000 }, async () => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'buddy-watchdog-'));
  const placeholder = spawn(process.execPath, ['-e', 'process.exit(0)'], { stdio: 'ignore' });
  await new Promise(resolve => placeholder.once('exit', resolve));
  const port = await freePort();
  const child = spawn(process.execPath, ['src/main.js'], {
    cwd: fileURLToPath(new URL('..', import.meta.url)),
    env: { ...process.env, BUDDY_DATA_DIR: root, BUDDY_PARENT_PID: String(placeholder.pid), BUDDY_NO_SYNC: '1', BUDDY_PORT: String(port) },
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  let logs = ''; child.stdout.on('data', x => logs += x); child.stderr.on('data', x => logs += x);
  const code = await new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error('看门狗未触发退出: ' + logs)), 25000);
    child.once('exit', (code) => { clearTimeout(timer); resolve(code); });
  });
  assert.equal(code, 0, '父进程消失时必须走优雅退出路径');
  const status = JSON.parse(await fs.readFile(path.join(root, 'status.json'), 'utf8'));
  assert.equal(status.phase, 'stopped');
});
