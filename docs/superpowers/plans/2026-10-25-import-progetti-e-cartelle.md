# Import di progetti DBeaver e cartelle: piano di implementazione

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** cartelle di connessioni nei progetti, cartelle di progetti nella barra, import di `.dbp`
DBeaver con progetti e cartelle in un nuovo dialog.

**Architecture:** il modello e le operazioni stanno in `platypus-store` (file `connections.toml`
v5, submodulo `connections/folders.rs` per non gonfiare `connections.rs`). L'import produce
`ImportedProject` e la richiesta di salvataggio viene applicata dallo store con una sola
scrittura del file. Il frontend ha un modulo puro per l'albero (`projects/folders.ts`), che
il Navigator usa per costruire le righe, e un `ProjectRail` che raggruppa i progetti per
cartella.

**Tech Stack:** Rust (serde, toml, crate `zip` 9), Tauri 2, React + TypeScript, Lucide via
`ui/Icon.tsx`, E2E con gli scenari `.mjs`.

**Spec:** `docs/superpowers/specs/2026-10-25-import-progetti-e-cartelle-design.md`

## Global Constraints

- `FILE_VERSION` = 5; i file v4 si leggono, un'app vecchia rifiuta v5.
- Password mai importate (`save_password = false`, `PasswordUpdate::Clear`).
- Icone solo da `ui/Icon.tsx` (Lucide) e quelle dei driver già usate; niente emoji.
- Testi UI in inglese, come il resto dell'app.
- clippy pedantic `-D warnings`: funzioni sotto le 100 righe, niente `unwrap`/`expect` nelle
  funzioni pubbliche.
- Commit Conventional Commits in inglese, senza righe di attribuzione; `git add <file>`.
- File condivisi (`connections.rs`, `api.ts`, `Navigator.tsx`, `App.tsx`, `ProjectRail.tsx`)
  in commit piccoli e separati.

**Scostamento dallo spec (semplificazione):** niente `rail: Vec<RailItem>`. L'ordine della
barra è l'ordine di `projects`; una cartella si mostra nella posizione del suo primo progetto
e i suoi progetti vengono resi contigui nell'ordine di `ProjectFolder.projects`. Stesso
comportamento con meno stato.

## Review Focus

1. `.dbp` con un progetto senza `data-sources.json` o con `meta.xml` assente: si importano gli
   altri progetti, nessun errore (si scoprono le cartelle `projects/*/` dallo zip).
2. Connessione con `folder` che non compare in `folders` (cartella implicita, anche annidata
   `A/B/C`): vengono create tutte le cartelle del percorso.
3. Import che unisce in un progetto esistente con una cartella dello stesso percorso: niente
   duplicati, le connessioni finiscono nella cartella esistente.
4. Eliminare una connessione, o toglierla dal progetto, lascia un `placement` orfano: va
   rimosso, e l'apertura di un file modificato a mano ripulisce placement e cartelle con
   `parent` sconosciuto.
5. Spostare una cartella dentro una sua discendente: va rifiutato (`StoreError::Invalid`).

---

### Task 1: modello v5 e cartelle di connessioni nello store

**Files:**
- Modify: `crates/platypus-store/src/connections.rs` (tipi, `FILE_VERSION`, `ConnectionsFile`,
  `FileOut`, `open`, `delete`, `set_member`, `mod folders;`)
- Create: `crates/platypus-store/src/connections/folders.rs` (operazioni + test)
- Modify: `crates/platypus-store/src/lib.rs` (export dei nuovi tipi)

**Interfaces:**
- Produces:
  - `pub struct ConnectionFolder { pub id: Uuid, pub name: String, pub parent: Option<Uuid> }`
  - `Project.folders: Vec<ConnectionFolder>`, `Project.placement: BTreeMap<Uuid, Uuid>`
    (`#[serde(default)]` su entrambi)
  - `ConnectionStore::create_folder(&mut self, project: Uuid, name: &str, parent: Option<Uuid>) -> Result<Uuid>`
  - `rename_folder(&mut self, project: Uuid, folder: Uuid, name: &str) -> Result<()>`
  - `move_folder(&mut self, project: Uuid, folder: Uuid, parent: Option<Uuid>) -> Result<()>`
  - `delete_folder(&mut self, project: Uuid, folder: Uuid) -> Result<()>`
  - `place_connection(&mut self, project: Uuid, connection: Uuid, folder: Option<Uuid>) -> Result<()>`
  - `Project::folder_path(&mut self, path: &[&str]) -> Uuid` (crea i pezzi mancanti, cerca per
    nome ignorando le maiuscole tra le sorelle; usato dall'import nel task 4)

- [ ] **Step 1: test** in `folders.rs`:
  `create_rename_delete_folder` (nome vuoto → `Invalid`; nome uguale tra sorelle ignorando le
  maiuscole → `Invalid`; stesso nome sotto genitori diversi ok; delete fa salire figli e
  connessioni al genitore), `move_folder_rejects_cycles` (A ⊃ B, sposta A in B → `Invalid`),
  `place_requires_membership` (connessione non nel progetto → `NotFound`), `removing_cleans_placement`
  (`set_membership(.., false)` e `delete` tolgono il placement), `folder_path_creates_missing`
  (`["A","B"]` due volte → stesso id, 2 cartelle), `v4_file_reads_and_writes_v5`
  (file v4 con un progetto → `folders` vuoto, riscritto con `version = 5`),
  `open_drops_dangling` (file v5 con placement verso connessione/cartella sconosciuta e cartella
  con `parent` sconosciuto → placement rimosso, cartella alla radice).
- [ ] **Step 2:** `cargo test -p platypus-store folders` → FAIL (non compila).
- [ ] **Step 3:** implementare. `set_member(.., false)` toglie anche il placement; `delete` lo toglie
  da tutti i progetti; `open` ripulisce come nel test. Aggiornare `reads_a_v1_file_and_writes_v4`
  (ora `version = 5`, rinominarlo `..._writes_v5`) e il commento di `FILE_VERSION`
  ("5 folders").
- [ ] **Step 4:** `cargo test -p platypus-store` → PASS; `cargo clippy -p platypus-store --all-targets` pulito.
- [ ] **Step 5:** commit `feat(store): connection folders inside projects`.

### Task 2: cartelle di progetti (barra)

**Files:**
- Modify: `crates/platypus-store/src/connections.rs` (campo `project_folders` in store, file in/out,
  `delete_project` lo pulisce)
- Modify: `crates/platypus-store/src/connections/folders.rs`

**Interfaces:**
- Produces:
  - `pub struct ProjectFolder { pub id: Uuid, pub name: String, pub color: String, pub projects: Vec<Uuid>, #[serde(default)] pub collapsed: bool }`
  - `project_folders(&self) -> &[ProjectFolder]`
  - `group_projects(&mut self, target: Uuid, dragged: Uuid) -> Result<Uuid>`: se `target` è in una
    cartella, `dragged` vi si aggiunge in coda; altrimenti nasce "New folder" col colore di
    `target`, con `[target, dragged]`. `dragged` esce dalla cartella precedente.
  - `save_project_folder(&mut self, folder: ProjectFolder) -> Result<()>` (nome non vuoto, `is_color`)
  - `set_project_folder(&mut self, project: Uuid, folder: Option<Uuid>, index: Option<usize>) -> Result<()>`
    (sposta in una cartella alla posizione data, o fuori)
  - `ungroup(&mut self, folder: Uuid) -> Result<()>`
  - `reorder_projects(&mut self, order: &[Uuid]) -> Result<()>` (permutazione dei progetti
    esistenti, altrimenti `Invalid`)

- [ ] **Step 1: test** `group_creates_then_extends`, `project_in_one_folder_only`,
  `empty_folder_disappears` (togliere l'ultimo progetto o eliminarlo con `delete_project`),
  `reorder_must_be_permutation`, `project_folders_roundtrip` (scrittura e rilettura del file).
- [ ] **Step 2:** `cargo test -p platypus-store project_folder` → FAIL.
- [ ] **Step 3:** implementare.
- [ ] **Step 4:** test e clippy verdi.
- [ ] **Step 5:** commit `feat(store): project folders in the rail`.

### Task 3: import di cartelle DBeaver, `.dbp` e gruppi pgAdmin

**Files:**
- Modify: `crates/platypus-store/src/import.rs` (diventa `import/mod.rs` solo se supera ~600
  righe; altrimenti nuovo file `import/dbp.rs` con `mod dbp;` dentro `import.rs`)
- Modify: `crates/platypus-store/Cargo.toml` (`zip = { version = "9", default-features = false, features = ["deflate"] }`)
- Create: `crates/platypus-store/tests/fixtures/two-projects.dbp` (generato dal test helper,
  vedi step 1) e test in `import.rs`

**Interfaces:**
- Produces:
  - `pub struct ImportedProject { pub name: Option<String>, pub folders: Vec<ImportedFolder>, pub connections: Vec<SavedConnection> }`
  - `pub struct ImportedFolder { pub path: Vec<String> }` e `SavedConnection` resta senza
    cartella: la cartella di ogni connessione è in `ImportedProject.placement: BTreeMap<Uuid, Vec<String>>`
    (percorso, non id: lo store risolve con `Project::folder_path`).
  - `pub fn import(source: ImportSource, path: &Path) -> Result<Vec<ImportedProject>>`
    (sostituisce il vecchio ritorno `Vec<SavedConnection>`; `import_connections` in `lib.rs`
    segue).

- [ ] **Step 1: test**
  - `dbeaver_folders`: `data-sources.json` con `"folders": {"Prod": {}, "Prod/Replica": {"parent": "Prod"}}`
    e connessioni con `"folder": "Prod/Replica"` e `"folder": "Dev/Local"` (implicita) → un progetto
    con `name: None`, cartelle `["Prod"]`, `["Prod","Replica"]`, `["Dev"]`, `["Dev","Local"]`, placement corretti.
  - `dbp_two_projects`: zip costruito nel test con `zip::ZipWriter` in un `tempdir`
    (`meta.xml` con `<projects><project name="Cliente Rossi"/><project name="Interno"/></projects>`,
    `projects/Cliente Rossi/.dbeaver/data-sources.json`, `projects/Interno/.dbeaver/data-sources.json`,
    più `projects/Vuoto/` senza file) → 2 progetti, nomi esatti, ordine di `meta.xml`.
  - `dbp_without_meta`: stesso zip senza `meta.xml` → progetti scoperti dai percorsi.
  - `dbeaver_source_accepts_zip`: `import(ImportSource::Dbeaver, path)` su un `.dbp` e su un
    `.json` (riconoscimento dai primi byte `PK\x03\x04`).
  - `pgadmin_groups_become_folders`: il JSON del test esistente (`"Group": "Team"`) → cartella
    `["Team"]` con la connessione dentro.
  - Aggiornare i test esistenti al nuovo tipo di ritorno.
- [ ] **Step 2:** `cargo test -p platypus-store import` → FAIL.
- [ ] **Step 3:** implementare. `meta.xml` letto con una ricerca semplice degli attributi
  `name="…"` dentro i tag `<project` (niente dipendenza XML nuova; DataGrip usa già un parser
  testuale in questo file, riusarne lo stile). Entità `&amp;` ecc. decodificate.
- [ ] **Step 4:** test e clippy verdi.
- [ ] **Step 5:** commit `feat(import): DBeaver projects (.dbp) and connection folders`.

### Task 4: applicare una richiesta di import

**Files:**
- Modify: `crates/platypus-store/src/connections/folders.rs` (o nuovo `connections/import.rs`)
- Modify: `crates/platypus-store/src/lib.rs`

**Interfaces:**
- Consumes: `ImportedProject` (task 3), `folder_path` (task 1), `ProjectFolder` (task 2),
  `PROJECT_COLORS` replicata in Rust come `const PROJECT_COLORS: [&str; 8]` con gli stessi
  valori di `ProjectDialog.tsx`.
- Produces:
  - `pub enum ImportInto { NewProject { name: String }, Existing(Uuid), None }` (serde
    `tag = "kind"`, `rename_all = "snake_case"`)
  - `pub struct ImportTarget { pub project: ImportedProject, pub into: ImportInto }`
  - `pub struct ImportRequest { pub targets: Vec<ImportTarget>, pub rail_folder: Option<String> }`
  - `ConnectionStore::apply_import(&mut self, request: ImportRequest) -> Result<()>`

- [ ] **Step 1: test** `import_new_projects_into_rail_folder` (2 target `NewProject`,
  `rail_folder: Some("lavoro")` → 2 progetti, cartella "lavoro" con entrambi, colori dalla
  palette), `import_name_taken_gets_suffix` ("Interno" esiste → "Interno (2)", poi "Interno (3)"),
  `import_merges_folders_by_path` (Review Focus 3), `import_none_saves_only_connections`,
  `import_is_all_or_nothing` (target `Existing(id sconosciuto)` → errore e nessuna connessione
  salvata, file invariato).
- [ ] **Step 2:** FAIL.
- [ ] **Step 3:** implementare: validare tutto prima, poi mutare e chiamare `write()` una volta.
  Connessioni salvate come `save(.., PasswordUpdate::Clear)` fa oggi, ma senza scrivere a ogni
  connessione (estrarre un `insert_connection` privato senza `write`).
- [ ] **Step 4:** test e clippy verdi.
- [ ] **Step 5:** commit `feat(store): apply an import of projects, folders and connections`.

### Task 5: comandi Tauri e `api.ts`

**Files:**
- Modify: `apps/desktop/src-tauri/src/commands/connections.rs` (`read_import`, `save_imported`)
- Modify: `apps/desktop/src-tauri/src/commands/projects.rs` (comandi cartelle)
- Modify: registrazione comandi (dove sono registrati `save_project` ecc., `lib.rs`/`main.rs`)
- Modify: `apps/desktop/src/api.ts`

**Interfaces:**
- Produces (Tauri → TS):
  - `read_import(source, path) -> Vec<ImportedProject>`
  - `save_imported(request: ImportRequest) -> Workspace` dove
    `#[derive(Serialize)] pub struct Workspace { connections: Vec<SavedConnection>, projects: Vec<Project>, project_folders: Vec<ProjectFolder> }`
  - `list_project_folders() -> Vec<ProjectFolder>`
  - comandi cartelle di connessioni, tutti `-> Vec<Project>`: `create_connection_folder(project_id, name, parent)`,
    `rename_connection_folder(project_id, folder_id, name)`, `move_connection_folder(project_id, folder_id, parent)`,
    `delete_connection_folder(project_id, folder_id)`, `place_connection(project_id, connection_id, folder_id)`
  - comandi barra, tutti `-> Workspace` (cambiano ordine e cartelle insieme):
    `group_projects(target, dragged)`, `save_project_folder(folder)`, `set_project_folder(project_id, folder_id, index)`,
    `ungroup_projects(folder_id)`, `reorder_projects(order)`
  - TS: `ConnectionFolder`, `ProjectFolder`, `ImportedProject`, `ImportInto`, `ImportRequest`,
    `Workspace`; `Project` guadagna `folders` e `placement: Record<string, string>`; i metodi
    `api.*` con gli stessi nomi in camelCase. `ImportPlan` esiste già (transfer): non riusarlo.

- [ ] **Step 1:** implementare (glue sottile, nessuna logica: è coperta dai test dello store).
- [ ] **Step 2:** `cargo clippy --workspace --all-targets` e `npm --prefix apps/desktop run build`
  (il TS si rompe in `ImportDialog.tsx` finché non c'è il task 6: in questo task adattare
  `ImportDialog` al minimo, appiattendo i progetti, così il build resta verde).
- [ ] **Step 3:** commit `feat(desktop): commands for folders and project imports`.

### Task 6: nuovo dialog di import (layout A)

**Files:**
- Modify: `apps/desktop/src/components/ImportDialog.tsx` (riscritto)
- Create: `apps/desktop/src/components/importTree.ts` (stato puro: selezione a tre stati, target)
- Modify: CSS del dialog (stesso foglio di `.import-list`, `.import-found`)
- Modify: `apps/desktop/src/App.tsx` (`onImported(workspace)` aggiorna connessioni, progetti e cartelle)

**Interfaces:**
- Consumes: `api.readImport`, `api.saveImported`, `ImportedProject`, `ImportRequest`.
- Produces: `Props = { existing: SavedConnection[]; projects: Project[]; project: string | null; onImported: (w: Workspace) => void; onClose: () => void }`

- [ ] **Step 1:** `importTree.ts`: `checkState(node) -> "on" | "off" | "mixed"`, `toggle(node)`,
  `request(projects, picked, targets, railFolder): ImportRequest` (scarta progetti senza
  connessioni scelte). Default del target: `NewProject { name }` per i progetti con nome;
  `Existing(project)` se c'è un progetto attivo e il file non ha nomi, altrimenti `None`.
- [ ] **Step 2:** UI come il mockup A: titolo "Import connections and projects"; griglia di tessere
  (DBeaver "`.dbp` · data-sources.json", DataGrip, pgAdmin, pg_service, più una tessera per file
  trovato con `detect`); drop zone (evento drag-drop di Tauri `getCurrentWebview().onDragDropEvent`)
  con "Choose file…"; albero progetto › cartelle › connessioni con checkbox tri-stato (icone
  `folder`/`folder-open`, chevron, icona del driver come nel Navigator); select per progetto
  "New project" / "Merge into <nome>" (tutti i progetti esistenti); piede con
  "Put in a rail folder: [nome]" visibile con 2+ `NewProject` (default: nome del file senza
  estensione), Cancel, "Import N".
- [ ] **Step 3:** `npm --prefix apps/desktop run build` verde.
- [ ] **Step 4:** commit `feat(desktop): import dialog with projects and folders`.

### Task 7: cartelle nel Navigator

**Files:**
- Create: `apps/desktop/src/projects/folders.ts` (albero puro + azioni del menu)
- Modify: `apps/desktop/src/components/Navigator.tsx`
- Modify: `apps/desktop/src/ui/Icon.tsx` se manca un'icona (es. `folder-input` per "Move to folder")

**Interfaces:**
- Produces:
  - `type FolderNode = { folder: ConnectionFolder; children: FolderNode[]; connections: SavedConnection[] }`
  - `folderTree(project: Project, connections: SavedConnection[]): { folders: FolderNode[]; loose: SavedConnection[] }`
    (cartelle in ordine alfabetico, connessioni nell'ordine del progetto)
  - azioni registrate con `registerAction`: scope nuovo `navigator.folder` (`{ project, folder }`):
    "New connection here", "New subfolder…", "Rename…", "Delete folder"; scope
    `navigator.connection`: "Move to folder…" (solo con progetto attivo); azione "New folder…"
    nel menu dell'area vuota/intestazione.

- [ ] **Step 1:** nel Navigator estrarre la costruzione delle righe di una connessione in
  `connectionRows(c): Row[]` (nessun cambiamento di comportamento), poi con un progetto attivo
  camminare `folderTree`: riga cartella (`depth = livello`, icona `folder`/`folder-open`,
  chevron), righe di connessione spostate di `livello` (`depth + level`). Senza progetto: lista
  piatta come oggi. Chiave di espansione `folder:<id>`, ricordata in `localStorage` per progetto
  (try/catch come `state.ts`).
- [ ] **Step 2:** con un filtro attivo, le cartelle che contengono connessioni visibili si aprono.
- [ ] **Step 3:** drag & drop HTML5 sulle righe di connessione e cartella (`draggable`,
  `dataTransfer` con `application/x-platypus-connection` o `-folder`); drop su cartella →
  `placeConnection`/`moveConnectionFolder`; drop sull'intestazione → radice.
- [ ] **Step 4:** "New subfolder…"/"Rename…"/"New folder…" usano il prompt dell'app (stesso
  componente usato per "Rename" degli script in `ScriptsPanel.tsx`); "New connection here" apre
  il dialog di connessione e dopo il salvataggio chiama `placeConnection`.
- [ ] **Step 5:** build verde; commit `feat(desktop): connection folders in the navigator`.

### Task 8: cartelle nella barra dei progetti

**Files:**
- Modify: `apps/desktop/src/projects/ProjectRail.tsx`
- Modify: `apps/desktop/src/projects/actions.ts` (ordine con le cartelle; menu cartella)
- Modify: `apps/desktop/src/workbench.ts` e `App.tsx` (stato `projectFolders`)
- Modify: CSS della barra

**Interfaces:**
- Produces:
  - `railItems(projects: Project[], folders: ProjectFolder[]): RailItem[]`, con
    `RailItem = { kind: "project"; project: Project } | { kind: "folder"; folder: ProjectFolder; projects: Project[] }`
    in `projects/state.ts` (ordine descritto in "Scostamento dallo spec").
  - `order()` in `actions.ts` segue `railItems` (Ctrl+N conta i progetti anche dentro cartelle chiuse).
  - Scope azioni `project.folder`: "Rename folder…", "Folder color" (palette di `PROJECT_COLORS`), "Ungroup".

- [ ] **Step 1:** `railItems` + test unitario se il frontend ha già test (altrimenti coperto dall'E2E).
- [ ] **Step 2:** resa: cartella chiusa = `.project-folder` 2×2 con i colori dei primi 4 progetti;
  aperta = contenitore con intestazione (`folder-open` nel colore della cartella) e i quadrati.
  Click: `saveProjectFolder({...f, collapsed: !f.collapsed})`. Progetto attivo in cartella chiusa:
  classe `active` sulla cartella.
- [ ] **Step 3:** drag & drop: progetto su progetto → `groupProjects`; progetto su cartella →
  `setProjectFolder(p, f, null)`; progetto tra due elementi → `setProjectFolder(p, null, null)` +
  `reorderProjects`.
- [ ] **Step 4:** build verde; commit `feat(desktop): project folders in the rail`.

### Task 9: E2E, documentazione, verifica

**Files:**
- Create: `apps/desktop/e2e/scenarios/import-dbeaver-project.mjs`, `apps/desktop/e2e/scenarios/folders.mjs`
- Create: `apps/desktop/e2e/fixtures/two-projects.dbp` (stesso contenuto del test del task 3;
  generato una volta e committato)
- Modify: `docs/ROADMAP.md` (righe nuove nella tabella del gruppo F, "Stato"), `README.md`
  (funzionalità utente: import `.dbp`, cartelle)

- [ ] **Step 1:** scenari seguendo `projects-sqlite.mjs` e `import-sqlite.mjs` e le regole E2E di
  CLAUDE.md: `import-dbeaver-project` apre il dialog, sceglie il fixture, verifica l'albero,
  "Put in a rail folder", importa, controlla la cartella nella barra e le cartelle nel Navigator.
  `folders` crea una cartella, ci sposta una connessione (menu "Move to folder…"), la elimina e
  verifica che la connessione resti; crea e scioglie una cartella della barra dal menu.
- [ ] **Step 2:** refactor con subagente su `git diff origin/main...HEAD` (CLAUDE.md, commit `refactor:`).
- [ ] **Step 3:** verifica completa di CLAUDE.md: fmt, build frontend, clippy, test workspace,
  `tauri build --no-bundle`, `e2e`, conformance con Docker.
- [ ] **Step 4:** `/code-review`, correzioni, docs, merge in `main`, rilascio lock.
