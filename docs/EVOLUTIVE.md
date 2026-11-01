# PlatypusDB — Evolutive

Idee e sviluppi per dopo la [roadmap](ROADMAP.md). Hanno la stessa logica: **gruppi indipendenti**,
ognuno con un'area di codice propria, che più persone possono portare avanti in parallelo.
Si prendono quando tutti i gruppi della roadmap sono `✅ completato`, `✅ fatto (…)` senza
altro da scegliere, `🔒`, `⏸ in attesa` o `⛔ bloccato`; la procedura è quella di
[CLAUDE.md](../CLAUDE.md), con queste differenze: il lock sta nella tabella "Lock delle evolutive"
qui sotto e il branch si chiama `evo-<lettera>-<tema>` (es. `evo-a-agenti`).

Fonti: il collaudo del gruppo S ([COVERAGE.md](COVERAGE.md), [COLLAUDO.md](COLLAUDO.md)), le
richieste dell'utente e i limiti noti nelle sezioni dei gruppi della roadmap.

**Priorità**
- **P1**: serve a molti utenti, o manca una funzione di base.
- **P2**: importante, ma si può rimandare.
- **P3**: nicchia, rifinitura, oppure da valutare.

Nella colonna **Origine**:
- **D**: presente in DBeaver Community.
- **D+**: solo nelle edizioni a pagamento di DBeaver.
- **N**: non presente in DBeaver.
- **C**: emersa dal collaudo (gruppo S), non corretta in quella sede.
- **U**: richiesta dell'utente.

Nella prima colonna delle tabelle:
- ✅: fatto.
- 🆕: aggiunto dopo la creazione del file, ancora da fare.

---

## 1. Regole

Valgono quelle di [ROADMAP.md](ROADMAP.md): "Regole per tutti i gruppi" (area propria, funzioni di
database dal `Dialect` o dalle capacità del driver, test di conformance, refactor continuo) e la
Definition of Done in fondo al file. Valgono anche le righe `⏭ serve un Mac` e `⛔ bloccato`, con
lo stesso significato: non si prendono, non contano per `✅ completato`. Le righe che dipendono
da una scelta dell'utente sono segnate `da decidere`: si chiede prima, poi si implementa.

Prima di aggiungere un tipo, un metodo del trait `Connection` o un modulo in `platypus-core` o
`platypus-sql`, controllare che non esista già (vedi [CLAUDE.md](../CLAUDE.md)). Le "Correzioni
aperte" della roadmap hanno sempre la precedenza.

---

## 2. Gruppi di lavoro

### Lock delle evolutive

Chi lavora su un gruppo lo segna qui e pusha su `main` prima di scrivere codice. Formato:
`🔒 <nome> · <branch> · <AAAA-MM-GG>`. Un lock senza commit sul branch da più di 3 giorni è
scaduto. Stesse regole di `✅ completato` e `✅ fatto (…)` della roadmap.

| Gruppo | Lock |
|---|---|
| A — Agenti e MCP | libero |
| B — Plugin e mod | libero |
| C — Kerberos e autenticazione | libero |
| D — Rifiniture dal collaudo | libero |
| E — Documentazione per utenti | libero |
| F — Pubblicazione e distribuzione | libero |

### Gruppo A — Agenti e MCP

**Stato: da fare.**

**Area:** `crates/platypus-mcp` (nuovo tool), `apps/desktop/src-tauri/src/` (ponte con l'app aperta), `apps/desktop/src/erd/` (export SVG già presente)
**Dipende da:** gruppo N della roadmap (completato)

| Funzionalità | Prio | Origine |
|---|---|---|
| 🆕 Tool MCP `get_er_diagram`: il diagramma ER di uno schema (tabelle, colonne chiave, FK). **Da decidere: formato.** Proposta: Mermaid `erDiagram` generato dal server MCP dal catalogo, quindi anche ad app chiusa; più SVG, come l'export dell'app, chiesto all'app aperta come fa `get_user_context` (`/v1/context`). Il parametro `format` sceglie tra i due | P2 | U |
| 🆕 `get_er_diagram` con elenco di tabelle e profondità di FK (le tabelle collegate a quelle scelte, fino a N salti), per non produrre un diagramma enorme su schemi grandi | P3 | U |
| 🆕 Test del tool nella conformance (Mermaid su PostgreSQL, MySQL e SQLite: tabelle, PK, FK, nomi con spazi) e scenario E2E per la richiesta all'app aperta | P2 | U |

---

### Gruppo B — Plugin e mod

**Stato: da fare.**

**Area:** `crates/platypus-plugins`, `apps/desktop/src/plugins/`, [PLUGINS.md](PLUGINS.md); punti di aggancio già esistenti in [EXTENDING.md](EXTENDING.md): registro delle azioni, `registerTabKind`, `GridSourceExtensions`, sezioni delle Impostazioni, tema
**Dipende da:** plugin WASM del gruppo G della roadmap (azioni e driver, ABI 1)

Oggi un plugin aggiunge azioni e un driver (HTTP). Qui si estende l'ABI perché i componenti
aggiuntivi si attacchino all'app senza toccarne il sorgente. Ogni capacità nuova è un permesso
da approvare all'installazione, come `http` e `clipboard`; le novità non rompono l'ABI 1 (`api = 2`
con la compatibilità all'indietro).

| Funzionalità | Prio | Origine |
|---|---|---|
| 🆕 **Pannelli e tipi di tab dei plugin**: un plugin dichiara un tab (`kind`) o un pannello laterale; il contenuto è un documento HTML in un iframe isolato che parla con l'app per messaggi, registrato in `registerTabKind` | P2 | U |
| 🆕 **Temi come plugin**: un pacchetto con soli token di colore e font (`kinds = ["theme"]`), senza modulo WASM, che compare nel selettore dei temi | P2 | U |
| 🆕 **Hook sugli eventi**: prima e dopo una query, alla connessione, al salvataggio delle modifiche; un hook "prima" può bloccare con un messaggio, e passa dai controlli di Sicurezza (gruppo L) | P2 | U |
| 🆕 **Estensioni della griglia**: un plugin aggiunge colonne calcolate, renderer di cella o azioni di cella tramite `GridSourceExtensions` | P3 | U |
| 🆕 **Pagina delle impostazioni di un plugin**: schema dei campi nel manifest, disegnato dall'app nelle Impostazioni e salvato nello store, letto dal modulo | P2 | U |
| 🆕 **Catalogo e installatore**: elenco di plugin da un indice (JSON) con scarico, controllo dell'hash SHA-256 e della firma del pacchetto, aggiornamento e rimozione | P2 | U |
| 🆕 Documentazione e plugin di esempio per ogni punto nuovo in `examples/plugins`, con test del permesso rifiutato | P2 | U |

---

### Gruppo C — Kerberos e autenticazione

**Stato: da fare.** La riga Kerberos del gruppo F della roadmap resta com'è (`⛔ bloccato`); qui
c'è il piano per superarla.

**Area:** `vendor/tokio-postgres`, `crates/platypus-driver-pg` (e `platypus-core` per `Auth`), `docker/compose.yml` (KDC di prova), `crates/platypus-driver-mysql` per la riga bloccata
**Dipende da:** gruppo F della roadmap (completato)

| Funzionalità | Prio | Origine |
|---|---|---|
| 🆕 **Kerberos/GSSAPI per PostgreSQL**: nella copia di `tokio-postgres` già mantenuta in `vendor/`, gestire i messaggi GSS dell'autenticazione con `cross-krb5` (SSPI su Windows, GSSAPI altrove); nuovo valore di `Auth` (`kerberos`) e campo per il nome del servizio. Funzione opzionale (feature di Cargo), così chi non la vuole non dipende dalle librerie Kerberos | P2 | D+ |
| 🆕 Prova con un KDC in Docker (servizio `kdc` e PostgreSQL con `gss` in `pg_hba.conf`, profilo di `docker/compose.yml`), nella conformance; saltata senza ticket né KDC | P2 | D+ |
| 🆕 Biglietto da un `kinit` esterno o da un keytab (`KRB5CCNAME`, `KRB5_CLIENT_KTNAME`) e messaggio chiaro quando manca; `sign_in` anche per `psql` | P3 | D+ |
| ⛔ bloccato — **Kerberos per MySQL e MariaDB**, `auth_gssapi` di MariaDB per primo (il plugin Kerberos di MySQL è solo nella versione Enterprise). `mysql_async` non ha un aggancio per i plugin di accesso: serve prima quello (patch o copia in `vendor/`) | P3 | D+ |

---

### Gruppo D — Rifiniture dal collaudo

**Stato: da fare.** Miglioramenti piccoli trovati dai tester del gruppo S e non corretti; l'elenco
dei limiti noti è in [COVERAGE.md](COVERAGE.md). Ogni riga ha un test (unitario, conformance o
scenario E2E) che prima falliva.

**Area:** varia per riga (indicata); `platypus-local` per SQL sui risultati, `apps/desktop/src/components/Navigator.tsx`, driver interessati
**Dipende da:** nulla

| Funzionalità | Prio | Origine |
|---|---|---|
| 🆕 Numeri oltre 38 cifre senza perdita in "Query with SQL" e nelle query tra database: ripiego su `VARCHAR` invece di `DOUBLE` (`platypus-local`, `duck.rs`) | P2 | C |
| 🆕 Posizione e dimensione della finestra ripristinate al riavvio (anche su più monitor, con ripiego se il monitor non c'è più) | P2 | C |
| 🆕 SQL Server `time(7)`: il settimo decimale in DuckDB non si perde | P3 | C |
| 🆕 CockroachDB: tabelle senza chiave primaria modificabili con il `rowid` nascosto | P3 | C |
| 🆕 DuckDB: una riga modificata resta al suo posto dopo il salvataggio | P3 | C |
| 🆕 Notebook: elenchi Markdown annidati | P3 | C |
| 🆕 SQL Server: un vincolo disabilitato senza nome, aggiunto a una tabella esistente, dà un avviso o riceve un nome | P3 | C |
| 🆕 MariaDB: tabelle con colonne di periodo esplicite nel confronto degli schemi | P3 | C |
| 🆕 Profilo di una colonna: un conteggio diverso da zero non appare come "1 (0%)" | P3 | C |
| 🆕 Grafico a torta: legenda leggibile con molte categorie (raggruppa le piccole in "Altro", o scorre) | P3 | C |
| 🆕 F2 mentre la tabella sta ancora caricando le colonne: un messaggio invece del silenzio | P3 | C |
| 🆕 MySQL: le variabili `@var` non restano tra una chiamata di un agente e l'altra sulla stessa sessione (o il limite è scritto in AGENTS.md) | P3 | C |
| 🆕 Navigator: le righe degli indici hanno un menu contestuale | P3 | C |
| 🆕 Navigator: digitando il nome si salta alla riga dell'albero (type-ahead) | P3 | C |
| 🆕 SQLite: le tabelle ombra di FTS5 non compaiono come tabelle normali | P3 | C |
| 🆕 Pagine di dettaglio di sequenze, tipi e domini: mostrano le proprietà del catalogo (valore corrente, incremento, tipo base, vincoli…) | P3 | C |
| 🆕 Filtro del navigator: `#nome` non è letto come tag, così le tabelle temporanee di SQL Server (`#temp`) si cercano | P3 | C |
| 🆕 "Del tema": i campioni di colore non restano grigi | P3 | C |
| 🆕 Tab fissati: icona diversa da quella dei tab normali | P3 | C |

---

### Gruppo E — Documentazione per utenti

**Stato: da fare.**

**Area:** `docs/`, `README.md` e `README.it.md`, `site/`
**Dipende da:** nulla

| Funzionalità | Prio | Origine |
|---|---|---|
| 🆕 Separare la documentazione per utenti da quella per sviluppatori: `docs/user/` (uso, funzionalità, scorciatoie, agenti) e `docs/dev/` (sviluppo, estensione, rilascio, benchmark, roadmap), con [docs/README.md](README.md) aggiornato e nessun link rotto | P2 | U |
| 🆕 **Guida per l'utente** vera e propria: primo avvio, collegarsi a un database, query e risultati, modifica dei dati, esportare e importare, sicurezza (Prod, sola lettura), agenti; con le schermate e i passi, non l'elenco delle funzioni di FEATURES.md | P2 | U |
| 🆕 README più corto, in inglese e in italiano: cosa è, perché, installazione, link alla guida; il dettaglio resta in `docs/` | P2 | U |
| 🆕 Sito di presentazione in `site/` allineato alla guida (testi, GIF, confronto con i numeri di BENCHMARKS.md). **La pubblicazione è una scelta dell'utente: da decidere** (vedi gruppo F) | P3 | U |
| 🆕 Controllo automatico dei link relativi tra i documenti (`scripts/`), da lanciare a mano | P3 | N |

---

### Gruppo F — Pubblicazione e distribuzione

**Stato: da fare.** Solo ciò che non è già nel gruppo P della roadmap, dove restano firma Windows,
verifica su Linux con GPU, benchmark con DBeaver, lancio e vetrina.

**Area:** `packaging/`, `scripts/packaging.mjs`, `.github/`, `site/`, [RELEASING.md](RELEASING.md)
**Dipende da:** nulla; ma le prime due righe sono decisioni del proprietario

| Funzionalità | Prio | Origine |
|---|---|---|
| 🆕 **Rendere pubblico il repository.** **Da decidere (proprietario).** Sblocca l'auto-update (oggi `latest.json` dà 404 perché il repository è privato: vedi la riga bloccata del gruppo P), i gestori di pacchetti e i minuti illimitati di Actions. Prima: controllo che nessun segreto o dato personale sia nella storia, licenze in regola, `SECURITY.md` e `CONTRIBUTING.md` aggiornati | P1 | U |
| 🆕 **Invio ai gestori di pacchetti**, con i manifest già pronti in `packaging/`: winget, Homebrew, AUR, Flathub; per ognuno i passi in RELEASING.md e un controllo che SHA-256 e versione coincidano con la release | P2 | N |
| 🆕 **GitHub Pages** per il sito (`site/`), con un workflow lanciato a mano; dominio da decidere | P3 | U |
| 🆕 Dopo la pubblicazione: la riga "Test reale dell'auto-update" del gruppo P si può fare | P1 | U |

---

## 3. Ondate consigliate

Con 1–3 persone. Ogni gruppo può partire quando la sua dipendenza è pronta.

| Ondata | Gruppi | Obiettivo | Stato |
|---|---|---|---|
| **1** | **D**, **A** | Chiudere i limiti noti del collaudo e dare agli agenti il diagramma ER | Da fare |
| **2** | **B**, **C** | Estendere l'app dall'esterno (plugin) e coprire le reti con Kerberos | Da fare |
| **3** | **E**, **F** | Documentazione per utenti e distribuzione, quando l'utente decide di pubblicare | In attesa della decisione |

Per ogni gruppo vale la Definition of Done della roadmap.
