import { MongoClient } from 'mongodb';
import { config } from '../config.js';

let client;
let db;
let connectPromise;
let connectFailed = false;

export function mongoEnabled() {
  return Boolean(config.mongo.uri);
}

export async function connectMongo() {
  if (!mongoEnabled()) return null;
  if (connectFailed) return null;
  if (db) return db;
  if (!connectPromise) {
    connectPromise = (async () => {
      client = new MongoClient(config.mongo.uri, {
        maxPoolSize: 10,
        serverSelectionTimeoutMS: 15_000,
      });
      await client.connect();
      db = client.db(config.mongo.dbName);
      await ensureIndexes(db);
      return db;
    })().catch((err) => {
      connectPromise = null;
      connectFailed = true;
      throw err;
    });
  }
  return connectPromise;
}

export function isDbReady() {
  return Boolean(db);
}

export async function getDb() {
  if (!mongoEnabled() || connectFailed) return null;
  if (db) return db;
  try {
    return await connectMongo();
  } catch {
    return null;
  }
}

export async function closeMongo() {
  if (client) {
    await client.close();
    client = null;
    db = null;
    connectPromise = null;
  }
}

async function ensureIndexes(database) {
  await database.collection('packages').createIndex({ name: 1 }, { unique: true });
  await database.collection('package_versions').createIndex({ package_name: 1, version: 1 }, { unique: true });
  await database.collection('package_versions').createIndex({ package_name: 1, status: 1 });
  await database.collection('niao_releases').createIndex({ version: 1 }, { unique: true });
  await database.collection('niao_releases').createIndex({ is_latest: 1 });
}
