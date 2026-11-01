// The map panel (Show on map) on PostgreSQL and MySQL results. The dev PostgreSQL images carry
// no PostGIS (none of postgres, timescale, cockroach or yugabyte has the extension), so the
// values a PostGIS column sends are stored as what they are on the wire: hex EWKB text with an
// SRID (as PostGIS prints a geometry), WKB in a bytea, and WKT text; PostgreSQL's own geometric
// types (point, polygon, circle, path, box) go through the same panel. MySQL has real spatial
// columns (GEOMETRY, with the SRID in front of the WKB).
//  - each column draws the right kinds of shape (points, lines, areas, circles, the parts of a
//    collection) and counts its geometries, NULL cells left out;
//  - text that is no geometry is counted as "not readable", and a column with none says so;
//  - the panel follows the selection: another column, or two columns together;
//  - a click on a shape focuses its row; the wheel zooms, "Show all" and a double click fit
//    the view again;
//  - 10,500 rows: the panel reads at most 10,000 shapes and says the rest was not shown;
//  - the same on a MySQL GEOMETRY column.
// Needs PostgreSQL and MySQL from docker/compose.yml.
import assert from "node:assert/strict";
import { CTRL, SHIFT } from "../cdp.mjs";
import { db, dbQuiet, DBS, openEditor, runScript, saveConnections, skipUnless, visibleTab } from "../qa-xfer.mjs";

export const skip = () => skipUnless("pg", "mysql");

/** PostGIS' hex EWKB of a point with an SRID. */
function ewkbPoint(x, y, srid) {
  const b = Buffer.alloc(25);
  b[0] = 1;
  b.writeUInt32LE(0x2000_0001, 1);
  b.writeUInt32LE(srid, 5);
  b.writeDoubleLE(x, 9);
  b.writeDoubleLE(y, 17);
  return b.toString("hex").toUpperCase();
}

/** WKB of a polygon from its rings (arrays of [x, y]); with an SRID it is EWKB. */
function polygon(rings, srid) {
  const parts = [];
  const head = Buffer.alloc(srid ? 13 : 9);
  head[0] = 1;
  head.writeUInt32LE(srid ? 0x2000_0003 : 3, 1);
  if (srid) head.writeUInt32LE(srid, 5);
  head.writeUInt32LE(rings.length, head.length - 4);
  parts.push(head);
  for (const ring of rings) {
    const n = Buffer.alloc(4);
    n.writeUInt32LE(ring.length);
    parts.push(n);
    for (const [x, y] of ring) {
      const p = Buffer.alloc(16);
      p.writeDoubleLE(x, 0);
      p.writeDoubleLE(y, 8);
      parts.push(p);
    }
  }
  return Buffer.concat(parts);
}

const SQUARE = [[0, 0], [4, 0], [4, 4], [0, 4], [0, 0]];
const HOLE = [[1, 1], [2, 1], [2, 2], [1, 2], [1, 1]];

export function setup({ dir }) {
  saveConnections(dir, ["pg", "mysql"]);
  db(
    "pg",
    `DROP TABLE IF EXISTS qa_map;
     CREATE TABLE qa_map (id int PRIMARY KEY, pt point, poly polygon, circ circle, pth path, bx box, wkt text, ewkb text, wkb bytea, junk text);
     INSERT INTO qa_map VALUES
       (1, '(1,2)', '((0,0),(4,0),(4,4),(0,4))', '<(5,5),2>', '[(0,0),(3,3)]', '((1,1),(2,2))', 'POINT (10 20)',
        '${ewkbPoint(12.5, 41.9, 4326)}', '\\x${polygon([SQUARE]).toString("hex")}', 'hello'),
       (2, '(-3,4)', '((10,10),(12,10),(11,12))', '<(0,0),1>', '((0,0),(1,0),(1,1))', '((0,0),(5,5))',
        'MULTIPOLYGON (((0 0, 1 0, 1 1, 0 0)), ((5 5, 6 5, 6 6, 5 5)))', '${polygon([SQUARE, HOLE], 4326).toString("hex").toUpperCase()}', NULL, NULL),
       (3, NULL, NULL, NULL, NULL, NULL, 'GEOMETRYCOLLECTION (POINT (1 1), LINESTRING (0 0, 2 2))', NULL, NULL, 'POINT(')`,
  );
  db(
    "mysql",
    `DROP TABLE IF EXISTS qa_map_my;
     CREATE TABLE qa_map_my (id int PRIMARY KEY, g geometry, note varchar(10));
     INSERT INTO qa_map_my VALUES
       (1, ST_GeomFromText('POINT(1 2)'), 'point'),
       (2, ST_GeomFromText('LINESTRING(0 0, 3 3, 6 0)'), 'line'),
       (3, ST_GeomFromText('POLYGON((0 0, 4 0, 4 4, 0 4, 0 0))', 4326, 'axis-order=long-lat'), 'polygon'),
       (4, NULL, 'no shape')`,
  );
}

const grid = `${visibleTab}.querySelector('.grid-scroll')`;
const panel = `document.querySelector('.map-panel')`;
const size = `(${panel}?.querySelector('.value-size')?.textContent ?? '')`;
const shapes = (selector) => `${panel}.querySelectorAll(${JSON.stringify(selector)}).length`;
const menuButton = (label) => `[...document.querySelectorAll('.context-menu button')].find(b => b.querySelector('span')?.textContent === ${JSON.stringify(label)})`;
const panelButton = (label) => `[...${panel}.querySelectorAll('button')].find(b => b.getAttribute('aria-label') === ${JSON.stringify(label)})`;

/** Names the visible tab's grid `e2e-grid`; hidden tabs keep theirs otherwise. */
async function markGrid(page) {
  await page.eval(`document.querySelectorAll('#e2e-grid').forEach(e => e.removeAttribute('id'))`);
  await page.eval(`${grid}.id = 'e2e-grid'`);
}

/** Focuses the cell in display column `col` of data row `row`. */
async function focusCell(page, row, col) {
  await page.eval(`document.getElementById('e2e-grid').focus()`);
  await page.key("Home", "Home", 36, CTRL);
  for (let i = 0; i < row; i++) await page.key("ArrowDown", "ArrowDown", 40);
  for (let i = 0; i < col; i++) await page.key("ArrowRight", "ArrowRight", 39);
}

async function showOnMap(page) {
  const r = await page.rect(`#e2e-grid`);
  await page.click(r.x + 4, r.y + 52, { button: "right" });
  await page.waitFor(`document.querySelector('.context-menu') !== null`);
  await page.waitFor(`${menuButton("Analyze")} !== undefined`);
  await page.eval(`${menuButton("Analyze")}.click()`);
  await page.waitFor(`${menuButton("Show on map")} !== undefined`);
  await page.eval(`${menuButton("Show on map")}.click()`);
  await page.waitFor(`${panel} !== null`);
}

/** Moves the focus to column `col` of row 0 (the panel follows) and waits for its footer. */
async function column(page, col, footer, name) {
  await focusCell(page, 0, col);
  await page.waitFor(`${panel}.querySelector('.value-bar strong').textContent === ${JSON.stringify(`Map · ${name}`)}`, 15_000);
  await page.waitFor(`${size} === ${JSON.stringify(footer)}`, 15_000);
  // The shapes are drawn once the panel has its size.
  // (the shapes of the column before stay until the new ones come, 300 ms after the selection).
  if (!footer.startsWith("0 ")) {
    await page.waitFor(`(() => { const g = [...${panel}.querySelectorAll('[data-row]')]; return g.length > 0 && g.every(e => e.dataset.col === ${JSON.stringify(String(col))}); })()`, 15_000);
  }
}

export default async function (page) {
  try {
    await openEditor(page, DBS.pg.name);
    assert.equal(await runScript(page, "SELECT * FROM qa_map ORDER BY id"), "3 rows");
    await markGrid(page);

    // Nothing selected yet: the panel asks for a column; with a column it draws it.
    await focusCell(page, 0, 1);
    await showOnMap(page);
    await page.waitFor(`${size} === '2 geometries' && ${shapes("[data-row]")} === 2`, 15_000);
    assert.match(await page.eval(`${panel}.querySelector('.value-bar strong').textContent`), /^Map · pt$/);
    assert.equal(await page.eval(shapes(".map-point")), 2);
    assert.equal(await page.eval(shapes("[data-row]")), 2);
    assert.equal(await page.eval(`${panelButton("Show all")}.disabled`), true, "the whole map is shown");

    // PostgreSQL's geometric types.
    await column(page, 2, "2 geometries", "poly"); // polygon
    assert.equal(await page.eval(shapes("path.map-area")), 2);
    await column(page, 3, "2 geometries", "circ"); // circle: drawn as circles
    assert.equal(await page.eval(shapes("circle.map-area")), 2);
    await column(page, 4, "2 geometries", "pth"); // path: open as a line, closed as an area
    assert.equal(await page.eval(shapes("path.map-line")), 1);
    assert.equal(await page.eval(shapes("path.map-area")), 1);
    await column(page, 5, "2 geometries", "bx"); // box: a rectangle
    assert.equal(await page.eval(shapes("path.map-area")), 2);

    // WKT: a point, a multipolygon (two areas in one shape), a collection (a point and a line).
    await column(page, 6, "3 geometries", "wkt");
    assert.equal(await page.eval(shapes("[data-row]")), 3);
    assert.equal(await page.eval(shapes("circle.map-point")), 2, "the point of row 1 and the one of the collection");
    assert.equal(await page.eval(shapes("path.map-area")), 2, "both parts of the multipolygon");
    assert.equal(await page.eval(shapes("path.map-line")), 1);

    // What PostGIS sends: hex EWKB with an SRID; a polygon with a hole is one path of two rings.
    await column(page, 7, "2 geometries", "ewkb");
    assert.equal(await page.eval(shapes("circle.map-point")), 1);
    assert.equal(await page.eval(shapes("path.map-area")), 1);
    assert.equal((await page.eval(`${panel}.querySelector('path.map-area').getAttribute('d')`)).match(/M/g).length, 2, "outer ring and hole");
    // WKB in a bytea; the NULL is not counted as unreadable.
    await column(page, 8, "1 geometry", "wkb");
    assert.equal(await page.eval(shapes("path.map-area")), 1);

    // Text that is no geometry: counted, and the panel says there is nothing to draw.
    await column(page, 9, "0 geometries · 2 not readable", "junk");
    assert.match(await page.eval(`${panel}.querySelector('.value-empty')?.textContent ?? ''`), /No geometries in the selected columns/);
    assert.equal(await page.eval(shapes("[data-row]")), 0);

    // Two columns together: the selection widens and the shapes of both are drawn.
    await focusCell(page, 0, 6);
    await page.key("ArrowRight", "ArrowRight", 39, SHIFT);
    await page.waitFor(`${size} === '5 geometries'`, 15_000);
    assert.match(await page.eval(`${panel}.querySelector('.value-bar strong').textContent`), /^Map · wkt, ewkb$/);

    // A click on a shape focuses its row; the focused shape is drawn on top, marked.
    await column(page, 1, "2 geometries", "pt");
    await page.eval(`${panel}.querySelector('[data-row="1"]').id = 'e2e-shape'`);
    const point = await page.rect("#e2e-shape");
    await page.click(point.x + point.w / 2, point.y + point.h / 2);
    await page.waitFor(`${panel}.querySelector('.map-shape.focused')?.dataset.row === '1'`);
    assert.match(await page.eval(`${panel}.querySelector('[data-row="1"] title').textContent`), /^Row 2 · pt$/);

    // The wheel zooms around the pointer; Show all and a double click fit the view again.
    const view = await page.rect(".map-panel .map-view");
    const before = await page.eval(`${panel}.querySelector('circle.map-point').getAttribute('cx')`);
    await page.send("Input.dispatchMouseEvent", { type: "mouseWheel", x: view.x + view.w / 2, y: view.y + view.h / 2, deltaX: 0, deltaY: -300 });
    await page.waitFor(`${panelButton("Show all")}.disabled === false`);
    assert.notEqual(await page.eval(`${panel}.querySelector('circle.map-point').getAttribute('cx')`), before);
    await page.eval(`${panelButton("Show all")}.click()`);
    await page.waitFor(`${panelButton("Show all")}.disabled === true`);
    assert.equal(await page.eval(`${panel}.querySelector('circle.map-point').getAttribute('cx')`), before);
    await page.send("Input.dispatchMouseEvent", { type: "mouseWheel", x: view.x + view.w / 2, y: view.y + view.h / 2, deltaX: 0, deltaY: 300 });
    await page.waitFor(`${panelButton("Show all")}.disabled === false`);
    await page.click(view.x + 5, view.y + 5, { clickCount: 2 });
    await page.waitFor(`${panelButton("Show all")}.disabled === true`);

    // Closing the panel leaves the grid as it was.
    await page.eval(`${panelButton("Close")}.click()`);
    await page.waitFor(`${panel} === null`);

    // More than 10,000 shapes: the panel stops there and says so.
    assert.match(await runScript(page, "SELECT 'POINT (' || g || ' ' || (g % 97) || ')' AS p FROM generate_series(1, 10500) g"), /rows loaded/);
    await markGrid(page);
    await page.eval(`[...${visibleTab}.querySelectorAll('button')].find(b => b.textContent.trim() === 'Fetch all').click()`);
    await page.waitFor(`/^10,500 rows/.test(${visibleTab}.querySelector('.statusbar span')?.textContent ?? '')`, 60_000);
    await focusCell(page, 0, 0);
    await showOnMap(page);
    await page.waitFor(`${size}.includes('(more not shown)')`, 60_000);
    assert.match(await page.eval(size), /^10,?000 geometries \(more not shown\)$/);
    await page.eval(`${panelButton("Close")}.click()`);

    // MySQL: a GEOMETRY column, its SRID in front of the WKB, drawn as points, lines and areas.
    await openEditor(page, DBS.mysql.name);
    assert.equal(await runScript(page, "SELECT id, g, note FROM qa_map_my ORDER BY id"), "4 rows");
    await markGrid(page);
    await focusCell(page, 0, 1);
    await showOnMap(page);
    await page.waitFor(`${size} === '3 geometries' && ${shapes("[data-row]")} === 3`, 15_000);
    assert.equal(await page.eval(shapes("circle.map-point")), 1);
    assert.equal(await page.eval(shapes("path.map-line")), 1);
    assert.equal(await page.eval(shapes("path.map-area")), 1);
    // The text column next to it holds no geometry.
    await focusCell(page, 0, 2);
    await page.waitFor(`${size} === '0 geometries · 4 not readable'`, 15_000);
  } finally {
    dbQuiet("pg", "DROP TABLE IF EXISTS qa_map");
    dbQuiet("mysql", "DROP TABLE IF EXISTS qa_map_my");
  }
}
