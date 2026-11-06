# PlatypusDB — regole di lavoro

Valgono per ogni sessione, umana o di un agente. Le regole di area e di qualità dei gruppi sono in
[docs/dev/ROADMAP.md](docs/dev/ROADMAP.md) ("Regole per tutti i gruppi" e "Definition of Done"): vanno
rispettate anche quelle.

## Requisiti

- Rust stable con `rustfmt` e `clippy`, Node + npm, Docker (test dei driver).
- [uv](https://docs.astral.sh/uv/) e **graphify** per il grafo di conoscenza del codice:
  ```bash
  uv tool install graphifyy
  graphify install --platform windows   # su macOS/Linux: graphify install
  graphify hook install                 # una volta per clone: aggiorna il grafo a ogni commit/checkout
  ```

## Prima di iniziare

1. **Aggiornare la codebase.** `git fetch origin`, poi `git pull` sul branch corrente. Su un branch
   di gruppo, unire anche `origin/main` (`git merge origin/main`) e risolvere i conflitti prima di
   scrivere codice.
2. **Aggiornare graphify** se non è stato fatto negli ultimi 7 giorni: `uv tool upgrade graphifyy`;
   se la versione è cambiata, rilanciare `graphify install --platform windows` per aggiornare la skill.
3. **Controllare la roadmap.** Se la sezione "Correzioni aperte" di [docs/dev/ROADMAP.md](docs/dev/ROADMAP.md)
   ha delle righe, correggerle prima di tutto il resto. Poi, nella tabella "Lock dei gruppi" di [docs/dev/ROADMAP.md](docs/dev/ROADMAP.md)
   scegliere un gruppo disponibile, con le dipendenze pronte, seguendo le "Ondate consigliate".
   **Disponibile** vuol dire `libero` **oppure** `✅ fatto (…)`: un gruppo fatto in parte si prende
   per le funzionalità che restano, alla pari di uno libero. Non si prendono solo i gruppi
   `✅ completato`, quelli con un lock valido (`🔒`), quelli `⏸ in attesa` e quelli `⛔ bloccato`.
   Tra i disponibili si sceglie quello che ha più senso: prima l'ondata più bassa, poi le
   priorità più alte ancora da fare (P1, poi P2), poi quello con più funzionalità aperte.
   Un lock è scaduto se il branch indicato non ha commit da più di **3 giorni**
   (`git log -1 --format=%cd origin/<branch>`): in quel caso il gruppo si considera libero.
   Quando ogni gruppo della roadmap è completato, fatto, bloccato o in attesa, si sceglie da
   [docs/dev/EVOLUTIONS.md](docs/dev/EVOLUTIONS.md) con le stesse regole (lock nella sua tabella, branch
   `evo-<lettera>-<tema>`).
4. **Prendere il lock e pusharlo su main**, prima di scrivere codice:
   - su `main` aggiornato, modificare solo la riga del gruppo in "Lock dei gruppi":
     `🔒 <nome> · <branch> · <AAAA-MM-GG>`;
   - commit con solo quella modifica: `chore(roadmap): lock gruppo <X>`, poi `git push origin main`;
   - se il push viene rifiutato: `git pull --rebase`, ricontrollare la riga. Se nel frattempo
     l'ha presa qualcun altro, scegliere un altro gruppo. Mai `--force` su main.
5. Creare il branch del gruppo da `main`: `group-<lettera>-<tema>` (es. `group-d-editor`).
   Pushare il branch almeno una volta al giorno, altrimenti il lock risulta scaduto.
   Se il gruppo dipende da un altro gruppo non ancora unito in `main` (o tocca gli stessi file),
   crearlo dal branch di quel gruppo e unirlo in `main` solo dopo quello.
6. **Scegliere i modelli prima di implementare.** La sessione principale (Opus) decide, per sé e
   per ogni subagente, il modello adatto al compito e lo passa esplicitamente (parametro `model`
   del tool Agent), per non sprecare token:
   - **Haiku**: ricerche, lettura e riassunto di file, lancio di test o build con resoconto,
     aggiornamenti meccanici di documentazione.
   - **Sonnet**: implementazione che segue uno schema già presente (un driver come un altro, una
     schermata come un'altra), test, scenari E2E, refactor.
   - **Opus**: progettazione, modifiche trasversali a `platypus-core`/`platypus-sql`, debugging
     difficile, code review.
   Un compito grande si divide: Opus progetta e rivede, i subagenti Sonnet/Haiku eseguono.
   Mai un subagente Opus per lavoro meccanico.

## Durante il lavoro

- Per capire il codice usare prima il grafo: `graphify query "<domanda>"`, `graphify explain "X"`,
  `graphify affected "X"` (legge `graphify-out/graph.json`). Se il grafo non esiste: `/graphify .`.
- Restare nell'**area** del gruppo. Una modifica a un file condiviso (vedi "Mappa dei conflitti")
  va in un commit piccolo e separato.
- **Prima di aggiungere un tipo, un metodo del trait `Connection` o un modulo in `platypus-core`
  o `platypus-sql`**, controllare che non esista già su `main` o sui branch degli altri gruppi
  (`git fetch`, poi `git log origin/main` e `git grep <nome> origin/<branch>`). Due gruppi in
  parallelo hanno già scritto la stessa cosa due volte (`ReferencingKey` di B e `ReferenceMeta`
  di C, unificati al merge).
- Logica nuova = test nuovo. Tutto ciò che tocca i driver va anche nella conformance suite.
- Commit in inglese, stile Conventional Commits (`feat:`, `fix:`, `refactor:`, `chore:`).
- Aggiungere al commit solo i propri file (`git add <file>`, non `git add -A`): nel working tree
  possono esserci modifiche non committate di altri.
- clippy è in modalità `pedantic` con `-D warnings`. I lint che scattano più spesso:
  `too_many_lines` (oltre 100 righe: dividere la funzione), `struct_excessive_bools` (più di 3
  `bool`: ricavarne uno o usare un enum), `format_push_string` (usare `write!`),
  `missing_panics_doc` (niente `expect`/`unwrap` nelle funzioni pubbliche).

### Test end-to-end

Gli scenari in `apps/desktop/e2e/scenarios/` guidano l'app compilata. Sul runner Windows della CI
sono molto più lenti che in locale: un test che passa in locale può fallire lì. Regole:
- **Mai aspettare una condizione che è già vera prima dell'azione.** Uno stato come "3 rows"
  può essere quello vecchio: svuotare `.statusbar span` prima dell'azione, oppure aspettare un
  segnale che cambia (es. il pulsante Cancel disabilitato a fine esecuzione).
- La fine di uno script non è la comparsa delle schede di risultato: compaiono quando parte.
  Finché gira, l'editor ignora Ctrl+Enter.
- Dopo `page.type(...)` nell'editor, aspettare che `.cm-line` contenga il testo prima di eseguire.
- CodeMirror ignora i tasti per 75 ms dopo l'apertura della lista di completamento.
- Se uno scenario fallisce solo in CI, il log riporta lo stato della pagina (`page: {...}`) e il
  run conserva gli screenshot (artifact `e2e-shots`): `gh run download <id> -n e2e-shots`.

## Dopo un'implementazione

1. **Refactor con un agente dedicato.** Lanciare un subagente che lavora su `git diff origin/main...HEAD`:
   rimuove duplicazioni, semplifica, rinomina, elimina lavoro superfluo. Non cambia il
   comportamento, resta nei file già toccati, e il risultato va in un commit `refactor:` separato.
2. **Verifica**, tutto verde prima di dire "fatto":
   ```bash
   cargo fmt --all --check
   npm --prefix apps/desktop run build    # prima di clippy: il crate desktop richiede il frontend compilato
   cargo clippy --workspace --all-targets
   cargo test --workspace --exclude platypus-desktop
   ```
   Se si tocca la UI: `npm --prefix apps/desktop run tauri build -- --no-bundle`, poi gli E2E
   (gli scenari guidano l'eseguibile di release, non il dev server: senza ricompilare si testa la
   versione vecchia). **Non si lanciano tutti gli E2E** a fine sviluppo: solo gli scenari del
   gruppo e quelli delle aree toccate (`npm --prefix apps/desktop run e2e -- <filtro>`).
   Gli scenari del gruppo devono essere corposi e provare tutte le sue funzionalità, compresi
   i casi limite e gli errori, non un solo percorso felice. Se si toccano i driver:
   `docker compose -f docker/compose.yml up -d --wait`, poi `cargo test -p platypus-conformance`.
   La conformance salta senza errori i database che non raggiunge (stampa `skipping ...` con
   `-- --nocapture`): controllare che li abbia provati tutti. Dopo una modifica a
   `docker/compose.yml`, `up -d` ricrea i container interessati.
3. **Code review** con `/code-review` sul branch e correzione dei problemi trovati.
4. **Aggiornare la documentazione**: la riga "Stato" del gruppo in ROADMAP.md, EXTENDING.md se
   nasce un nuovo punto di estensione, docs/user/FEATURES.md per le funzionalità utente (e il README,
   in inglese e in italiano, solo se cambia il quadro generale).
5. **Unire in `main`** (niente pull request):
   ```bash
   git fetch origin
   git merge origin/main          # sul branch del gruppo: conflitti risolti qui, poi riverificare
   git checkout main && git pull
   git merge --no-ff <branch>
   git push origin main
   ```
   Si unisce solo con la verifica del punto 2 tutta verde, rifatta dopo il merge di `origin/main`.
   Se il push viene rifiutato perché `main` è avanzato, ripetere da `git fetch`. Mai `--force`.
6. **Niente CI automatica** (vedi "GitHub Actions"): la verifica in locale del punto 2 è l'unico
   controllo. Prima di unire in `main` si fanno sempre tutti i passi, anche se il gruppo non tocca
   UI o driver: build di release, gli E2E del gruppo e delle aree toccate, e conformance con i
   container Docker avviati.

## GitHub Actions

Durante lo sviluppo la CI su GitHub non gira: i minuti di Actions dell'account (3.000 al mese)
sono finiti in meno di un giorno, soprattutto per i push della roadmap e per macOS (contato ×10).
- `ci.yml` parte solo a mano (`workflow_dispatch`), e solo quando lo chiede l'utente.
- La release (`release.yml`, tag `v*` o avvio manuale) solo quando lo chiede l'utente.
- Un run superato da un altro si annulla (`gh run cancel <id>`); un run fallito non si rilancia
  senza una correzione.
- Fino al 2 novembre 2026 i minuti sono esauriti: nessun run, neanche a mano.

## Chiusura del lock

- Quando il branch del gruppo è unito in `main` (o il lavoro previsto è finito), aggiornare la riga
  con un commit `chore(roadmap): rilascio gruppo <X>`:
  - `✅ completato` solo se **tutte** le funzionalità P1, P2 e P3 della tabella del gruppo sono
    fatte, tranne le righe `⏭ serve un Mac` e `⛔ bloccato`, che si saltano finché non c'è un Mac o finché non cade il blocco. Anche la riga "Stato" della sezione del gruppo diventa `**Stato: completato**`. Un gruppo
    completato non si riprende: è il segno che lì non c'è più niente da fare.
  - `✅ fatto (<priorità completate>)` se resta qualcosa (es. `✅ fatto (P1, P2)`, o
    `✅ fatto (P1; P2 in parte)`): il gruppo resta disponibile per il resto, come un gruppo libero.
  - `libero` se non è stato completato nessun livello di priorità.
- Se si abbandona il lavoro, rilasciare subito il lock, senza aspettare i 3 giorni.

## graphify

This project has a knowledge graph at graphify-out/ with god nodes, community structure, and cross-file relationships.

Rules:
- When graphify-out/graph.json exists, run `graphify query "<question>"` before your first grep, search or source-file read for a codebase question, and put this rule in every subagent prompt that explores code. Read raw files only for lines the graph or an earlier read already located. Use `graphify path "<A>" "<B>"` for relationships and `graphify explain "<concept>"` for focused concepts. These return a scoped subgraph, usually much smaller than GRAPH_REPORT.md or raw grep output.
- If graphify-out/wiki/index.md exists, use it for broad navigation instead of raw source browsing.
- Read graphify-out/GRAPH_REPORT.md only for broad architecture review or when query/path/explain do not surface enough context.
- After modifying code, run `graphify update .` to keep the graph current (AST-only, no API cost).
- A committed graphify-out/graph.json is rewritten whole on every build, so a large diff to it in a pull request is expected. Do not flag it as churn or suggest skipping the graph rebuild or its hooks.

## Grafo di conoscenza: note del progetto

- `graphify-out/` è locale e ignorato da git: ogni clone ha il suo grafo. Gli hook git (`graphify hook install`)
  lo aggiornano a commit, checkout, merge e rebase, ma solo per i file di codice (estrazione AST, senza LLM).
- Dopo modifiche a documentazione o immagini, o se il grafo sembra vecchio: `/graphify . --update`.
- graphify **non si aggiorna da solo**: vale il punto 2 di "Prima di iniziare".
- Non rinominare la sezione `## graphify` qui sopra: la riscrive `graphify claude install`.
