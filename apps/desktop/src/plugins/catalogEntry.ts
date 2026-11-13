// What the catalog page offers for an entry. Kept apart from the page, so that a test can run it.

interface EntryState {
  status: "new" | "update" | "current" | "older";
}

/** A step back asks first, and the install then says it was asked for: the backend refuses one
 *  otherwise. */
export function entryAction(item: EntryState): { confirmDowngrade: boolean } {
  return { confirmDowngrade: item.status === "older" };
}

/** The choices when removing a plugin: whether anything of its settings can stay. A secret never
 *  does, so a plugin with only secrets has nothing to keep. */
export function keepableSettings(fields: readonly { type: string }[] | undefined): boolean {
  return (fields ?? []).some((f) => f.type !== "secret");
}
