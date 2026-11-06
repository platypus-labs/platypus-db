// Hearing of plugins changed in another window. Kept apart from the store, without the app around it:
// the event's listener is set up once, the first time it is asked for, and not before.

/** A function that starts following at its first call, and does nothing at the others. `start`
 *  listens and calls what it is given at each change, which reloads the list. */
export function changeFollower(start: (heard: () => void) => Promise<unknown>, reload: () => void): () => void {
  let following = false;
  return () => {
    if (following) return;
    following = true;
    start(reload).catch((e: unknown) => {
      following = false;
      console.warn("plugins", e);
    });
  };
}
