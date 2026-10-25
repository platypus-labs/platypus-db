// Builds samples/wildlife.db, the SQLite sample database, from samples/wildlife.sql.
// Replaces the file if it exists. Needs Node 22.13 or later (node:sqlite).
//
//   node scripts/sample-db.mjs
import { existsSync, readFileSync, rmSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { DatabaseSync } from "node:sqlite";

const samples = join(dirname(dirname(fileURLToPath(import.meta.url))), "samples");
const target = join(samples, "wildlife.db");

if (existsSync(target)) rmSync(target);
const db = new DatabaseSync(target);
db.exec(readFileSync(join(samples, "wildlife.sql"), "utf8"));
const count = (table) => db.prepare(`SELECT count(*) AS n FROM ${table}`).get().n;
const sizes = ["rivers", "platypuses", "surveys", "sightings", "measurements"].map((t) => `${t} ${count(t)}`);
db.close();

console.log(`Created ${target}`);
console.log(sizes.join(", "));
