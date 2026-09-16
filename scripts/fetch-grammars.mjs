import { createHash } from 'node:crypto';
import { copyFileSync, existsSync, mkdirSync, readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = dirname(fileURLToPath(import.meta.url + '/..'));

const GRAMMARS = {
  'tree-sitter-rust.wasm': '4409921a70d0aa5bec7d1d7ce809a557a8ee1cf6ace901e3ac6a76e62cfea903',
  'tree-sitter-go.wasm': '9963ca89b616eaf04b08a43bc1fb0f07b85395bec313330851f1f1ead2f755b6',
  'tree-sitter-solidity.wasm': '160745e470f234cae903a9ba445d19e758d0b02e1197401fc765976c6254d2b6',
  'tree-sitter-typescript.wasm': '8515404dceed38e1ed86aa34b09fcf3379fff1b4ff9dd3967bcd6d1eb5ac3d8f',
  'tree-sitter-tsx.wasm': '6aa3b2c70e76f5d48eafef1093e9c4de383e13f2fdde2f4e9b98a378f6a8f1b6',
  'tree-sitter-python.wasm': '9056d0fb0c337810d019fae350e8167786119da98f0f282aceae7ab89ee8253b',
  'tree-sitter.wasm': 'f38dcc4b43b818f9a0785bc1c6d5611a75ac4cdd428ff3f02757c34ca4e46d7f',
};

const SOURCES = {
  'tree-sitter.wasm': join(root, 'node_modules', 'web-tree-sitter', 'tree-sitter.wasm'),
};

for (const g of Object.keys(GRAMMARS)) {
  if (!SOURCES[g]) SOURCES[g] = join(root, 'node_modules', 'tree-sitter-wasms', 'out', g);
}

function sha256(file) {
  return createHash('sha256').update(readFileSync(file)).digest('hex');
}

function fail(msg) {
  console.error(`fetch-grammars: ${msg}`);
  process.exit(1);
}

const checkOnly = process.argv.includes('--check');
const update = process.argv.includes('--update');
const destDir = join(root, 'resources', 'grammars');

if (!checkOnly) mkdirSync(destDir, { recursive: true });
mkdirSync(join(root, 'resources'), { recursive: true });

let failures = 0;
for (const [name, expected] of Object.entries(GRAMMARS)) {
  const dest = name === 'tree-sitter.wasm' ? join(root, 'resources', 'tree-sitter.wasm') : join(destDir, name);

  if (checkOnly) {
    if (!existsSync(dest)) {
      console.error(`MISSING ${name}`);
      failures++;
      continue;
    }
    const got = sha256(dest);
    if (got !== expected && !update) {
      console.error(`CHECKSUM MISMATCH ${name}: ${got}`);
      failures++;
    } else {
      console.log(`ok ${name}`);
    }
    continue;
  }

  const src = SOURCES[name];
  if (!existsSync(src)) fail(`source not found for ${name}: ${src}. Run npm ci first.`);
  const got = sha256(src);
  if (got !== expected && !update) {
    fail(`checksum mismatch for ${name} from node_modules: ${got}\nRefusing to vendor. If the bump is intentional, rerun with --update and commit the new hashes.`);
  }
  copyFileSync(src, dest);
  console.log(`${update && got !== expected ? 're-pinned' : 'vendored'} ${name} ${got}`);
}

if (failures > 0) fail(`${failures} checksum failure(s)`);
console.log('done.');
