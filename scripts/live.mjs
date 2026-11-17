// Starts the app for a browser: the built screens and the API on one port
// (`PORT`), every piece of data under `APP_DATA_DIR`. Installs the frontend
// packages when they are not there yet, and builds the screens first.
import { spawn, spawnSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
// corepack is a .cmd script on Windows, so it runs through the shell.
function pnpm(args) {
  const out = spawnSync(`corepack pnpm ${args}`, { cwd: root, stdio: 'inherit', shell: true });
  if (out.status !== 0) process.exit(out.status ?? 1);
}

if (!existsSync(join(root, 'node_modules', '.bin'))) pnpm('install --frozen-lockfile');
pnpm('build');

const host = spawn(
  'cargo',
  ['run', '-q', '--manifest-path', 'src-tauri/Cargo.toml', '--bin', 'web-host'],
  { cwd: root, stdio: 'inherit', env: { ...process.env, DIST_DIR: join(root, 'dist') } },
);
for (const signal of ['SIGINT', 'SIGTERM']) process.on(signal, () => host.kill(signal));
host.on('exit', (code) => process.exit(code ?? 1));
