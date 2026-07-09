import dotenv from 'dotenv';
import { config } from '../config.js';
import { ftpConfigured } from '../services/ftp.js';

dotenv.config();

const checks = [
  ['API_URL', config.apiUrl],
  ['FILES_URL', config.filesUrl],
  ['ADMIN_PASSWORD', config.admin.password ? '(set)' : 'MISSING'],
  ['JWT_SECRET', config.admin.jwtSecret.length >= 32 ? '(set)' : 'MISSING'],
  ['FTP_HOST', config.ftp.host || 'MISSING'],
  ['FTP_USER', config.ftp.user || 'MISSING'],
  ['FTP_PASSWORD', config.ftp.password ? '(set)' : 'MISSING'],
  ['FTP_REMOTE_DIR', config.ftp.remoteDir],
  ['DATA_DIR', config.dataDir],
  ['SERVERLESS', config.isServerless ? 'yes (remote reads enabled)' : 'no'],
  ['NIAO_VERSION', config.niaoVersion],
];

console.log('\n.env configuration check\n');
for (const [key, val] of checks) {
  const ok = !String(val).includes('MISSING');
  console.log(`  ${ok ? '✓' : '✗'} ${key}: ${val}`);
}
console.log(`\n  FTP ready: ${ftpConfigured() ? 'yes' : 'no'}`);
console.log(`  Auto-sync: ${config.ftp.autoSync ? 'yes' : 'no'}\n`);

if (checks.some(([, v]) => String(v).includes('MISSING'))) {
  process.exit(1);
}
