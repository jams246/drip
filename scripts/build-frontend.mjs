import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFile, readdir, writeFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { build } from 'vite';
import { viteSingleFile } from 'vite-plugin-singlefile';

const root = fileURLToPath(new URL('../src/desktop/', import.meta.url));
const output = new URL('../dist/desktop/', import.meta.url);

await build({
    configFile: false,
    root,
    plugins: [viteSingleFile()],
    build: { outDir: fileURLToPath(output), emptyOutDir: true },
});

assert.deepEqual(await readdir(output), ['index.html'], 'Frontend must build into one self-contained HTML file.');
const index = new URL('index.html', output);
const html = await readFile(index, 'utf8');
const scripts = [...html.matchAll(/<script\b([^>]*)>([\s\S]*?)<\/script>/g)];
assert(scripts.length > 0, 'Frontend must contain its React bundle.');
assert(scripts.every(([, attributes]) => !/\bsrc\s*=/.test(attributes)), 'Scripts must be embedded.');
const hashes = scripts.map(([, , code]) => `'sha256-${createHash('sha256').update(code).digest('base64')}'`);
const policy = `default-src 'none'; script-src ${hashes.join(' ')}; style-src 'unsafe-inline'; img-src data:; font-src data:`;
assert(html.includes('http-equiv="Content-Security-Policy"'), 'Frontend must declare its content security policy.');
await writeFile(index, html.replace(/(http-equiv="Content-Security-Policy" content=")[^"]*(")/, `$1${policy}$2`));
