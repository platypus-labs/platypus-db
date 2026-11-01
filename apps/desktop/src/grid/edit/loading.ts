/** Whether F2 and Enter are offered on a cell: while the columns load they still are, to say so. */
export const offersEdit = (edit: { editable: boolean; loading: boolean } | undefined) =>
  edit !== undefined && (edit.editable || edit.loading);
