// The pages the reader went through in the guide, so that Back returns to where they were.
// No app imports: the unit tests load it as it is.

export interface Place {
  page: string;
  anchor: string | null;
  /** Where the reader was scrolled when they left this place; `null` before they leave it. */
  scroll: number | null;
}

/** The trail after going to `next`: the place left keeps the scroll position it was left at. */
export function advance(trail: Place[], scroll: number, next: Omit<Place, "scroll">): Place[] {
  const last = trail.length - 1;
  return [...trail.map((p, i) => (i === last ? { ...p, scroll } : p)), { ...next, scroll: null }];
}

/** The trail after Back; the same trail when there is nothing to go back to. */
export function back(trail: Place[]): Place[] {
  return trail.length < 2 ? trail : trail.slice(0, -1);
}

/** Where to scroll to on arriving at `place`: the position it was left at, else its heading. */
export function arrival(place: Place): { top: number } | { anchor: string | null } {
  return place.scroll === null ? { anchor: place.anchor } : { top: place.scroll };
}
