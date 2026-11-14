# Brand

| File | What it is |
|---|---|
| [app-icon.png](app-icon.png) | The app icon, 1024×1024: the source of the installers' icons, the app's top bar and the site's favicon |
| [platypus-mark.png](platypus-mark.png) | The mark: the platypus head, 1024×1024, for small sizes and square spaces |
| [platypus-db-on-light.png](platypus-db-on-light.png) | Mark and wordmark for light backgrounds, 1600×400 (the README header, the welcome screen) |
| [platypus-db-on-dark.png](platypus-db-on-dark.png) | Mark and wordmark for dark backgrounds, 1600×400 |
| [logo-only-1024.png](logo-only-1024.png) | The mascot, the full platypus, 1024×1024: the site's hero and the app's lock screen |
| [parts/](parts/) | The mascot in parts, for the animated platypus of the site's home page: `head`, `body` (the upper disk), `bottom` (the lower disk), `tail`, `arm`, `foot-left`, `foot-right`, each a PNG of the same 1024×1024 canvas as logo-only-1024.png with the part where it sits, so the layers stacked make the mascot |
| [loading/](loading/) | The loading animation, the app's loading screen: `head.png` (the head without pupils), `eyes.png` (the pupils) and `rolling-eyes.png` (the dizzy spiral eyes) are layers of the same 1024×1024 canvas; [platypus-loader.css](loading/platypus-loader.css) animates them, [loader.html](loading/loader.html) has the markup to copy into a page and shows it; `loading-animated.png` is the animation rendered as an animated PNG (256×256, transparent, 50 fps) by `node scripts/loading-animation.mjs`, for where CSS cannot run (READMEs, documents, chats) |
| [avatar.png](avatar.png) | The organization's picture on GitHub, 512×512: the mark on the dark background, rendered from [avatar.html](avatar.html) |
| [social-preview.png](social-preview.png) | The repository's social preview, 1280×640, rendered from [social-preview.html](social-preview.html) by `node scripts/social-preview.mjs` (which renders the avatar too) |

The logos are PNG only, on purpose: there are no vector sources in the repository. The white
parts of the mark and the mascot (eyes, bill, belly) are transparent. Colors: orange `#ff8d28`
(`#e8721a` for text on white), ink `#1d1f24`, light `#f2f3f5`, dark background `#141519`.

## Changing a logo

The files above are the masters: every other copy (the app icons in
`apps/desktop/src-tauri/icons/`, `apps/desktop/src/assets/brand/`, `site/img/`, the mockups) is
generated from them. To change one:

1. Replace the master here, same name and size (the app icon: a square PNG with transparency,
   at least 1024×1024).
2. Run `node scripts/brand-assets.mjs` (it needs `npm --prefix apps/desktop ci` for `tauri icon`).
3. If the mark or the mascot changed, run `node scripts/social-preview.mjs` too, then upload the
   social preview and the avatar to GitHub ([RELEASING.md](../dev/RELEASING.md)).

The social preview is uploaded by hand in the repository settings (Settings › General ›
Social preview).

## License

These logos are not covered by the AGPL that covers the code: © 2026 PlatypusDB maintainers,
all rights reserved. "PlatypusDB" and the platypus logo are trademarks of the maintainers. What
you may do with them (refer to the project, link to it, redistribute the official builds) is in
[TRADEMARKS.md](../../TRADEMARKS.md); every reserved file is listed in [NOTICE](../../NOTICE).
`social-preview.html`, `avatar.html`, `loading/platypus-loader.css`, `loading/loader.html` and
this README are covered by the AGPL like the code.
