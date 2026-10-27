/**
 * `value` formatted by `format`, from its exact decimal text when the backend sent one: a
 * BIGINT above 2^53 keeps every digit instead of the nearest `f64`.
 */
export function formatExact(format: Intl.NumberFormat, value: number, exact: string | null | undefined): string {
  if (exact == null) return format.format(value);
  if (/^-?\d+$/.test(exact)) return format.format(BigInt(exact));
  // Intl.NumberFormat formats a decimal string as written, without going through a double.
  return format.format(exact as unknown as number);
}
