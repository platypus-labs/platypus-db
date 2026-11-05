/**
 * The pages the end-to-end tests show in place of the bundled guide. Apart from `bundled.ts`, so
 * that the test hook of the app does not load the guide's text.
 */
let replaced: Record<string, string> | null = null;

/** The pages in place of the bundled ones, or `null` for the bundled ones. */
export const replacedPages = () => replaced;

/** Shows these pages in place of the bundled ones (`null`: the bundled again). */
export const replaceGuide = (pages: Record<string, string> | null) => {
  replaced = pages;
};
