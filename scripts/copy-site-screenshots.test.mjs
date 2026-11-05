// Which pictures the site's pages show (scripts/copy-site-screenshots.mjs).
import assert from "node:assert/strict";
import { test } from "node:test";
import { picturesIn } from "./copy-site-screenshots.mjs";

test("the pictures of the pages, once each and in order", () => {
  const pages = [
    `<img src="img/screenshots/palette.png"><video src="img/screenshots/hero.gif">`,
    `<img src="./img/screenshots/palette.png" alt="x"><a href="img/other/logo.png">`,
  ];
  assert.deepEqual(picturesIn(pages), ["hero.gif", "palette.png"]);
  assert.deepEqual(picturesIn(["<p>nothing</p>"]), []);
  assert.deepEqual(picturesIn([`<img src="img/screenshots/guide/welcome.png">`, `background: url(../img/screenshots/hero.gif);`]), ["guide/welcome.png", "hero.gif"]);
});
