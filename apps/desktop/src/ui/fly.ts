/** Moves a copy of `from` onto `to` (an item taken somewhere else, e.g. a history entry into
 *  the editor), then calls `landed`. With reduced motion, or nothing to fly between, `landed`
 *  runs at once. */
export function flyInto(from: Element | null, to: Element | null, landed: () => void): void {
  const reduce = window.matchMedia?.("(prefers-reduced-motion: reduce)").matches;
  if (!from || !to || reduce || typeof Element.prototype.animate !== "function") {
    landed();
    return;
  }
  const a = from.getBoundingClientRect();
  const b = to.getBoundingClientRect();
  const ghost = from.cloneNode(true) as HTMLElement;
  ghost.classList.add("fly-ghost");
  Object.assign(ghost.style, {
    position: "fixed",
    left: `${a.left}px`,
    top: `${a.top}px`,
    width: `${a.width}px`,
    height: `${a.height}px`,
    margin: "0",
    zIndex: "2000",
    pointerEvents: "none",
  });
  document.body.appendChild(ghost);
  // Toward the target's top left corner, shrinking to a small card on the way.
  const scale = Math.min(1, 160 / Math.max(a.width, 1));
  const dx = b.left - a.left;
  const dy = b.top - a.top;
  const flight = ghost.animate(
    [
      { transform: "translate(0, 0) scale(1)", opacity: 1, transformOrigin: "top left" },
      { transform: `translate(${dx}px, ${dy}px) scale(${scale})`, opacity: 0.2, transformOrigin: "top left" },
    ],
    { duration: 380, easing: "cubic-bezier(0.3, 0.7, 0.2, 1)" },
  );
  let done = false;
  const finish = () => {
    if (done) return;
    done = true;
    ghost.remove();
    landed();
  };
  flight.finished.then(finish, finish);
  // A tab hidden mid-flight pauses the animation: the text goes in all the same.
  setTimeout(finish, 600);
}
