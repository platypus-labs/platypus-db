import { useEffect, useRef, useState } from "react";
import { api } from "../api";
import { t } from "../i18n";
import { workbench } from "../workbench";
import { changelog, guide } from "./bundled";
import { openExternal } from "./external";
import { GuideMarkdown } from "./GuideMarkdown";
import { changelogSection, type LinkTarget, pageSource, sortedKeys, START_PAGE, titleOf } from "./pages";

/** Opens the guide that comes with the app, on `page` (a key such as `connect`) at the heading `anchor`. */
export function openGuide(page: string = START_PAGE, anchor: string | null = null) {
  workbench().openDialog((close) => <GuideDialog page={page} anchor={anchor} onClose={close} />);
}

/** Opens the changelog section of the running version. */
export function openWhatsNew() {
  workbench().openDialog((close) => <WhatsNewDialog onClose={close} />);
}

interface Place {
  page: string;
  anchor: string | null;
}

/** Scrolls the reader to the heading `anchor`, or to the top when the page has none. */
function scrollTo(reader: HTMLElement, anchor: string | null) {
  const heading = anchor ? [...reader.querySelectorAll<HTMLElement>("[data-anchor]")].find((h) => h.dataset.anchor === anchor) : undefined;
  reader.scrollTop = heading ? reader.scrollTop + heading.getBoundingClientRect().top - reader.getBoundingClientRect().top : 0;
}

function GuideDialog({ page, anchor, onClose }: { page: string; anchor: string | null; onClose: () => void }) {
  const [trail, setTrail] = useState<Place[]>([{ page, anchor }]);
  const here = trail[trail.length - 1]!;
  const { pages, images } = guide();
  const keys = sortedKeys(Object.keys(pages).filter((key) => !key.endsWith(".md")));
  const reader = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (reader.current) scrollTo(reader.current, here.anchor);
  }, [here]);

  const go = (place: Place) => setTrail((all) => [...all, place]);
  const follow = (target: LinkTarget) => {
    if (target.kind === "external") void openExternal(target.url);
    else if (target.kind === "anchor") go({ page: here.page, anchor: target.anchor });
    else if (target.kind === "page") go({ page: target.key ?? target.path, anchor: target.anchor });
  };
  const shown = pageSource(pages, here.page);

  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div
        className="modal guide-dialog"
        role="dialog"
        aria-label={t("User guide")}
        onKeyDown={(e) => e.key === "Escape" && onClose()}
      >
        <div className="guide-head">
          <h2>{t("User guide")}</h2>
          <button type="button" disabled={trail.length < 2} onClick={() => setTrail((all) => all.slice(0, -1))}>
            {t("Back")}
          </button>
          <span className="spacer" />
          <button type="button" className="primary" onClick={onClose}>
            {t("Done")}
          </button>
        </div>
        <div className="guide-body">
          <nav className="guide-index" aria-label={t("Contents")}>
            {keys.length === 0 ? (
              <p className="muted small">{t("The guide is not included in this build.")}</p>
            ) : (
              keys.map((key) => (
                <button
                  key={key}
                  type="button"
                  className={key.startsWith("databases/") && key !== "databases/README" ? "sub" : undefined}
                  aria-current={key === here.page ? "page" : undefined}
                  data-page={key}
                  onClick={() => go({ page: key, anchor: null })}
                >
                  {titleOf(pages[key]!, key)}
                </button>
              ))
            )}
            <p className="muted small guide-language">{t("The guide is written in English.")}</p>
          </nav>
          <div className="guide-reader" ref={reader} tabIndex={0} data-page={here.page} autoFocus>
            {"missing" in shown ? (
              <p className="guide-missing">{t("This page is not part of the guide that comes with the app: {path}", { path: shown.missing })}</p>
            ) : (
              <GuideMarkdown text={shown.text} page={here.page} images={images} onLink={follow} />
            )}
          </div>
        </div>
      </div>
    </div>
  );
}

function WhatsNewDialog({ onClose }: { onClose: () => void }) {
  const [version, setVersion] = useState<string | null>(null);
  useEffect(() => {
    api.appInfo().then((i) => setVersion(i.version), () => setVersion(""));
  }, []);
  const section = version ? changelogSection(changelog, version) : null;
  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="modal guide-dialog whats-new" role="dialog" aria-label={t("What's new")} onKeyDown={(e) => e.key === "Escape" && onClose()}>
        <div className="guide-head">
          <h2>{version ? t("What's new in {version}", { version }) : t("What's new")}</h2>
          <span className="spacer" />
          <button type="button" className="primary" autoFocus onClick={onClose}>
            {t("Done")}
          </button>
        </div>
        <div className="guide-reader" tabIndex={0}>
          {version === null ? null : section ? (
            <>
              {!section.exact && <p className="muted small">{t("There are no notes for version {version} yet. These are the changes not released yet.", { version })}</p>}
              <GuideMarkdown text={section.body} page={START_PAGE} images={{}} onLink={(target) => target.kind === "external" && void openExternal(target.url)} />
            </>
          ) : (
            <p className="guide-missing">{t("The changelog has nothing about this version.")}</p>
          )}
        </div>
      </div>
    </div>
  );
}
