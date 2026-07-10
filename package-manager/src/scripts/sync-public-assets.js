import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const srcAssets = path.join(root, 'src', 'public', 'assets');
const destAssets = path.join(root, 'public', 'assets');

if (!fs.existsSync(srcAssets)) {
  console.error('sync-public-assets: missing', srcAssets);
  process.exit(1);
}

fs.mkdirSync(path.dirname(destAssets), { recursive: true });
fs.cpSync(srcAssets, destAssets, { recursive: true });
console.log('sync-public-assets: copied to public/assets');
