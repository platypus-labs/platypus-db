/**
 * The app's icons, by name. Components use `Icon` and never import Lucide themselves: a new
 * icon is one more name here, and only the icons named here end up in the bundle.
 */
import {
  Activity,
  ArrowLeft,
  ArrowRight,
  Bookmark,
  Braces,
  Check,
  ChevronDown,
  ChevronRight,
  Code2,
  Columns2,
  Columns3,
  Copy,
  Database,
  Download,
  Eraser,
  Eye,
  FileCode,
  FilePlus,
  Folder,
  FolderOpen,
  FolderPlus,
  Hash,
  History,
  Info,
  KeyRound,
  Layers,
  Link2,
  ListOrdered,
  ListTree,
  ListVideo,
  Minus,
  type LucideIcon,
  PanelLeft,
  PanelLeftClose,
  Pencil,
  Pin,
  Play,
  Plus,
  Puzzle,
  RefreshCw,
  RotateCcw,
  Save,
  Search,
  Settings,
  Shapes,
  Square,
  SquareFunction,
  SquareTerminal,
  SunMoon,
  Table2,
  Timer,
  ToggleLeft,
  ToggleRight,
  Trash2,
  Undo2,
  Workflow,
  X,
  Zap,
} from "lucide-react";
import type { ObjectKind } from "../api";

const ICONS = {
  database: Database,
  schema: Layers,
  table: Table2,
  view: Eye,
  routine: SquareFunction,
  trigger: Zap,
  sequence: Hash,
  type: Shapes,
  extension: Puzzle,
  event: Timer,
  column: Columns3,
  key: KeyRound,
  index: ListTree,
  "fk-out": ArrowRight,
  "fk-in": ArrowLeft,
  folder: Folder,
  bookmark: Bookmark,
  sql: Code2,
  object: Info,
  run: Play,
  script: ListVideo,
  cancel: Square,
  "chevron-right": ChevronRight,
  "chevron-down": ChevronDown,
  search: Search,
  settings: Settings,
  theme: SunMoon,
  plus: Plus,
  history: History,
  open: FolderOpen,
  save: Save,
  edit: Pencil,
  import: Download,
  commit: Check,
  rollback: Undo2,
  autocommit: ToggleRight,
  "manual-commit": ToggleLeft,
  refresh: RefreshCw,
  variables: Braces,
  outline: ListOrdered,
  terminal: SquareTerminal,
  file: FileCode,
  "new-file": FilePlus,
  "new-folder": FolderPlus,
  link: Link2,
  builder: Workflow,
  minimize: Minus,
  maximize: Square,
  restore: Copy,
  close: X,
  copy: Copy,
  delete: Trash2,
  discard: Eraser,
  revert: RotateCcw,
  pin: Pin,
  split: Columns2,
  activity: Activity,
  sidebar: PanelLeft,
  "sidebar-close": PanelLeftClose,
} satisfies Record<string, LucideIcon>;

export type IconName = keyof typeof ICONS;

/** An icon in the current text color; decorative unless it has a `title`. */
export function Icon({ name, size = 14, className, title }: { name: IconName; size?: number; className?: string; title?: string }) {
  const Svg = ICONS[name];
  return (
    <Svg
      size={size}
      strokeWidth={2}
      className={className}
      aria-hidden={title ? undefined : true}
      aria-label={title}
      role={title ? "img" : undefined}
    />
  );
}

const OBJECT_ICONS: Record<ObjectKind, IconName> = {
  table: "table",
  foreign_table: "table",
  view: "view",
  materialized_view: "view",
  function: "routine",
  procedure: "routine",
  trigger: "trigger",
  sequence: "sequence",
  type: "type",
  domain: "type",
  extension: "extension",
  event: "event",
  dictionary: "table",
};

/** The icon of a kind of database object. */
export function objectIcon(kind: ObjectKind): IconName {
  return OBJECT_ICONS[kind];
}
