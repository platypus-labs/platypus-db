import type { ObjectKind } from "../api";
import { ObjectTab, type ObjectSection } from "../objects/ObjectTab";
import { registerTabKind } from "./registry";

const text = (v: unknown) => (typeof v === "string" ? v : null);

registerTabKind({
  kind: "object",
  component: ObjectTab,
  create: (init) => ({
    schema: init.schema ?? "",
    currentSchema: init.currentSchema ?? false,
    name: init.name ?? "",
    objectKind: init.objectKind ?? "table",
    parent: init.parent ?? null,
    ...init,
  }),
  label: (tab) => tab.title ?? tab.name,
  persist: ({ schema, currentSchema, name, objectKind, parent, section, title, database }) => ({
    schema,
    currentSchema,
    name,
    objectKind,
    parent,
    section,
    title,
    database,
  }),
  restore: (saved) => ({
    schema: text(saved.schema) ?? "",
    currentSchema: saved.currentSchema === true,
    name: text(saved.name) ?? "",
    objectKind: (text(saved.objectKind) ?? "table") as ObjectKind,
    parent: text(saved.parent),
    section: (text(saved.section) ?? undefined) as ObjectSection | undefined,
    title: text(saved.title),
    database: text(saved.database),
  }),
});
