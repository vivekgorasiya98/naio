import dotenv from 'dotenv';
import { syncToFtp, testFtpConnection, ftpConfigured, createConsoleFtpProgress } from '../services/ftp.js';

dotenv.config();

async function main() {
  if (!ftpConfigured()) {
    console.error('FTP not configured — copy .env.example to .env and fill FTP_* values');
    process.exit(1);
  }

  console.log('Testing FTP connection…');
  try {
    const test = await testFtpConnection();
    console.log(`  ✓ Connected to ${test.host} (${test.entries} entries in ${test.pwd})\n`);
  } catch (err) {
    console.error(`  ✗ ${err.message}\n`);
    console.error('Before syncing, unlock FTP in StackCP:');
    console.error('  1. Log in to StackCP');
    console.error('  2. Manage Hosting → select nm.c4compare.com package');
    console.error('  3. FTP panel → Unlock FTP (by time or your current IP)');
    console.error('  4. Run: npm run test-ftp\n');
    process.exit(1);
  }

  console.log('Uploading registry data…');
  const progress = createConsoleFtpProgress({ label: 'FTP sync' });
  try {
    const result = await syncToFtp({ onProgress: progress.onProgress });
    progress.done(result);
    console.log('');
  } catch (err) {
    progress.fail(err);
    throw err;
  }
}

main().catch((err) => {
  console.error(err.message);
  process.exit(1);
});
