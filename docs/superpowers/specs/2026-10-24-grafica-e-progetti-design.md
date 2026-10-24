# Nuova grafica e progetti come spazi di lavoro

**Data:** 24 ottobre 2026
**Stato:** design approvato in chat, da rivedere per iscritto
**Mockup:** `.superpowers/brainstorm/` (locale, non versionato): `switcher-layout.html`
(scelta B) e `visual-direction.html` (scelta 2)

Due lavori distinti, ognuno con il suo piano di implementazione e il suo branch, in
quest'ordine:

1. **Parte A — Grafica** (`feat/visual-refresh`, da `main`)
2. **Parte B — Progetti** (`feat/projects`, sopra la parte A)

La grafica viene prima perché la barra dei progetti è disegnata nel nuovo stile: farla
dopo eviterebbe di ridisegnarla.

---

## Obiettivo

L'aspetto attuale è spoglio: bottoni di sistema, poche icone, gerarchia visiva debole.
L'obiettivo è un'app moderna e curata, nello stile di TablePlus e Linear, senza perdere
densità di informazione.

Chi usa PlatypusDB lavora su più clienti o progetti, ognuno con le sue connessioni. Oggi
le vede tutte insieme. Un progetto diventa uno **spazio di lavoro**: scegliendolo, l'app
mostra solo le sue connessioni, le sue tab e la sua cronologia.

**Successo:**
- Il tema predefinito corrisponde alla direzione 2 del mockup ("scura e compatta").
- Cambiare progetto è un clic (o `Ctrl+1…9`) e riporta esattamente le tab lasciate lì.
- Tutti gli scenari E2E esistenti passano senza cambiare i selettori, più i nuovi.

---

## Parte A — Grafica

### A1. Temi

Due nuovi preset in `theme.ts`, che diventano quelli usati dall'opzione "sistema":

- **Platypus Dark** (predefinito al primo avvio): fondo `#141519`, sidebar `#17181d`,
  barra progetti `#0d0e11`, linee `#23252c`, testo `#c9ccd3`, testo forte `#f2f3f5`,
  testo secondario `#6f7480`, accento viola `#6c63ff`.
- **Platypus Light**: stessa struttura e stessi rapporti di contrasto, su fondi chiari
  (fondo `#ffffff`, sidebar `#f6f7f9`, linee `#e6e8eb`, accento `#5b52f0`).

I preset `light` e `dark` attuali vengono rinominati in **Classic Light** e **Classic Dark**
e restano selezionabili, con Nord, Dracula, Solarized Light e alto contrasto. Chi aveva
scelto esplicitamente un preset lo mantiene. Chi era su "sistema" passa ai nuovi temi
Platypus.

**Nuove variabili** nella `Palette`, con un valore per ogni preset esistente (derivato
dai suoi colori dove il preset non ne ha uno proprio):

| Variabile | Uso |
|---|---|
| `bg-rail` | barra dei progetti |
| `bg-tabbar` | fondo della barra delle tab |
| `fg-strong` | titoli, tab attiva, elemento attivo |
| `border-strong` | bordi di input e bottoni (più marcati di `border`) |
| `active` | elemento selezionato nel navigator, tab attiva su sfondo colorato |
| `obj-db`, `obj-table`, `obj-view`, `obj-routine`, `obj-trigger` | icone per tipo di oggetto |
| `type-dev`, `type-dev-bg`, `type-test`, `type-test-bg`, `type-prod`, `type-prod-bg` | badge e chip dei tipi di connessione |

I colori personalizzati delle Impostazioni (accento, sfondo, sidebar, testo) continuano a
sovrascrivere il preset come oggi.

### A2. Font

- **Inter** (variabile, sottoinsieme latino) per l'interfaccia.
- **JetBrains Mono** per editor, griglia e ogni testo SQL.

Entrambi inclusi nell'app come file `woff2` in `apps/desktop/src/assets/fonts/`, caricati
con `@font-face` (circa 300 KB in tutto). Nessuna richiesta di rete: l'app deve funzionare
offline. Le dimensioni restano regolabili dalle Impostazioni come oggi (`--editor-font-size`,
zoom dell'interfaccia). Fallback: `Segoe UI Variable`, `system-ui` e `Cascadia Code`,
`Consolas`.

### A3. Icone

Nuova dipendenza: **`lucide-react`** (licenza ISC), con import per singola icona perché
nel bundle entrino solo quelle usate. Un componente `Icon` in `src/ui/Icon.tsx` centralizza
dimensione e colore, così i componenti non importano Lucide direttamente.

Mappa minima:

| Dove | Icona |
|---|---|
| connessione / database | `Database` (colore per tipo di connessione) |
| schema | `Layers` |
| tabella | `Table2` (`obj-table`) |
| vista | `Eye` (`obj-view`) |
| funzione / procedura | `SquareFunction` (`obj-routine`) |
| trigger | `Zap` (`obj-trigger`) |
| tab SQL / tab tabella / tab oggetto | `Code2` / `Table2` / `Info` |
| Run / Script / Cancel | `Play` / `ListVideo` / `Square` |
| espandi / comprimi | `ChevronRight` / `ChevronDown` |

### A4. Componenti

Le classi CSS usate dagli scenari E2E (`.tree-row`, `.tree-label`, `.tab`, `.query-tab`,
`.object-tab`, `.statusbar`, `.edit-bar`, `.modal`, `.palette`, `.notice`, `.error`,
`.type-badge`, `.empty-import`, `.import-found`, `.import-list`, `.shell`, `.tabs`, …)
**restano invariate**. Cambia lo stile, non la struttura su cui si appoggiano i test.

- **Navigator:** intestazione con il nome del progetto (Parte B; fino ad allora
  "Connections"), ricerca con l'indicazione `Ctrl K`, righe di 24px con icona colorata per
  tipo, rientro di 16px per livello, elemento attivo con sfondo `active`. I badge
  DEV/TEST/PROD diventano etichette con `type-*-bg` e `type-*`.
- **Tab:** icona per tipo, tab attiva dello stesso colore del contenuto sotto (unita),
  le altre su `bg-tabbar` in `fg-muted`. Il comportamento della rotella del gruppo K resta.
- **Toolbar dell'editor:** Run come bottone primario (accento), poi Script e Cancel, poi
  il **chip "connessione · schema"** colorato per tipo di connessione, poi Auto-commit e il
  resto. L'opzione "solo icone" del gruppo K continua a funzionare.
- **Editor:** colori della sintassi dal preset; numeri di riga in `fg-muted`.
- **Griglia:** font mono, numeri allineati a destra con cifre tabulari, `NULL` in grigio
  corsivo, intestazione con fondo `grid-header` e testo `fg-muted` in grassetto leggero.
- **Barra di stato:** testo piccolo in `fg-muted`, righe e tempo a sinistra, encoding e
  versione del server a destra.
- **Dialog, Impostazioni, palette, menu contestuali:** angoli `--radius-xl`, linee sottili,
  ombra leggera, scorciatoie mostrate a destra nei menu in un riquadro `kbd`.
- **Bottoni e input:** altezza 28px, bordo `border-strong`, focus con anello dell'accento.

### A5. Organizzazione del CSS

`styles.css` (1536 righe) viene diviso per area in `src/styles/`:

```
styles/
  index.css        importa gli altri nell'ordine giusto
  base.css         reset, font, variabili non di colore, bottoni, input, kbd
  shell.css        layout, barra dei progetti, tab, barra di stato
  navigator.css
  editor.css       toolbar e CodeMirror
  grid.css         griglia, edit bar, pannelli dei risultati
  dialogs.css      modali, Impostazioni, palette, menu, import, guardie
```

`main.tsx` importa `styles/index.css`. Ogni gruppo futuro tocca il file della sua area,
riducendo i conflitti di merge che abbiamo visto su `styles.css`.

### A6. Test

- Tutti gli scenari E2E esistenti passano senza modifiche ai selettori.
- `theme-sqlite` esteso: al primo avvio il tema è Platypus Dark; passando a Platypus Light
  cambiano le variabili su `<html>`; un preset scelto prima resta scelto.
- Screenshot di riferimento (`PLATYPUS_E2E_SHOTS`) di navigator, editor con risultati,
  Impostazioni e palette, in Dark e Light, da guardare a mano nella PR.
- Test unitario in `theme.ts`: ogni preset definisce tutte le variabili della `Palette`
  (lo garantisce già il tipo; il test copre i valori derivati).

---

## Parte B — Progetti

### B1. Modello e salvataggio

I progetti stanno in `connections.toml`, accanto alle connessioni:

```toml
version = 2

[[projects]]
id = "6f1c…"
name = "Cliente Rossi"
color = "#d08f20"
initials = "CR"          # facoltativo: se manca si calcola dal nome
connections = ["a1b2…", "c3d4…"]

[[connections]]
id = "a1b2…"
# …come oggi
```

- **Molti a molti:** una connessione può comparire in più progetti. La lista sta nel
  progetto. Modificare una connessione (password e tunnel compresi) vale ovunque.
- L'ordine dei progetti nel file è l'ordine nella barra.
- **Versione del file a 2.** Una versione precedente dell'app, leggendo un file con
  progetti, li perderebbe al primo salvataggio. Con la versione 2 la rifiuta con il messaggio
  che esiste già ("written by a newer PlatypusDB"). Un file versione 1 viene letto
  normalmente e riscritto come versione 2 al primo salvataggio.
- Il campo `folder` delle connessioni, mai usato dall'interfaccia, viene rimosso. Se un file
  lo contiene viene ignorato in lettura.
- Eliminare una connessione la toglie da tutti i progetti. Un id di connessione inesistente
  in un progetto viene scartato in lettura.

**Store (`platypus-store`):** `Project { id, name, color, initials: Option<String>,
connections: Vec<Uuid> }` e su `ConnectionStore` i metodi `projects()`, `save_project`,
`delete_project`, `set_membership(connection, project, member: bool)`. Validazione: nome
non vuoto e unico (senza distinguere maiuscole), colore `#rrggbb`, iniziali da 1 a 2
caratteri.

**Comandi Tauri:** `list_projects`, `save_project`, `delete_project`,
`set_project_membership`. `save_connection` riceve in più `projects: Vec<Uuid>`, l'elenco
completo dei progetti della connessione.

### B2. "Tutte le connessioni"

Non è un progetto salvato: è una voce fissa, sempre prima nella barra, che mostra tutte le
connessioni. Non si rinomina e non si elimina. Ha le sue tab come ogni progetto.

### B3. Barra dei progetti

Colonna di 52px a sinistra del navigator (`.project-rail`).

- Un quadrato 32×32 per progetto: colore del progetto, iniziali in bianco, tooltip con il
  nome. Quello attivo ha una barretta a sinistra.
- **+** in fondo apre il dialog del progetto: nome, colore (8 colori proposti più un
  selettore libero), iniziali precompilate dal nome e modificabili.
- **Clic destro:** Rinomina…, Colore…, Elimina progetto. L'eliminazione chiede conferma
  e dice che le connessioni restano.
- **Scorciatoie:** azioni `project.switch.1` … `project.switch.9` (`Ctrl+1…9`;
  `Ctrl+1` è "Tutte le connessioni") e `project.next` / `project.previous`
  (`Ctrl+Alt+PageDown/PageUp`; `Ctrl+Alt+↓/↑` è già dei cursori multipli del gruppo D),
  modificabili come le altre nelle Impostazioni. Il controllo dei conflitti del gruppo K le
  copre.
- La palette (Ctrl+K) ha l'azione "Passa al progetto…" e i progetti come risultati.

Iniziali calcolate: le iniziali delle prime due parole del nome ("Cliente Rossi" → "CR"),
o le prime due lettere se è una parola sola ("Interno" → "IN").

### B4. Assegnare le connessioni

- **Dialog della connessione:** campo **Progetti** con i progetti come chip
  selezionabili, nel tab Generale.
- **Navigator, clic destro su una connessione:** "Aggiungi al progetto ▸" (sottomenu con
  i progetti di cui non fa parte) e, dentro un progetto, "Rimuovi da questo progetto".
- Una connessione **creata o importata** mentre è attivo un progetto entra in quel
  progetto. In "Tutte le connessioni" non entra in nessuno.

### B5. Cambio di progetto

Lo stato dell'app ha un `activeProject: string | null` (`null` = Tutte le connessioni),
salvato tra un avvio e l'altro.

- **Navigator:** mostra solo le connessioni del progetto. Intestazione con pallino del
  colore e nome del progetto.
- **Tab:** `workspace.ts` salva le tab per progetto, con chiave
  `platypus.workspace.v2.<projectId|all>`. Cambiando progetto salva le tab correnti e
  ripristina quelle del progetto scelto, con la tab attiva. Al primo avvio dopo
  l'aggiornamento, le tab salvate in `platypus.workspace.v1` diventano quelle di "Tutte le
  connessioni".
- **Sessioni:** restano aperte in background. Le tab ripristinate si riagganciano alle
  sessioni già aperte, quindi tornare a un progetto è immediato.
- **Tab chiuse (Ctrl+Shift+T):** la pila è per progetto.
- **Cronologia:** `HistoryQuery` passa da `connection_id: Option<Uuid>` a
  `connection_ids: Option<Vec<Uuid>>`. Il pannello Cronologia e la palette filtrano sulle
  connessioni del progetto.
- **Palette:** connessioni, tabelle e query recenti solo del progetto attivo. Le azioni
  sono sempre tutte.
- **Titolo della finestra:** "Cliente Rossi — PlatypusDB", oppure solo "PlatypusDB" in
  Tutte le connessioni.
- **Transazioni aperte:** cambiare progetto non chiude nulla. Le tab di un progetto con una
  transazione aperta mantengono l'orologio del gruppo L, e il quadrato del progetto nella
  barra mostra un pallino d'avviso finché la transazione resta aperta.

### B6. Casi limite

- **Progetto eliminato mentre è attivo:** si passa a Tutte le connessioni. Le tab di quel
  progetto si chiudono, chiedendo conferma se ci sono modifiche non salvate (come la
  chiusura di una tab oggi).
- **Connessione tolta dal progetto attivo con tab aperte:** le tab restano aperte finché non
  le chiudi. La connessione sparisce solo dal navigator.
- **Progetto vuoto:** il navigator mostra "Nessuna connessione in questo progetto" con i
  bottoni Nuova connessione, Aggiungi esistenti… (lista di quelle in nessun progetto o in
  altri) e Importa.

### B7. Test

- **Store (Rust):** lettura v1 → riscrittura v2; molti a molti; eliminare una connessione la
  toglie dai progetti; id orfani scartati; validazione di nome, colore e iniziali; file v2
  rifiutato da un lettore v1 (test sul messaggio).
- **Cronologia (Rust):** filtro su più connessioni.
- **Iniziali (TS):** casi "Cliente Rossi", "Interno", "  a  b  c ", unicode.
- **Nuovo scenario E2E `projects-sqlite`:** crea due progetti, assegna le connessioni dal
  dialog e dal menu, apre tab diverse in ognuno, cambia con il clic e con Ctrl+2, verifica
  navigator, tab ripristinate, titolo e cronologia filtrata, elimina un progetto.
- Gli scenari esistenti partono in Tutte le connessioni e passano invariati.

---

## Fuori da questo lavoro

In ordine di quando li farei:

1. Script salvati dentro il progetto, con il pannello script del gruppo D (P2).
2. Riordinare i progetti trascinando i quadrati.
3. Progetti condivisi in team come file (gruppo O).
4. Impostazioni diverse per progetto.

## Documentazione da aggiornare

- `README.md`: nuova grafica (con uno screenshot) e progetti.
- `docs/ROADMAP.md`: i progetti come voce nuova; aggiornare il gruppo D (gli script andranno
  nei progetti) e il gruppo O.
- `docs/EXTENDING.md`: `src/styles/` per area, il componente `Icon`, le nuove variabili
  della `Palette`.
