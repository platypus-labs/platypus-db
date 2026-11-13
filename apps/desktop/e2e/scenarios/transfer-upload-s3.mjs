// The result export with "Also upload > To a bucket (S3, GCS, R2, MinIO)" against the S3 service
// of docker/compose.yml (SeaweedFS, profile `s3`, http://127.0.0.1:49000, path style, keys
// platypus / platypus-secret), each object read back with a signed GET from node:
//  - a CSV goes to the bucket under the folder given, byte for byte as the local file;
//    a folder with slashes around it is trimmed;
//  - a file name with spaces and non-ASCII letters, a gzip file, and a Parquet file (binary)
//    arrive intact under their own names; a 5 MB file goes in one piece;
//  - no folder: the object is at the root of the bucket;
//  - the form remembers the endpoint, region, bucket, folder and access key for the next export
//    but never the secret key;
//  - failures say what the store said, and nothing is left in the bucket: a wrong secret key
//    (SignatureDoesNotMatch), a wrong access key (InvalidAccessKeyId), a bucket that does not
//    exist (NoSuchBucket); an endpoint that is not an address, and one nobody listens on, give
//    their own messages. The local file is written in every case.
// Needs the s3 profile of docker/compose.yml.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { gunzipSync } from "node:zlib";
import { S3, deleteObject, getObject, listKeys, skipUnlessS3 } from "../s3.mjs";
import { button, fill, openEditor, runScript, sqliteOnly, visibleTab } from "../transfer-helpers.mjs";
import { answerSave } from "../transfer-helpers.mjs";

export const skip = skipUnlessS3;

const RUN = `qa-upload-${Date.now()}`;
const created = [];

export async function setup({ dir }) {
  await sqliteOnly(
    dir,
    `CREATE TABLE people (id INTEGER PRIMARY KEY, name TEXT, note TEXT);
     INSERT INTO people VALUES (1, 'Ada', 'first'), (2, 'Grace, "Amazing"', NULL), (3, 'Zoë 😀', 'três');`,
  );
}

const dialog = `[...document.querySelectorAll('.modal')].find(m => m.querySelector('h2')?.textContent === 'Export result')`;
const field = (label) => `[...${dialog}.querySelectorAll('label.field')].find(l => l.querySelector('span')?.textContent === ${JSON.stringify(label)})?.querySelector('input, select')`;

/** The fields of the bucket form for the service of the test, with the changes in `bucket`. */
const bucketFields = (bucket = {}) => ({
  Endpoint: S3.endpoint,
  Region: S3.region,
  Bucket: S3.bucket,
  "Folder (optional)": RUN,
  "Access key": S3.accessKey,
  "Secret key": S3.secretKey,
  ...bucket,
});

/** Opens Export…, picks the format and compression, "To a bucket" with `fields`, and exports to
 *  `path`. Returns the status text (the outcome) once it shows. */
async function exportToBucket(page, path, { format = "csv", compression, fields = bucketFields(), keepForm = false } = {}) {
  await page.eval(`[...${visibleTab}.querySelectorAll('button.link')].find(b => b.textContent === 'Export…').click()`);
  await page.waitFor(`${dialog} !== undefined`);
  await page.eval(fill(`${dialog}.querySelector('select')`, format));
  if (compression) await page.eval(fill(field("Compression"), compression));
  await page.eval(fill(field("Also upload to…"), "bucket"));
  await page.waitFor(`${field("Endpoint")} != null`);
  if (keepForm) return;
  for (const [label, value] of Object.entries(fields)) {
    await page.eval(fill(field(label), value));
    await page.waitFor(`${field(label)}.value === ${JSON.stringify(value)}`);
  }
  await answerSave(page, path);
  await page.eval(`${button(dialog, "Export…")}.click()`);
  const outcome = `[...${visibleTab}.querySelectorAll('span')].find(s => /^(Exported|Export failed|Export cancelled)/.test(s.textContent))`;
  await page.waitFor(`${outcome} !== undefined`, 60_000);
  const text = await page.eval(`${outcome}.textContent`);
  await page.eval(`${outcome}.querySelector('button')?.click()`);
  return text;
}

const key = (name) => `${RUN}/${name}`;

export default async function (page, { dir }) {
  try {
    await openEditor(page, "QA SQLite");
    assert.equal(await runScript(page, "SELECT * FROM people ORDER BY id"), "3 rows");

    // A CSV under a folder: the object is the local file.
    let out = await exportToBucket(page, join(dir, "people.csv"));
    assert.match(out, /^Exported 3 rows/, out);
    created.push(key("people.csv"));
    const local = readFileSync(join(dir, "people.csv"));
    assert.deepEqual(await getObject(key("people.csv")), local);
    assert.equal(local.toString("utf8").split("\n")[0], "id,name,note");
    assert.deepEqual(await listKeys(RUN), [key("people.csv")]);

    // The form keeps what is not secret: reopening it shows the bucket again, the secret empty.
    await exportToBucket(page, join(dir, "unused.csv"), { keepForm: true });
    assert.equal(await page.eval(`${field("Endpoint")}.value`), S3.endpoint);
    assert.equal(await page.eval(`${field("Bucket")}.value`), S3.bucket);
    assert.equal(await page.eval(`${field("Folder (optional)")}.value`), RUN);
    assert.equal(await page.eval(`${field("Access key")}.value`), S3.accessKey);
    assert.equal(await page.eval(`${field("Secret key")}.value`), "", "the secret key is never kept");
    assert.equal(await page.eval(`${field("Secret key")}.type`), "password");
    assert.ok(!(await page.eval(`localStorage.getItem('platypus.export.bucket')`)).includes(S3.secretKey));
    await page.eval(`${button(dialog, "Cancel")}.click()`);
    await page.waitFor(`${dialog} === undefined`);

    // A name with spaces and letters outside ASCII; slashes around the folder are dropped.
    out = await exportToBucket(page, join(dir, "rapporto è 1 😀.csv"), { fields: bucketFields({ "Folder (optional)": `/${RUN}/nested/` }) });
    assert.match(out, /^Exported 3 rows/, out);
    created.push(`${RUN}/nested/rapporto è 1 😀.csv`);
    assert.deepEqual(await getObject(`${RUN}/nested/rapporto è 1 😀.csv`), readFileSync(join(dir, "rapporto è 1 😀.csv")));

    // gzip and Parquet arrive as the local bytes.
    out = await exportToBucket(page, join(dir, "people.csv.gz"), { compression: "gzip" });
    assert.match(out, /^Exported 3 rows/, out);
    created.push(key("people.csv.gz"));
    const gz = await getObject(key("people.csv.gz"));
    assert.deepEqual(gz, readFileSync(join(dir, "people.csv.gz")));
    assert.equal(gunzipSync(gz).toString("utf8"), local.toString("utf8"));
    out = await exportToBucket(page, join(dir, "people.parquet"), { format: "parquet" });
    assert.match(out, /^Exported 3 rows/, out);
    created.push(key("people.parquet"));
    const parquet = await getObject(key("people.parquet"));
    assert.deepEqual(parquet, readFileSync(join(dir, "people.parquet")));
    assert.equal(parquet.subarray(0, 4).toString("latin1"), "PAR1");

    // No folder: the object is at the root of the bucket.
    out = await exportToBucket(page, join(dir, `${RUN}-root.csv`), { fields: bucketFields({ "Folder (optional)": "" }) });
    assert.match(out, /^Exported 3 rows/, out);
    created.push(`${RUN}-root.csv`);
    assert.deepEqual(await getObject(`${RUN}-root.csv`), readFileSync(join(dir, `${RUN}-root.csv`)));

    // A bigger file (5 MB of text) in one request.
    assert.match(await runScript(page, "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 100000) SELECT i, hex(randomblob(20)) AS h FROM n"), /rows/);
    out = await exportToBucket(page, join(dir, "big.csv"));
    assert.match(out, /^Exported 100,000 rows/, out);
    created.push(key("big.csv"));
    const big = readFileSync(join(dir, "big.csv"));
    assert.ok(big.length > 4_000_000, `${big.length} bytes`);
    assert.deepEqual(await getObject(key("big.csv")), big);

    // Failures: the store's own words, nothing uploaded, the local file still written.
    const before = (await listKeys(RUN)).length;
    out = await exportToBucket(page, join(dir, "wrong-secret.csv"), { fields: bucketFields({ "Secret key": "not-the-secret" }) });
    assert.match(out, /^Export failed: /, out);
    assert.match(out, /403/, out);
    assert.match(out, /SignatureDoesNotMatch/, out);
    assert.ok(out.includes(`${S3.endpoint.replace("http://", "http://")}/${S3.bucket}/${RUN}/wrong-secret.csv`), `the failure names the object: ${out}`);
    assert.ok(!out.includes("not-the-secret"), "the secret is not in the message");
    out = await exportToBucket(page, join(dir, "wrong-access.csv"), { fields: bucketFields({ "Access key": "someone-else" }) });
    assert.match(out, /^Export failed: /, out);
    assert.match(out, /403/, out);
    assert.match(out, /InvalidAccessKeyId|AccessDenied/, out);
    out = await exportToBucket(page, join(dir, "no-bucket.csv"), { fields: bucketFields({ Bucket: "no-such-bucket-qa" }) });
    assert.match(out, /^Export failed: /, out);
    // SeaweedFS answers 500 InternalError for a bucket it does not have; S3 says 404 NoSuchBucket.
    assert.match(out, /: (404|500) /, out);
    assert.ok(out.includes("/no-such-bucket-qa/"), out);
    out = await exportToBucket(page, join(dir, "bad-endpoint.csv"), { fields: bucketFields({ Endpoint: "127.0.0.1:49000" }) });
    assert.match(out, /^Export failed: .*endpoint must be an address like https:\/\/s3\.eu-west-1\.amazonaws\.com/, out);
    out = await exportToBucket(page, join(dir, "closed-port.csv"), { fields: bucketFields({ Endpoint: "http://127.0.0.1:9" }) });
    assert.match(out, /^Export failed: .*127\.0\.0\.1:9/, out);
    assert.equal((await listKeys(RUN)).length, before, "a failed upload leaves nothing in the bucket");
    assert.equal(await getObject(key("wrong-secret.csv")), null);
    // The failed uploads still left the local files the export wrote first.
    for (const name of ["wrong-secret.csv", "wrong-access.csv"]) assert.equal(readFileSync(join(dir, name), "utf8").split("\n")[0].length > 0, true, name);
    // The export works again with the right keys right after the failures.
    out = await exportToBucket(page, join(dir, "again.csv"));
    assert.match(out, /^Exported /, out);
    created.push(key("again.csv"));
    assert.ok(await getObject(key("again.csv")));
  } finally {
    for (const k of created) await deleteObject(k);
  }
}
