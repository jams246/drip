import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';

const [executable, html] = await Promise.all([
    readFile(new URL('../dist/production/drip.exe', import.meta.url)),
    readFile(new URL('../dist/desktop/index.html', import.meta.url)),
]);

assert(executable.includes(html), 'Production executable must contain the complete frontend bundle.');
console.log('Production frontend embedding check passed.');
