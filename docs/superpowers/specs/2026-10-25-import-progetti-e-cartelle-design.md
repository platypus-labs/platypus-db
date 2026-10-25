# Import di progetti DBeaver e cartelle

**Data:** 25 ottobre 2026
**Stato:** design approvato in chat, da rivedere per iscritto
**Mockup:** `.superpowers/brainstorm/` (locale, non versionato): `import-dialog.html` (scelta A)
e `rail-navigator.html` (approvato)

## Obiettivo

Il dialog "Import connections" è una tendina e una lista piatta, e importa solo connessioni.
Chi arriva da DBeaver ha progetti e cartelle: deve ritrovarli in PlatypusDB così come li aveva.

Tre lavori legati:

1. **Cartelle di connessioni** dentro un progetto, annidabili, nel Navigator.
2. **Cartelle di progetti** nella barra dei progetti, su un livello, come le cartelle di
   server di Discord.
3. **Import di un progetto DBeaver esportato (`.dbp`)** e di un `data-sources.json` con le
   sue cartelle, in un nuovo dialog di import.

**Successo:**
- Un `.dbp` con due progetti e cartelle annidate diventa due progetti PlatypusDB con le
  stesse cartelle e le stesse connessioni, raccolti a scelta in una cartella della barra.
- Le cartelle si creano, rinominano, spostano ed eliminano anche a mano.
- I file di configurazione v4 si aprono senza perdite.

**Fuori ambito:** password (non si importano, come oggi), script SQL dei progetti DBeaver,
cartelle nella vista "tutte le connessioni" (senza progetto attivo resta una lista piatta),
cartelle di progetti annidate.

## Modello dei dati (`platypus-store`)

In `connections.rs`:

```rust
pub struct ConnectionFolder {
    pub id: Uuid,
    pub name: String,
    /// `None` alla radice del progetto.
    pub parent: Option<Uuid>,
}

pub struct Project {
    // campi esistenti …
    #[serde(default)]
    pub folders: Vec<ConnectionFolder>,
    /// Cartella di ogni connessione del progetto; assente = radice.
    #[serde(default)]
    pub placement: HashMap<Uuid, Uuid>,
}

pub struct ProjectFolder {
    pub id: Uuid,
    pub name: String,
    /// `#rrggbb`.
    pub color: String,
    /// In ordine.
    pub projects: Vec<Uuid>,
    #[serde(default)]
    pub collapsed: bool,
}
```

- Una connessione ha al più una cartella per progetto; in due progetti può stare in due
  cartelle diverse.
- Un progetto sta in al più una `ProjectFolder`.
- L'ordine della barra: `rail: Vec<RailItem>` con `RailItem = Project(Uuid) | Folder(Uuid)`.
  I progetti assenti da `rail` e da ogni cartella vanno in coda (file v4, progetti nuovi).
- `FILE_VERSION` 4 → 5. Un file v4 si legge senza cartelle e con l'ordine attuale dei
  progetti; si riscrive v5 al primo salvataggio. Un'app vecchia rifiuta v5, come già oggi.

**Operazioni dello store**, ognuna con test:
- cartelle di connessioni: crea (nome non vuoto, unico tra le sorelle ignorando le maiuscole),
  rinomina, sposta (rifiuta i cicli), elimina (figli e connessioni salgono al genitore);
  sposta una connessione in una cartella o alla radice;
- togliere una connessione dal progetto ne toglie il `placement`; eliminare una connessione
  la toglie da tutti i progetti;
- cartelle di progetti: crea da due progetti, aggiungi, togli (una cartella vuota sparisce), rinomina, colore, apri/chiudi, riordina la barra;
- eliminare un progetto lo toglie da `rail` e dalla sua cartella.

## Import (`platypus-store/src/import.rs`)

- `ImportSource::Dbeaver` accetta sia `data-sources.json` sia `.dbp` (riconosciuto dalla firma
  zip, non dall'estensione).
- Un `.dbp` è uno zip: `meta.xml` elenca i progetti, ognuno con
  `projects/<nome>/.dbeaver/data-sources.json`. Lettura con il crate `zip` (già nel
  workspace, `default-features = false` + `deflate`). Un progetto senza `data-sources.json`
  si salta.
- In `data-sources.json` le cartelle sono in `folders` (chiave = percorso `A/B`, campo
  `parent`) e ogni connessione ha `folder`. Le cartelle mancanti in `folders` ma usate da una
  connessione si creano dal percorso.
- Risultato:

  ```rust
  pub struct ImportedProject {
      /// `None` per un file da solo (data-sources.json, DataGrip, pgAdmin, pg_service).
      pub name: Option<String>,
      pub folders: Vec<ConnectionFolder>,
      pub connections: Vec<SavedConnection>,
      pub placement: HashMap<Uuid, Uuid>,
  }
  ```

  `import()` restituisce `Vec<ImportedProject>`. DataGrip, pgAdmin e pg_service danno un solo
  progetto senza nome e senza cartelle (pgAdmin: i gruppi di server diventano cartelle).
- `detect()` invariato.

**Salvataggio** (`save_imported`), un piano scelto nel dialog:

```rust
pub struct ImportPlan {
    pub projects: Vec<ImportTarget>,
    /// Raccoglie i progetti nuovi in una cartella della barra con questo nome.
    pub rail_folder: Option<String>,
}
pub struct ImportTarget {
    pub project: ImportedProject,   // solo le connessioni scelte
    pub into: ImportInto,           // NewProject { name } | Existing(Uuid) | None
}
```

- `NewProject`: colore automatico (palette esistente dei progetti), nome già usato → suffisso
  ` (2)`, ` (3)`…
- `Existing`: le cartelle si uniscono per percorso con quelle già presenti.
- `None` (nessun progetto attivo e file senza progetti): solo connessioni, come oggi.
- Tutto in un solo salvataggio del file: o entra tutto o niente.

## Dialog di import (layout A)

`apps/desktop/src/components/ImportDialog.tsx`, riscritto:

- Titolo "Import connections and projects".
- **Tessere delle sorgenti** in griglia: DBeaver (`.dbp · data-sources.json`), DataGrip,
  pgAdmin, pg_service, più una tessera per ogni file trovato da `detect()`. Click su una tessera
  di file trovato = lettura immediata; su una sorgente = la seleziona per la drop zone.
- **Drop zone**: trascinare il file nel dialog o "Choose file…"; mostra il nome del file letto.
- **Albero di anteprima**: progetto › cartelle › connessioni, con checkbox a tre stati (una
  cartella spuntata spunta tutto sotto). Accanto a ogni progetto un select "New project" /
  "Merge into <progetto>". Le connessioni già salvate sono deselezionate con "already saved".
  Righe di connessione come oggi: icona del driver, `user@host:port/db`, SSH, tipo.
- **Piede**: "Put in a rail folder: <nome>" (visibile con 2+ progetti nuovi, nome = file senza
  estensione), Cancel, "Import N".
- Errori di lettura nella notice esistente.

Icone: solo da `ui/Icon.tsx` (Lucide: `Folder`, `FolderOpen`, `FolderPlus`, `ChevronRight`,
`ChevronDown`…) e le icone dei driver già usate nel Navigator. Niente emoji.

## Navigator

- Con un progetto attivo, le connessioni sono un albero: cartelle prima (ordine alfabetico),
  poi connessioni libere. Cartella: `ChevronRight/Down` + `Folder/FolderOpen`; aperta/chiusa
  ricordato per progetto (stato UI locale, non nel file).
- Menu contestuale della cartella: New connection here, New subfolder, Rename, Delete folder.
  Menu della connessione: Move to folder ›. Menu dell'area vuota: New folder.
- Drag & drop di connessioni e cartelle tra cartelle e alla radice.
- La ricerca del Navigator apre le cartelle che contengono un risultato.

## Barra dei progetti

- Trascinare un progetto su un altro crea una cartella ("New folder", colore del primo).
- Cartella chiusa: quadrato con griglia 2×2 dei colori dei primi 4 progetti. Aperta: intestazione
  con `FolderOpen` nel colore della cartella, poi i progetti, tutto su uno sfondo comune.
- Click sulla cartella: apre/chiude. Click destro: Rename, Color, Ungroup.
- Trascinare un progetto fuori o tra cartelle; trascinare per riordinare la barra.
- Il progetto attivo dentro una cartella chiusa: indicatore sulla cartella.

## Comandi Tauri

`read_import` → `Vec<ImportedProject>`; `save_imported(plan)` → stato aggiornato (connessioni,
progetti, barra). Nuovi comandi per le operazioni sulle cartelle, uno per operazione dello
store. Tipi TypeScript in `api.ts`.

## Test

- **Store**: le operazioni sopra, migrazione v4 → v5, piano di import (nuovo, unione per
  percorso, suffisso del nome, cartella della barra, tutto o niente).
- **Import**: `.dbp` di prova in `crates/platypus-store/tests/fixtures/` (zip con due progetti,
  cartelle annidate, una cartella solo implicita); `data-sources.json` con cartelle; pgAdmin con
  gruppi.
- **E2E**: `import-dbeaver-project.mjs` (importa il fixture, controlla barra e albero) e
  `folders.mjs` (crea, sposta, elimina cartelle nel Navigator; crea e scioglie una cartella
  nella barra), secondo le regole E2E di CLAUDE.md.

## Gruppo

Gruppo **F — Connettività e credenziali** (`✅ fatto (P1, P2)`, l'import delle connessioni è suo).
Branch `group-f-import-folders`. File condivisi toccati (`connections.rs`, `api.ts`,
`Navigator.tsx`, `ProjectRail.tsx`) in commit piccoli e separati. Nuove righe nella tabella del
gruppo F in ROADMAP.md.
