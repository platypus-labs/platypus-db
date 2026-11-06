import { type KeyboardEvent, useEffect, useState } from "react";
import { type SettingDef, type SettingPicture, getSetting } from "../settings";
import { Icon, type IconName, type IconSet } from "../ui/Icon";
import { toHex } from "../ui/colorHex";
import { t } from "../i18n";
import { numberRule, settingNumber } from "./settingValue";
import { toast } from "../ui/toast";

type Value = boolean | number | string;

/** Arrow keys in a radio group: select the previous or next option (wrapping) and focus it. */
function moveRadio(e: KeyboardEvent<HTMLElement>, values: string[], select: (value: string) => void) {
  const step = { ArrowLeft: -1, ArrowUp: -1, ArrowRight: 1, ArrowDown: 1 }[e.key];
  if (step === undefined) return;
  e.preventDefault();
  const at = values.indexOf(e.currentTarget.dataset.value ?? "");
  const value = values[(at + step + values.length) % values.length]!;
  select(value);
  e.currentTarget.parentElement?.querySelector<HTMLElement>(`[data-value="${CSS.escape(value)}"]`)?.focus();
}

/** The theme variable behind each color setting. */
const THEME_VAR: Record<string, string> = {
  "appearance.accent": "--accent",
  "appearance.background": "--bg",
  "appearance.sidebar": "--bg-sidebar",
  "appearance.text": "--fg",
};

/** The color the theme gives now: a color input needs a value, and an empty one would show grey. */
function themeHex(key: string): string {
  const v = getComputedStyle(document.documentElement).getPropertyValue(THEME_VAR[key] ?? "");
  return toHex(v) ?? "#808080";
}

/** A setting's row; `current` is its value when the setting does not live with the app's (a
 *  plugin's). */
export function SettingRow({ def, current, onSave }: { def: SettingDef<Value>; current?: Value; onSave: (value: Value | null) => void }) {
  const value = current ?? getSetting(def);
  const changed = value !== def.default;
  const c = def.control;
  let input: React.ReactNode;
  switch (c.type) {
    case "boolean":
      input = <input type="checkbox" checked={value as boolean} onChange={(e) => onSave(e.target.checked)} />;
      break;
    case "number":
      input = (
        <NumberInput
          value={value as number}
          min={c.min}
          max={c.max}
          step={c.step}
          onCommit={(n) => onSave(n)}
        />
      );
      break;
    case "text":
      input = c.multiline ? (
        <textarea
          key={value as string}
          rows={3}
          defaultValue={value as string}
          placeholder={c.placeholder}
          onBlur={(e) => e.target.value !== value && onSave(e.target.value)}
        />
      ) : (
        <input
          // A new field when the value changes elsewhere (Reset): blur would save the old one.
          key={value as string}
          defaultValue={value as string}
          placeholder={c.placeholder}
          onBlur={(e) => e.target.value !== value && onSave(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && e.currentTarget.blur()}
        />
      );
      break;
    case "color": {
      const hex = /^#[0-9a-f]{6}$/i.test(value as string) ? (value as string) : null;
      input = (
        <div className="color-control">
          {!hex && <span className="muted small">{t("Theme's")}</span>}
          <input
            type="color"
            value={hex ?? themeHex(def.key)}
            title={hex ?? t("The theme's color")}
            onChange={(e) => onSave(e.target.value)}
          />
        </div>
      );
      break;
    }
    case "swatches":
      input = (
        <div className="swatches-control" role="radiogroup">
          {c.options.map((o) => (
            <button
              key={o.value}
              type="button"
              role="radio"
              aria-checked={value === o.value}
              className={`theme-card${value === o.value ? " selected" : ""}`}
              onClick={() => onSave(o.value)}
            >
              <span className="theme-preview" style={{ background: o.colors[0] }}>
                <i style={{ background: o.colors[1] }} />
                <i style={{ background: o.colors[1] }} />
                <span className="theme-preview-editor">
                  {o.colors.slice(2, 5).map((color, i) => (
                    <b key={i} style={{ background: color }} />
                  ))}
                </span>
              </span>
              <span className="theme-card-name">{t(o.label)}</span>
            </button>
          ))}
        </div>
      );
      break;
    case "pictures":
      input = (
        <div className={`pictures-control${c.options.some((o) => "icons" in o.picture) ? " icons" : ""}`} role="radiogroup" aria-label={t(def.label)}>
          {c.options.map((o) => (
            <button
              key={o.value}
              type="button"
              role="radio"
              data-value={o.value}
              aria-checked={value === o.value}
              className={`picture-choice${value === o.value ? " selected" : ""}`}
              tabIndex={value === o.value ? 0 : -1}
              onClick={() => onSave(o.value)}
              onKeyDown={(e) => moveRadio(e, c.options.map((x) => x.value), onSave)}
            >
              <Picture picture={o.picture} />
              <span className="picture-choice-name">{t(o.label)}</span>
              {o.description && <span className="muted small">{t(o.description)}</span>}
            </button>
          ))}
        </div>
      );
      break;
    case "select":
      input = (
        <select value={value as string} onChange={(e) => onSave(e.target.value)}>
          {c.options.map((o) => (
            <option key={o.value} value={o.value}>
              {t(o.label)}
            </option>
          ))}
        </select>
      );
      break;
  }
  const reset = (
    <button
      type="button"
      className="link setting-reset"
      style={{ visibility: changed ? "visible" : "hidden" }}
      title={c.type === "color" ? t("Back to the theme's color") : t("Default: {value}", { value: defaultText(def) })}
      onClick={() => onSave(null)}
    >
      {t("Reset")}
    </button>
  );
  const text = (
    <span>
      <span className={`setting-title${changed ? " changed" : ""}`} title={changed ? t("Different from the default") : undefined}>{t(def.label)}</span>
      {def.description && <span className="muted small">{t(def.description)}</span>}
    </span>
  );
  // Cards take a row of their own, under the label.
  if (c.type === "swatches" || c.type === "pictures") {
    return (
      <div className={`setting${c.type === "pictures" ? " pictures" : ""}`} data-key={def.key}>
        {text}
        {reset}
        {input}
      </div>
    );
  }
  return (
    <label className="setting" data-key={def.key}>
      {text}
      {input}
      {reset}
    </label>
  );
}

/** The default of a setting as its control names it: an option's label, On or Off. */
function defaultText(def: SettingDef<Value>): string {
  const c = def.control;
  if ("options" in c) {
    const option = (c.options as { value: unknown; label: string }[]).find((o) => o.value === def.default);
    if (option) return t(option.label);
  }
  if (c.type === "boolean") return t(def.default ? "Enabled" : "Disabled");
  return String(def.default);
}

/** The icons each family shows in its card. */
const SAMPLE_ICONS: IconName[] = ["database", "table", "folder", "run", "settings"];

/** The picture of a choice: the window drawn with its parts in place, or a row of icons. */
function Picture({ picture }: { picture: SettingPicture }) {
  if ("icons" in picture) {
    return (
      <span className="picture-icons">
        {SAMPLE_ICONS.map((name) => (
          <Icon key={name} name={name} size={16} set={picture.icons as IconSet} />
        ))}
      </span>
    );
  }
  return (
    <span className="picture-window" style={{ gridTemplateColumns: picture.columns, gridTemplateRows: picture.rows }}>
      {picture.parts.map((p, i) => (
        <i key={i} className={`part-${p.part}`} style={{ gridRow: p.row, gridColumn: p.column }} />
      ))}
    </span>
  );
}

/** Saves on blur or Enter, and only valid numbers within bounds (whole ones for a whole
 *  step). */
function NumberInput(props: { value: number; min?: number; max?: number; step?: number; onCommit: (n: number) => void }) {
  const [text, setText] = useState(String(props.value));
  // A rejected value stays in the field, marked, with a toast saying what is accepted.
  const [invalid, setInvalid] = useState(false);
  // Follows the value when it changes elsewhere (Reset): blur would save the old one.
  useEffect(() => {
    setText(String(props.value));
    setInvalid(false);
  }, [props.value]);
  const commit = () => {
    const n = settingNumber(text, props);
    if (n === null) {
      setInvalid(true);
      const { min, max } = props;
      const rule = numberRule(props);
      toast(
        rule === "between"
          ? t("Between {min} and {max}", { min: String(min), max: String(max) })
          : rule === "min"
            ? t("At least {min}", { min: String(min) })
            : rule === "max"
              ? t("At most {max}", { max: String(max) })
              : t("Not a valid number"),
        "error",
      );
      return;
    }
    setInvalid(false);
    if (n !== props.value) props.onCommit(n);
    else setText(String(props.value));
  };
  return (
    <input
      type="number"
      value={text}
      min={props.min}
      max={props.max}
      step={props.step}
      aria-invalid={invalid || undefined}
      onChange={(e) => {
        setText(e.target.value);
        setInvalid(false);
      }}
      onBlur={commit}
      onKeyDown={(e) => e.key === "Enter" && commit()}
    />
  );
}
