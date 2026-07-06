const { MongoClient } = require("mongodb");

async function runBench(uri, dbName, bench, n) {
  const client = new MongoClient(uri);
  await client.connect();
  const collName = `bench_${n}`;
  const coll = client.db(dbName).collection(collName);
  await coll.drop().catch(() => {});

  if (bench === "insert_one_loop") {
    for (let i = 0; i < n; i++) {
      await coll.insertOne({ tag: i, value: i });
    }
  } else if (bench === "insert_many_single") {
    await coll.insertMany(Array.from({ length: n }, (_, i) => ({ tag: i, value: i })));
  } else if (bench === "insert_many_chunks") {
    for (let i = 0; i < n; i += 1000) {
      const chunk = [];
      for (let j = i; j < Math.min(i + 1000, n); j++) {
        chunk.push({ tag: j, value: j });
      }
      await coll.insertMany(chunk);
    }
  } else if (bench === "find_all") {
    await coll.insertMany(Array.from({ length: n }, (_, i) => ({ tag: i, value: i })));
    await coll.find({}).toArray();
  } else if (bench === "find_filtered") {
    await coll.insertMany(Array.from({ length: n }, (_, i) => ({ tag: i, value: i })));
    await coll.find({ tag: 50 }).toArray();
  } else if (bench === "count") {
    await coll.insertMany(Array.from({ length: n }, (_, i) => ({ tag: i, value: i })));
    await coll.countDocuments({});
  } else if (bench === "update_many") {
    await coll.insertMany(Array.from({ length: n }, (_, i) => ({ tag: i, value: i })));
    await coll.updateMany({}, { $inc: { value: 1 } });
  } else if (bench === "delete_many") {
    await coll.insertMany(Array.from({ length: n }, (_, i) => ({ tag: i, value: i })));
    await coll.deleteMany({ tag: { $lt: 50 } });
  } else if (bench === "bulk_write") {
    const ops = [];
    for (let i = 0; i < n; i++) {
      ops.push({ insertOne: { document: { tag: i, value: i } } });
    }
    for (let i = 0; i < n; i++) {
      ops.push({ deleteOne: { filter: { tag: i } } });
    }
    await coll.bulkWrite(ops);
  } else if (bench === "aggregate") {
    await coll.insertMany(Array.from({ length: n }, (_, i) => ({ tag: i, value: i })));
    await coll
      .aggregate([{ $match: { tag: { $gte: 0 } } }, { $group: { _id: "$tag", c: { $sum: 1 } } }])
      .toArray();
  } else if (bench === "concurrent_reads") {
    await coll.insertMany(Array.from({ length: n }, (_, i) => ({ tag: i, value: i })));
    await Promise.all(
      Array.from({ length: 100 }, () => coll.find({}).limit(n).toArray())
    );
  } else {
    throw new Error(`unknown bench: ${bench}`);
  }

  await coll.drop().catch(() => {});
  await client.close();
}

const [uri, db, bench, nStr] = process.argv.slice(2);
runBench(uri, db, bench, parseInt(nStr, 10)).catch((err) => {
  console.error(err);
  process.exit(1);
});
