import path from 'node:path';
import { config } from '../config.js';

export const publicDir = path.join(config.root, 'src', 'public');
export const assetsDir = path.join(publicDir, 'assets');
