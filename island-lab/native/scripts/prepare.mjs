import { mkdir, copyFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { dirname, resolve, join } from 'node:path';

// Keep one canonical copy of the web UI. Tauri bundles a generated copy,
// so the Rust build does not recursively package source or its own target/.
const here = dirname(fileURLToPath(import.meta.url));
const source = resolve(here, '../..');
const target = resolve(here, '../dist');
await mkdir(target, { recursive: true });
for (const file of ['index.html', 'styles.css', 'app.js']) {
  await copyFile(join(source, file), join(target, file));
}
console.log('Island Lab: prepared static frontend in native/dist');
