// Which catalog entries move when the app renames an object or edits a table. Pure, so it can be
// tested without the backend.

export interface CatalogMove {
  from: { object: string; column: string };
  to: { object: string; column: string };
}

interface NamedDesign {
  name: string;
  columns: { name: string; original: string | null }[];
}

/** Column renames first, while the entries still sit under the old table name, then the table. */
export function tableCatalogMoves(original: { name: string }, design: NamedDesign): CatalogMove[] {
  const moves: CatalogMove[] = [];
  for (const c of design.columns) {
    if (c.original && c.original !== c.name) {
      moves.push({ from: { object: original.name, column: c.original }, to: { object: original.name, column: c.name } });
    }
  }
  if (original.name !== design.name) {
    moves.push({ from: { object: original.name, column: "" }, to: { object: design.name, column: "" } });
  }
  return moves;
}

/** The catalog name of an object after a rename; a routine keeps its "(args)" signature. */
export function renamedObject(name: string, newName: string): string | null {
  const bare = name.replace(/\(.*$/, "");
  return newName && newName !== bare ? newName + name.slice(bare.length) : null;
}
