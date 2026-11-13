/** A chart saved as an SVG or PNG file, through the save dialog used by the other exports. */
import { save } from "@tauri-apps/plugin-dialog";
import { api } from "../api";
import { t } from "../i18n";
import { answerFor } from "../testAnswers";

/** The chart as an SVG file's text. */
export function svgText(svg: SVGSVGElement): string {
  const copy = svg.cloneNode(true) as SVGSVGElement;
  copy.setAttribute("xmlns", "http://www.w3.org/2000/svg");
  return `<?xml version="1.0" encoding="UTF-8"?>\n${new XMLSerializer().serializeToString(copy)}`;
}

/** The chart drawn on a canvas at `scale` times its size, as PNG bytes. */
async function pngBytes(svg: SVGSVGElement, scale = 2): Promise<Uint8Array> {
  const width = svg.width.baseVal.value;
  const height = svg.height.baseVal.value;
  const image = new Image();
  image.src = `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svgText(svg))}`;
  await image.decode();
  const canvas = document.createElement("canvas");
  canvas.width = Math.round(width * scale);
  canvas.height = Math.round(height * scale);
  const context = canvas.getContext("2d");
  if (!context) throw new Error(t("The chart could not be drawn as PNG."));
  context.scale(scale, scale);
  context.drawImage(image, 0, 0, width, height);
  const blob = await new Promise<Blob | null>((resolve) => canvas.toBlob(resolve, "image/png"));
  if (!blob) throw new Error(t("The chart could not be drawn as PNG."));
  return new Uint8Array(await blob.arrayBuffer());
}

/** Asks where to save the chart and writes it there; the path, or `null` when cancelled. */
export async function exportChart(svg: SVGSVGElement, format: "png" | "svg", name = "chart"): Promise<string | null> {
  const picked = await answerFor<string | null>("file", () =>
    save({ defaultPath: `${name}.${format}`, filters: [{ name: format.toUpperCase(), extensions: [format] }] }),
  );
  if (!picked) return null;
  // The backend writes bytes only to image files: a name typed without extension gets one.
  const path = picked.toLowerCase().endsWith(`.${format}`) ? picked : `${picked}.${format}`;
  if (format === "svg") await api.writeTextFile(path, svgText(svg));
  else await api.writeBinaryFile(path, await pngBytes(svg));
  return path;
}
