import bcrypt from 'bcryptjs';
import jwt from 'jsonwebtoken';
import { config } from '../config.js';

let passwordHash = null;

export async function initAuth() {
  if (!config.admin.password) {
    if (config.nodeEnv === 'production') {
      throw new Error('ADMIN_PASSWORD is required in production');
    }
    passwordHash = await bcrypt.hash('admin', 10);
    console.warn('[auth] using default dev password "admin" — set ADMIN_PASSWORD in .env');
    return;
  }
  passwordHash = await bcrypt.hash(config.admin.password, 12);
}

export async function verifyLogin(username, password) {
  if (username !== config.admin.username) {
    return null;
  }
  const ok = await bcrypt.compare(password, passwordHash);
  if (!ok) return null;
  const token = jwt.sign({ sub: username, role: 'admin' }, config.admin.jwtSecret, {
    expiresIn: config.admin.jwtExpires,
  });
  return { token, expiresIn: config.admin.jwtExpires };
}

export function requireAdmin(request, reply, done) {
  const header = request.headers.authorization || '';
  const token = header.startsWith('Bearer ') ? header.slice(7) : null;
  if (!token) {
    reply.code(401).send({ error: 'missing bearer token' });
    return;
  }
  try {
    const payload = jwt.verify(token, config.admin.jwtSecret);
    request.admin = payload;
    done();
  } catch {
    reply.code(401).send({ error: 'invalid or expired token' });
  }
}
