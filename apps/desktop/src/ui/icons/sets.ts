/** The icon families to choose from; the labels are proper names and are not translated. */
export type IconSet = "lucide" | "tabler" | "phosphor";

export const ICON_SETS: { id: IconSet; label: string }[] = [
  { id: "lucide", label: "Lucide" },
  { id: "tabler", label: "Tabler" },
  { id: "phosphor", label: "Phosphor" },
];
