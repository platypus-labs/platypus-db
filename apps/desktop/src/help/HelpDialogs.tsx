import { useEffect, useMemo, useRef, useState } from "react";
import { api } from "../api";
import { t } from "../i18n";
import { workbench } from "../workbench";
import { changelog, guide } from "./bundled";
import { openExternal } from "./external";
import { GuideMarkdown } from "./GuideMarkdown";
import { changelogSection, type LinkTarget, pageSource, sortedKeys, START_PAGE, titleOf } from "./pages";
import { advance, arrival, back, type Place } from "./trail";

// Loaded at first use, through `open.ts`: import that one, not this module.

export function openGuide(page: string = START_PAGE, anchor: string | null = null) {
  workbench().openDialog((close) => <GuideDialog page={page} anchor={anchor} onClose={close} />);
}

export function openWhatsNew() {
  workbench().openDialog((close) => <WhatsNewDialog onClose={close} />);
}

/** Scrolls the reader to where it was left, else to the heading of the place, else to the top. */
function scrollTo(reader: HTMLElement, place: Place) {
  const at = arrival(place);
  if ("top" in at) {
    reader.scrollTop = at.top;
    return;
  }
  const heading = at.anchor ? [...reader.querySelectorAll<HTMLElement>("[data-anchor]")].find((h) => h.dataset.anchor === at.anchor) : undefined;
  reader.scrollTop = heading ? reader.scrollTop + heading.getBoundingClientRect().top - reader.getBoundingClientRect().top : 0;
}

function GuideDialog({ page, anchor, onClose }: { page: string; anchor: string | null; onClose: () => void }) {
  const [trail, setTrail] = useState<Place[]>([{ page, anchor, scroll: null }]);
  const here = trail[trail.length - 1]!;
  const { pages, images } = guide();
  const index = useMemo(
    () =>
      sortedKeys(Object.keys(pages).filter((key) => !key.endsWith(".md"))).map((key) => ({ key, title: titleOf(pages[key]!, key) })),
    [pages],
  );
  const reader = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (reader.current) scrollTo(reader.current, here);
  }, [here]);
  // Escape and the arrow keys work at once; closing gives the focus back, so that the form or the
  // Settings under the guide still answer to the keyboard.
  useEffect(() => {
    const before = document.activeElement;
    reader.current?.focus();
    return () => {
      if (before instanceof HTMLElement) before.focus();
    };
  }, []);

  const go = (place: Omit<Place, "scroll">) => {
    const scroll = reader.current?.scrollTop ?? 0;
    setTrail((all) => advance(all, scroll, place));
  };
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
          <button type="button" disabled={trail.length < 2} onClick={() => setTrail(back)}>
            {t("Back")}
          </button>
          <span className="spacer" />
          <button type="button" className="primary" onClick={onClose}>
            {t("Done")}
          </button>
        </div>
        <div className="guide-body">
          <nav className="guide-index" aria-label={t("Contents")}>
            {index.length === 0 ? (
              <p className="muted small">{t("The guide is not included in this build.")}</p>
            ) : (
              index.map(({ key, title }) => (
                <button
                  key={key}
                  type="button"
                  className={key.startsWith("databases/") && key !== "databases/README" ? "sub" : undefined}
                  aria-current={key === here.page ? "page" : undefined}
                  data-page={key}
                  onClick={() => go({ page: key, anchor: null })}
                >
                  {title}
                </button>
              ))
            )}
            <p className="muted small guide-language">{t("The guide is written in English.")}</p>
          </nav>
          <div className="guide-reader" ref={reader} tabIndex={0} data-page={here.page}>
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
