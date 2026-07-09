import dotenv from 'dotenv';
import { testFtpConnection, ftpConfigured } from '../services/ftp.js';
import { config } from '../config.js';

dotenv.config();

async function main() {
  console.log('\nStackCP FTP connection test\n');
  console.log(`  host: ${config.ftp.host}`);
  console.log(`  user: ${config.ftp.user}`);
  console.log(`  remote: ${config.ftp.remoteDir}`);
  console.log(`  configured: ${ftpConfigured()}\n`);

  if (!ftpConfigured()) {
    console.error('  ✗ FTP credentials missing in .env\n');
    process.exit(1);
  }

  try {
    const result = await testFtpConnection();
    console.log('  ✓ Connected successfully');
    console.log(`  pwd: ${result.pwd}`);
    console.log(`  entries: ${result.entries}`);
    console.log(`  sample: ${result.sample.join(', ')}\n`);
  } catch (err) {
    console.error(`  ✗ ${err.message}\n`);
    console.error('  StackCP checklist:');
    console.error('    1. FTP_HOST=ftp.stackcp.com');
    console.error('    2. Unlock FTP in StackCP → Manage Hosting → Unlock FTP');
    console.error('    3. Use credentials from StackCP FTP Details panel\n');
    process.exit(1);
  }
}

main();
