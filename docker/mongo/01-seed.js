// Demo database for PlatypusDB development and integration tests, like the SQL seeds:
// `customers`, `orders` (with an index on customer_id) and the view `customer_totals`.

const demo = db.getSiblingDB("platypus");
const countries = ["IT", "DE", "FR", "US", "JP"];
const statuses = ["new", "paid", "shipped", "cancelled"];
const now = Date.now();

demo.customers.insertMany(
  Array.from({ length: 1000 }, (_, i) => ({
    _id: i + 1,
    email: `user${i + 1}@example.com`,
    full_name: `Customer ${i + 1}`,
    country: countries[(i + 1) % 5],
    created_at: new Date(now - (i + 1) * 3600 * 1000),
    tags: i % 3 === 0 ? ["vip"] : [],
    address: { city: `City ${(i % 20) + 1}`, zip: String(10000 + i) },
  })),
);
demo.customers.createIndex({ email: 1 }, { unique: true });

demo.orders.insertMany(
  Array.from({ length: 20000 }, (_, i) => ({
    customer_id: 1 + ((i + 1) % 1000),
    total: NumberDecimal(((i * 37) % 50000 / 100).toFixed(2)),
    status: statuses[(i + 1) % 4],
    placed_at: new Date(now - (i + 1) * 60 * 1000),
  })),
);
demo.orders.createIndex({ customer_id: 1 });

demo.createView("customer_totals", "customers", [
  { $lookup: { from: "orders", localField: "_id", foreignField: "customer_id", as: "o" } },
  { $project: { full_name: 1, orders: { $size: "$o" }, spent: { $sum: "$o.total" } } },
]);
