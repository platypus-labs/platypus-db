/**
 * Display formats of the result grid. The backend renders the grid's text with them
 * (`src-tauri/src/formats.rs`); editing, copying and exports keep the plain values.
 */
import { defineSetting, getSetting, useFormatsStored, useSetting } from "../settings";

export const dateFormat = defineSetting({
  key: "results.date_format",
  section: "Results",
  label: "Date format",
  description: "strftime pattern, e.g. %d/%m/%Y. Empty or invalid: %Y-%m-%d.",
  default: "",
  control: { type: "text", placeholder: "%Y-%m-%d" },
});

export const timestampFormat = defineSetting({
  key: "results.timestamp_format",
  section: "Results",
  label: "Timestamp format",
  description: "strftime pattern; zoned timestamps add their offset.",
  default: "",
  control: { type: "text", placeholder: "%Y-%m-%d %H:%M:%S%.f" },
});

export const timeZone = defineSetting({
  key: "results.time_zone",
  section: "Results",
  label: "Time zone of timestamps with a zone",
  default: "as_returned",
  control: {
    type: "select",
    options: [
      { value: "as_returned", label: "As the database returns them" },
      { value: "local", label: "This computer's time zone" },
      { value: "utc", label: "UTC" },
    ],
  },
});

export const numberFormat = defineSetting({
  key: "results.number_format",
  section: "Results",
  label: "Numbers",
  default: "plain",
  control: {
    type: "select",
    options: [
      { value: "plain", label: "1234567.89" },
      { value: "comma", label: "1,234,567.89" },
      { value: "dot", label: "1.234.567,89" },
      { value: "space", label: "1 234 567,89" },
      { value: "apostrophe", label: "1'234'567.89" },
    ],
  },
});

export const relativeTime = defineSetting({
  key: "results.relative_time",
  section: "Results",
  label: "Show timestamps as relative time (3 hours ago)",
  default: false,
  control: { type: "boolean" },
});

/** Changes whenever a format does, so rendered rows can be fetched again. */
export function useFormatSignature(): string {
  return [
    useSetting(dateFormat),
    useSetting(timestampFormat),
    useSetting(timeZone),
    useSetting(numberFormat),
    useSetting(relativeTime),
    useFormatsStored(),
  ].join("\u0000");
}

const SEPARATORS: Record<string, [group: string, decimal: string]> = {
  comma: [",", "."],
  dot: [".", ","],
  space: [" ", ","],
  apostrophe: ["'", "."],
};

/** A number as the grid shows it, back to its plain text, e.g. `1.234,5` → `1234.5`. */
export function plainNumber(text: string): string {
  const separators = SEPARATORS[getSetting(numberFormat)];
  if (!separators) return text;
  const [group, decimal] = separators;
  return text.replaceAll(group, "").replace(decimal, ".");
}
