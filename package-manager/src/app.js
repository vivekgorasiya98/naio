import Fastify from 'fastify';
import cors from '@fastify/cors';
import helmet from '@fastify/helmet';
import rateLimit from '@fastify/rate-limit';
import multipart from '@fastify/multipart';
import fastifyStatic from '@fastify/static';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { config } from './config.js';
import { ensureDirs } from './services/storage.js';
import { initAuth } from './services/auth.js';
import { connectMongo, mongoEnabled } from './db/mongo.js';
import { registryRoutes } from './routes/registry.js';
import { adminRoutes } from './routes/admin.js';

const __dirname = path.dirname(fileURLToPath(import.meta.url));

export async function buildApp() {
  const app = Fastify({
    logger: config.nodeEnv !== 'test',
    trustProxy: true,
  });

  await app.register(helmet, {
    contentSecurityPolicy: false,
  });

  await app.register(cors, {
    origin: true,
    methods: ['GET', 'POST', 'PUT', 'PATCH', 'DELETE', 'OPTIONS'],
  });

  await app.register(rateLimit, {
    max: 300,
    timeWindow: '1 minute',
  });

  await app.register(multipart, {
    limits: { fileSize: 50 * 1024 * 1024 },
  });

  app.addContentTypeParser('application/json', { parseAs: 'string' }, (req, body, done) => {
    try {
      done(null, body ? JSON.parse(body) : {});
    } catch (err) {
      done(err);
    }
  });

  await ensureDirs();
  if (mongoEnabled()) {
    try {
      await connectMongo();
      app.log.info('MongoDB connected');
    } catch (err) {
      app.log.warn(`MongoDB unavailable — using filesystem fallback: ${err.message}`);
    }
  }
  await initAuth();

  await registryRoutes(app);
  await adminRoutes(app);

  await app.register(fastifyStatic, {
    root: path.join(__dirname, 'public'),
    prefix: '/admin/',
    decorateReply: false,
  });

  return app;
}
