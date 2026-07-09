import { buildApp } from './app.js';
import { config } from './config.js';

const app = await buildApp();

try {
  await app.listen({ port: config.port, host: config.host });
  app.log.info(
    `Niao registry API: ${config.apiUrl} | files: ${config.filesUrl} | data: ${config.dataDir} (bind ${config.host}:${config.port})`,
  );
} catch (err) {
  app.log.error(err);
  process.exit(1);
}
