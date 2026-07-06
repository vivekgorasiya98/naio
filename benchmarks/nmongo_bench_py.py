"""Python pymongo benchmark implementations."""

from __future__ import annotations

from concurrent.futures import ThreadPoolExecutor


def run_bench(uri: str, db: str, bench: str, n: int) -> None:
    from pymongo import MongoClient

    client = MongoClient(uri)
    coll_name = f"bench_{n}"
    coll = client[db][coll_name]
    coll.drop()

    if bench == "insert_one_loop":
        for i in range(n):
            coll.insert_one({"tag": i, "value": i})
    elif bench == "insert_many_single":
        coll.insert_many([{"tag": i, "value": i} for i in range(n)])
    elif bench == "insert_many_chunks":
        i = 0
        while i < n:
            chunk = [{"tag": j, "value": j} for j in range(i, min(i + 1000, n))]
            coll.insert_many(chunk)
            i += 1000
    elif bench == "find_all":
        coll.insert_many([{"tag": i, "value": i} for i in range(n)])
        list(coll.find({}))
    elif bench == "find_filtered":
        coll.insert_many([{"tag": i, "value": i} for i in range(n)])
        list(coll.find({"tag": 50}))
    elif bench == "count":
        coll.insert_many([{"tag": i, "value": i} for i in range(n)])
        coll.count_documents({})
    elif bench == "update_many":
        coll.insert_many([{"tag": i, "value": i} for i in range(n)])
        coll.update_many({}, {"$inc": {"value": 1}})
    elif bench == "delete_many":
        coll.insert_many([{"tag": i, "value": i} for i in range(n)])
        coll.delete_many({"tag": {"$lt": 50}})
    elif bench == "bulk_write":
        from pymongo import InsertOne, DeleteOne

        ops = [InsertOne({"tag": i, "value": i}) for i in range(n)]
        ops += [DeleteOne({"tag": i}) for i in range(n)]
        coll.bulk_write(ops)
    elif bench == "aggregate":
        coll.insert_many([{"tag": i, "value": i} for i in range(n)])
        list(coll.aggregate([{"$match": {"tag": {"$gte": 0}}}, {"$group": {"_id": "$tag", "c": {"$sum": 1}}}]))
    elif bench == "concurrent_reads":
        coll.insert_many([{"tag": i, "value": i} for i in range(n)])

        def read():
            return list(coll.find({}).limit(n))

        with ThreadPoolExecutor(max_workers=100) as pool:
            list(pool.map(lambda _: read(), range(100)))
    else:
        raise ValueError(f"unknown bench: {bench}")

    coll.drop()
    client.close()
