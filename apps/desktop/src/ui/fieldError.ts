/** What is wrong with a field the browser finds invalid: read by ui/fieldErrors.ts, kept apart
 *  from the interface so it can be tested alone. */

/** The part of a field `fieldError` reads. */
interface Field {
  type?: string;
  min?: string;
  max?: string;
  minLength?: number;
  validationMessage?: string;
  validity: Partial<ValidityState>;
}

export type FieldErrorKind = "tick" | "fill" | "min" | "max" | "short" | "own" | "number" | "invalid";

/** What is wrong with `field`, and the value its message names. */
export function fieldError(field: Field): { kind: FieldErrorKind; value?: string | number } {
  const v = field.validity;
  if (v.valueMissing) return { kind: field.type === "checkbox" ? "tick" : "fill" };
  if (v.rangeUnderflow) return { kind: "min", value: field.min ?? "" };
  if (v.rangeOverflow) return { kind: "max", value: field.max ?? "" };
  if (v.tooShort) return { kind: "short", value: field.minLength ?? 0 };
  // A message the form set itself (`setCustomValidity`) is already the app's.
  if (v.customError && field.validationMessage) return { kind: "own", value: field.validationMessage };
  if (v.badInput || v.stepMismatch) return { kind: "number" };
  return { kind: "invalid" };
}
