/**
 * JSON handled as text, so that values from the database are shown and written back exactly:
 * `JSON.parse` rounds integers above 2^53 and respells numbers (`1.10` → `1.1`, `1e2` → `100`).
 * Here numbers keep the spelling they have in the text.
 */

const isSpace = (ch: string | undefined) => ch === " " || ch === "\t" || ch === "\n" || ch === "\r";

/** Index just past the string literal that starts at `start` (on its opening quote). */
function stringEnd(text: string, start: number): number {
  let i = start + 1;
  while (i < text.length) {
    const ch = text[i];
    if (ch === "\\") i += 2;
    else if (ch === '"') return i + 1;
    else i++;
  }
  return text.length;
}

/**
 * Indents JSON text by re-placing whitespace between its tokens: numbers, strings and key
 * order stay as written. Throws `SyntaxError` (from `JSON.parse`) when `text` is not JSON.
 */
export function formatJson(text: string, indent = "  "): string {
  JSON.parse(text);
  let out = "";
  let depth = 0;
  const newline = () => `\n${indent.repeat(depth)}`;
  let i = 0;
  while (i < text.length) {
    const ch = text[i]!;
    if (ch === '"') {
      const end = stringEnd(text, i);
      out += text.slice(i, end);
      i = end;
    } else if (isSpace(ch)) {
      i++;
    } else if (ch === "{" || ch === "[") {
      let j = i + 1;
      while (isSpace(text[j])) j++;
      // An empty object or array stays on one line.
      if (text[j] === (ch === "{" ? "}" : "]")) {
        out += ch + text[j];
        i = j + 1;
      } else {
        depth++;
        out += ch + newline();
        i++;
      }
    } else if (ch === "}" || ch === "]") {
      depth--;
      out += newline() + ch;
      i++;
    } else if (ch === ",") {
      out += `,${newline()}`;
      i++;
    } else if (ch === ":") {
      out += ": ";
      i++;
    } else {
      out += ch;
      i++;
    }
  }
  return out;
}

/** An object or array, as opposed to a bare number, string or literal, which is valid JSON
 *  too but reads better as plain text. */
export function isJsonDocument(text: string): boolean {
  const start = text.trimStart()[0];
  if (start !== "{" && start !== "[") return false;
  try {
    JSON.parse(text);
    return true;
  } catch {
    return false;
  }
}

/** A JSON number, kept as written. */
export class JsonNumber {
  readonly text: string;
  constructor(text: string) {
    this.text = text;
  }
  toString() {
    return this.text;
  }
}

export type JsonValue = null | boolean | JsonNumber | string | JsonValue[] | { [key: string]: JsonValue };

const NUMBER = /^-?(?:0|[1-9]\d*)(?:\.\d+)?(?:[eE][+-]?\d+)?$/;

/** `text` as a JSON number when it is spelled as one, else `null`. */
export function jsonNumber(text: string): JsonNumber | null {
  return NUMBER.test(text) ? new JsonNumber(text) : null;
}

/** Parses JSON with numbers kept as `JsonNumber`. Throws `SyntaxError` when invalid. */
export function parseJson(text: string): JsonValue {
  JSON.parse(text);
  let i = 0;
  const skip = () => {
    while (isSpace(text[i])) i++;
  };
  const value = (): JsonValue => {
    skip();
    const ch = text[i];
    if (ch === "{") {
      i++;
      const out: { [key: string]: JsonValue } = {};
      skip();
      if (text[i] === "}") {
        i++;
        return out;
      }
      for (;;) {
        skip();
        const key = string();
        skip();
        i++; // ':'
        // Defined, not assigned: a key named `__proto__` is a plain key, as with JSON.parse.
        Object.defineProperty(out, key, { value: value(), enumerable: true, writable: true, configurable: true });
        skip();
        if (text[i++] === "}") return out;
      }
    }
    if (ch === "[") {
      i++;
      const out: JsonValue[] = [];
      skip();
      if (text[i] === "]") {
        i++;
        return out;
      }
      for (;;) {
        out.push(value());
        skip();
        if (text[i++] === "]") return out;
      }
    }
    if (ch === '"') return string();
    for (const [word, literal] of [
      ["true", true],
      ["false", false],
      ["null", null],
    ] as const) {
      if (text.startsWith(word, i)) {
        i += word.length;
        return literal;
      }
    }
    const start = i;
    while (i < text.length && /[-+.\deE]/.test(text[i]!)) i++;
    return new JsonNumber(text.slice(start, i));
  };
  const string = (): string => {
    const end = stringEnd(text, i);
    const s = JSON.parse(text.slice(i, end)) as string;
    i = end;
    return s;
  };
  return value();
}

/** Writes a value of `parseJson` back as JSON text, numbers as they were read. */
export function stringifyJson(value: JsonValue, indent = "  "): string {
  const write = (v: JsonValue, pad: string): string => {
    if (v === null || typeof v === "boolean" || typeof v === "string") return JSON.stringify(v);
    if (v instanceof JsonNumber) return v.text;
    const inner = pad + indent;
    if (Array.isArray(v)) {
      if (v.length === 0) return "[]";
      return `[\n${v.map((x) => inner + write(x, inner)).join(",\n")}\n${pad}]`;
    }
    const entries = Object.entries(v);
    if (entries.length === 0) return "{}";
    return `{\n${entries.map(([k, x]) => `${inner}${JSON.stringify(k)}: ${write(x, inner)}`).join(",\n")}\n${pad}}`;
  };
  return write(value, "");
}
