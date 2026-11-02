# PlatypusDB — Collaudo a mano

Liste per ciò che gli E2E (CDP sull'eseguibile di release) non possono guidare: installer, sistema
operativo, finestre vere. Fanno parte del gruppo S ([ROADMAP.md](ROADMAP.md)); i buchi sono in
[COVERAGE.md](COVERAGE.md) (righe 1 e 12). Ogni passo ha un risultato atteso e una casella da
spuntare; a fianco si scrivono versione, data, sistema e chi ha provato.

Percorsi usati (dal codice):

| Cosa | Windows | Linux |
|---|---|---|
| Identificatore | `dev.platypusdb.desktop` | uguale |
| Connessioni e impostazioni: `connections.toml`, `settings.toml`, `secrets.vault` | `%APPDATA%\dev.platypusdb.desktop\` | `~/.config/dev.platypusdb.desktop/` |
| Dati: `history.db`, `catalog.db`, `schema-history.db`, `scripts\`, `dashboards\`, `schedules.json`, `transfer-profiles.json`, `saved-results\` | `%APPDATA%\dev.platypusdb.desktop\` | `~/.local/share/dev.platypusdb.desktop/` |
| Log (`platypus.log`, poi `platypus.1.log`...) | `<dati>\logs\` | `<dati>/logs/` |
| Web view (schede aperte, tema) | `%LOCALAPPDATA%\dev.platypusdb.desktop\` | `~/.local/share/dev.platypusdb.desktop/` |
| Password | Gestione credenziali di Windows, servizio `platypusdb`, utente = id della connessione | Secret Service (libsecret), stesso servizio |
| Copia portatile | cartella `data` accanto a `platypus-db.exe`: se esiste, sostituisce tutto | accanto all'eseguibile |
| Cartella scelta a mano | variabile `PLATYPUS_DATA_DIR` (vince su tutto) | uguale |

Installazione di Windows: NSIS per l'utente corrente (senza amministratore, `installMode:
currentUser`) e MSI. Gli aggiornamenti arrivano da
`github.com/PaoloCisly/platypus-db/releases/latest/download/latest.json` (installazione passiva).

Lo script [scripts/collaudo-profile.ps1](../scripts/collaudo-profile.ps1) copia, confronta e
ripristina le due cartelle (non le password, che restano nella Gestione credenziali). Solo
`-Restore` sostituisce dati, e prima salva lo stato attuale in una copia a parte. Esempi:

```powershell
.\scripts\collaudo-profile.ps1                      # elenca file, password e copie
.\scripts\collaudo-profile.ps1 -Backup -Name v021   # copia con nome
.\scripts\collaudo-profile.ps1 -Compare -Name v021  # che cosa è cambiato rispetto alla copia
.\scripts\collaudo-profile.ps1 -Restore -Name v021  # ripristina (salva prima lo stato attuale)
```

---

## 1. Installer di Windows e aggiornamento

Servono: Windows 11 pulito (VM o secondo utente), gli installer 0.2.1 dalle release di GitHub e
quelli da provare (`.exe` NSIS e `.msi`), un database a cui connettersi.

**Cosa copre `s-upgrade-*`.** Gli scenari `apps/desktop/e2e/scenarios/s-upgrade-<tag>.mjs` (e
`-master`, con il master password) scaricano l'MSI di ogni release pubblicata (`gh release
download`, in `target/e2e-releases/<tag>/`), ne estraggono l'eseguibile con `msiexec /a`
(installazione amministrativa: copia i file, non registra nulla) e con quello scrivono su un
profilo nuovo i dati di un utente (due connessioni con password nel keychain, cartella, progetto,
tre script, tema e lingua, cronologia, schede aperte, ...); poi aprono la build in prova sullo
stesso profilo. Quindi **i dati e le password attraverso le versioni** (1.3 punti 2-4 e 6, e il
"sopra se stessa" del punto 8) sono automatici. Restano a mano, su una VM pulita, la **meccanica
degli installer** (NSIS/MSI sopra una versione installata, con l'app aperta, disinstallazione,
voci di Windows, download dell'aggiornamento automatico): eseguire un installer nei test
toccherebbe l'installazione dell'utente.

### 1.1 Installazione nuova, NSIS

1. Installare il `.exe` NSIS con doppio clic. Atteso: nessuna richiesta di amministratore; voce nel menu Start. - [ ]
2. Avviare. Atteso: schermata iniziale, nessun errore; `%APPDATA%\dev.platypusdb.desktop\logs\platypus.log` ha la riga di avvio con la versione. - [ ]
3. Creare una connessione con password, uscire, riaprire. Atteso: si connette senza chiedere la password; `cmdkey /list` mostra una voce `platypusdb`. - [ ]
4. Impostazioni → "Cerca aggiornamenti". Atteso: versione corretta, nessun errore. - [ ]
5. Impostazioni di Windows → App installate. Atteso: editore `PlatypusDB`, versione giusta. - [ ]

### 1.2 Installazione nuova, MSI

1. Installare il `.msi`. Atteso: procedura guidata e collegamento nel menu Start. - [ ]
2. Ripetere i passi 2-5 di 1.1. Atteso: stessi risultati. - [ ]
3. `msiexec /i PlatypusDB.msi /qn` da prompt. Atteso: installa senza finestre. - [ ]

### 1.3 Installazione sopra la versione pubblicata (0.2.1)

Preparazione: disinstallare tutto; installare 0.2.1; con essa creare due connessioni con password
(una PostgreSQL, una SQLite), una cartella e un progetto, tre script salvati, tema e lingua diversi
dal predefinito, una query eseguita (cronologia), una query salvata. Chiudere l'app, poi
`.\scripts\collaudo-profile.ps1 -Backup -Name v021`. Per il punto 6 rifare lo stesso con il master
password attivo (Impostazioni → Sicurezza) su un secondo profilo (`-Backup -Name v021-vault`).

I dati dei punti 2, 3, 4 e 6 sono provati anche da `s-upgrade-*` (vedi sopra): qui si guarda solo
che l'installer li lasci dove sono.

1. Installare la versione nuova (NSIS) sopra, senza disinstallare. Atteso: nessuna domanda sulla cartella, nessun errore; `platypus-mcp.exe` non c'è più nella cartella di installazione. - [ ]
2. `.\scripts\collaudo-profile.ps1 -Compare -Name v021`. Atteso: nessun file con `<` (nessuno sparito); i `=` solo per file che l'app migra (`settings.toml`, `history.db`). - [ ]
3. Avviare. Atteso: connessioni, cartella, progetto, script, cronologia, query salvate, tema e lingua come prima; le schede aperte tornano. - [ ]
4. Connettersi a entrambe. Atteso: nessuna richiesta di password (keychain). - [ ]
5. Rifare tutto con l'MSI (disinstallare, `-Restore -Name v021`, installare 0.2.1, chiudere, installare l'MSI nuovo). Atteso: stessi risultati. - [ ]
6. Profilo con master password (`-Restore -Name v021-vault`): aggiornare e avviare. Atteso: l'app chiede il master password, lo accetta, le connessioni si aprono. - [ ]
7. Installare sopra con l'app aperta. Atteso: l'installer chiede di chiuderla o la chiude; dopo l'avvio i dati ci sono. - [ ]
8. Installare di nuovo la stessa versione sopra. Atteso: riuscita, nulla perso. - [ ]
9. Se un agente MCP era registrato con 0.2.0 (puntava a `platypus-mcp.exe`): avviarlo. Atteso: l'app lo segnala e la registrazione si ripara verso `platypus-db.exe --mcp`; il client funziona. - [ ]

### 1.4 Aggiornamento automatico dalla versione pubblicata

Serve una versione più nuova pubblicata (release di prova con `latest.json`).

1. Installare 0.2.1, creare i dati di 1.3, `-Backup -Name v021`. - [ ]
2. Avviare. Atteso: compare l'avviso di una nuova versione ("Aggiorna a {version}"). - [ ]
3. Impostazioni → "Cerca aggiornamenti". Atteso: stesso avviso; senza rete un errore chiaro ("Aggiornamento non riuscito: ..."), non un blocco. - [ ]
4. Premere "Aggiorna". Atteso: scarica con avanzamento; l'installer parte in modo passivo (solo una barra) e l'app si riavvia. - [ ]
5. Dopo il riavvio. Atteso: versione nuova, dati come in 1.3 punto 3; `-Compare` come in 1.3 punto 2. - [ ]
6. Staccare la rete a metà download. Atteso: errore; l'app resta usabile e la versione vecchia intatta. - [ ]
7. Con una query in corso o una transazione aperta, premere "Aggiorna". Atteso: avviso prima di chiudere, o la transazione annullata come alla chiusura normale. - [ ]
8. Release di prova con firma `.sig` alterata. Atteso: aggiornamento rifiutato. - [ ]

### 1.5 Disinstallazione

1. Disinstallare l'NSIS da App installate. Atteso: la cartella di installazione sparisce (nessun `platypus-mcp.exe` residuo). - [ ]
2. Guardare `%APPDATA%\dev.platypusdb.desktop`. Atteso: resta tutto (connessioni, script, cronologia, log). - [ ]
3. `cmdkey /list`. Atteso: le voci `platypusdb` restano. - [ ]
4. Reinstallare. Atteso: dati e password di prima si ritrovano. - [ ]
5. Solo su macchina di prova: cancellare a mano la cartella dei dati e le voci `platypusdb`, avviare. Atteso: primo avvio pulito. - [ ]
6. Disinstallare l'MSI. Atteso: stessi risultati dei punti 1-3. - [ ]
7. Disinstallare con l'app aperta. Atteso: l'installer chiede di chiuderla. - [ ]

### 1.6 Copia portatile (`data`)

1. Copiare l'eseguibile in `D:\Portatile\`, creare accanto una cartella vuota `data`, avviare. Atteso: `data\` si riempie (`connections.toml`, `settings.toml`, `logs\`); nulla di nuovo in `%APPDATA%\dev.platypusdb.desktop`. - [ ]
2. Creare una connessione e uno script, chiudere, riaprire. Atteso: ci sono; la password funziona. - [ ]
3. Spostare tutta la cartella su un altro percorso e avviare. Atteso: i dati vanno con la cartella. - [ ]
4. Sostituire solo l'eseguibile con la versione nuova. Atteso: nessuna perdita. - [ ]
5. Rinominare `data` in `data-off`. Atteso: l'app torna a usare `%APPDATA%\dev.platypusdb.desktop`. - [ ]
6. Impostare `PLATYPUS_DATA_DIR` con `data` presente. Atteso: vince la variabile. - [ ]
7. Cartella in sola lettura. Atteso: errore leggibile, non un crash. - [ ]

---

## 2. Integrazione col sistema (Windows)

Nel codice ci sono: icona nel vassoio con menu e finestra "Query rapida" (`windows.rs`; impostazione
"Mostra un'icona nell'area di notifica"); notifica di sistema a fine esecuzione oltre N secondi con
la finestra in secondo piano (`tauri-plugin-notification`; impostazione "Notifica quando
un'esecuzione finisce dopo (secondi)", 0 la disattiva); associazione dei file `.platypus` e schema
`platypus://` (`tauri-plugin-deep-link`); istanza unica (un secondo avvio passa il file
all'istanza aperta); più finestre ("Apri in una nuova finestra"); tema che segue il sistema.
Non c'è l'associazione di `.sql` né altro.

### 2.1 Vassoio e Query rapida

1. Attivare "Mostra un'icona nell'area di notifica". Atteso: icona subito; se non si può, "Impossibile mostrare l'icona nell'area di notifica: ...". - [ ]
2. Clic sull'icona. Atteso: piccola finestra "Query rapida" sopra le altre, con l'editor a fuoco. - [ ]
3. Scrivere `SELECT 1`, Ctrl+Invio. Atteso: risultato nella stessa finestra. - [ ]
4. Menu dell'icona: "Query rapida…", "Mostra PlatypusDB", "Esci". Atteso: ognuna fa ciò che dice; "Esci" chiude tutto e le connessioni. - [ ]
5. Chiudere la finestra principale con l'icona attiva. Atteso: annotare se l'app esce o resta nel vassoio. - [ ]
6. Disattivare l'impostazione. Atteso: l'icona sparisce subito. - [ ]
7. Cambiare lingua con l'icona attiva. Atteso: le voci del menu cambiano. - [ ]
8. Riavviare con l'impostazione attiva. Atteso: l'icona ricompare. - [ ]

### 2.2 Notifica di una query lunga

1. Soglia 5 secondi. Eseguire `SELECT pg_sleep(8)` e portare un'altra finestra in primo piano. Atteso: a fine query, notifica di Windows; il clic porta l'app in primo piano. - [ ]
2. Stessa query con PlatypusDB in primo piano. Atteso: nessuna notifica. - [ ]
3. Query più corta della soglia, finestra in secondo piano. Atteso: nessuna notifica. - [ ]
4. Soglia 0. Atteso: mai notifiche. - [ ]
5. Query che finisce in errore dopo la soglia. Atteso: la notifica dice che è fallita. - [ ]
6. Assistente notifiche ("Non disturbare") attivo. Atteso: nessun crash; la notifica va nel centro notifiche o non arriva. - [ ]
7. Notifiche bloccate per l'app nelle impostazioni di Windows. Atteso: l'app non si blocca. - [ ]

### 2.3 File, trascinamento e collegamenti

1. Doppio clic su un file `.platypus` con l'app chiusa. Atteso: l'app parte e propone la query condivisa. - [ ]
2. Stessa cosa con l'app aperta. Atteso: nessuna seconda istanza; la finestra esistente va in primo piano e propone la query. - [ ]
3. Da prompt: `start platypus://...` con un collegamento valido. Atteso: come sopra. - [ ]
4. Trascinare un file `.sql` sulla finestra. Atteso: si apre in un editor. - [ ]
5. Trascinare un `.csv`, un `.parquet`, un `.xlsx`, un file SQLite. Atteso: l'app propone importazione o apertura come database. - [ ]
6. Trascinare un file non supportato. Atteso: messaggio, nessun crash. - [ ]
7. Trascinare più file insieme. Atteso: tutti gestiti, o un messaggio chiaro. - [ ]
8. Dopo la disinstallazione: doppio clic su un `.platypus`. Atteso: Windows non lo associa più a PlatypusDB. - [ ]
9. Dopo l'installazione. Atteso: `.sql` resta associato all'editor di prima. - [ ]

### 2.4 Più finestre, DPI, monitor, tema

1. "Apri in una nuova finestra" su una scheda. Atteso: finestra nuova con quella scheda; "Sposta nella finestra principale" la riporta. - [ ]
2. Chiudere una finestra con una transazione aperta. Atteso: "Chiudere questa finestra?" con l'avviso. - [ ]
3. Chiudere e riaprire, anche massimizzata. Atteso: stessa dimensione e posizione. - [ ]
4. Scala di Windows al 100%, 150%, 200%. Atteso: testo nitido, niente tagliato; barra del titolo, griglia ed editor proporzionati; Ctrl+= e Ctrl+- (zoom dell'app) funzionano. - [ ]
5. Due monitor con scale diverse: trascinare la finestra dall'uno all'altro. Atteso: si ridisegna alla scala nuova, senza sfocature durature. - [ ]
6. Chiudere con la finestra sul secondo monitor, staccarlo, riaprire. Atteso: la finestra è visibile sul monitor che c'è. - [ ]
7. Query rapida con due monitor. Atteso: compare sul monitor giusto (annotare quale). - [ ]
8. Tema "segue il sistema": passare Windows da chiaro a scuro con l'app aperta. Atteso: l'app cambia subito, barra del titolo compresa. - [ ]
9. Tema fisso chiaro con Windows scuro. Atteso: resta chiaro. - [ ]
10. Contrasto elevato di Windows. Atteso: testo leggibile, nessuna area vuota. - [ ]
11. Sospensione e ripresa del PC con connessioni aperte. Atteso: al ritorno si ricollegano o dicono di farlo. - [ ]

---

## 3. Linux

Prova su GNOME (Wayland) e su KDE o X11. Cambia rispetto a Windows: pacchetti, dati sotto `~/.config`
e `~/.local/share`, password nel Secret Service, vassoio solo con AppIndicator.

1. AppImage: `chmod +x PlatypusDB*.AppImage` e avvio. Atteso: parte; i dati vanno in `~/.config/dev.platypusdb.desktop` e `~/.local/share/dev.platypusdb.desktop`. - [ ]
2. `.deb`: `sudo apt install ./PlatypusDB*.deb`, voce nel menu delle applicazioni, avvio; `.rpm` su Fedora allo stesso modo. Atteso: come sopra. - [ ]
3. Aggiornamento sopra la versione precedente (stesso comando; per l'AppImage, sostituire il file). Atteso: connessioni, script, impostazioni, cronologia e password intatti, come 1.3. - [ ]
4. Aggiornamento automatico (AppImage): avviso, download, riavvio come 1.4. Per `.deb` e `.rpm` l'app dice di aggiornare con il gestore di pacchetti ("Aggiornalo con {manager}..."). - [ ]
5. Password: con GNOME Keyring o KWallet, salvare una connessione con password, riavviare. Atteso: la password c'è (`secret-tool search service platypusdb`). Senza servizio (ambiente minimale): messaggio chiaro e master password come alternativa. - [ ]
6. Disinstallare (`apt remove`). Atteso: restano `~/.config/...`, `~/.local/share/...` e le voci del Secret Service. - [ ]
7. Copia portatile: cartella `data` accanto all'eseguibile (AppImage estratta con `--appimage-extract`). Atteso: usata al posto delle cartelle di `~`. - [ ]
8. Vassoio con l'estensione AppIndicator di GNOME, come 2.1. Senza: errore "this system has no tray for the icon (install AppIndicator)", nessun crash. - [ ]
9. Notifica di una query lunga: nel centro notifiche, come 2.2. - [ ]
10. `xdg-open file.platypus` e `xdg-open platypus://...` dopo il `.deb`: aprono l'app (istanza unica). Con la sola AppImage, senza integrazione del desktop, può non funzionare: annotare. - [ ]
11. Trascinare file dal gestore di file sulla finestra (Wayland e X11): come 2.3 punti 4-7. - [ ]
12. Scala frazionaria (125%, 150%) su Wayland e X11, due monitor con scale diverse: come 2.4 punti 4-5. - [ ]
13. Aspetto chiaro/scuro del desktop. Atteso: l'app segue il sistema. - [ ]
14. Barra del titolo con decorazioni del sistema (GNOME) e senza (alcuni WM). Atteso: si sposta, ridimensiona, chiude. - [ ]
15. Layout di tastiera italiano e inglese e le scorciatoie di [SHORTCUTS.md](SHORTCUTS.md). Atteso: tutte funzionano. - [ ]

---

## 4. macOS

`⏭ serve un Mac`: finché non c'è, si salta. Poi si riusano le liste 1, 2 e 3 così: DMG al posto di
NSIS/MSI (trascinare in Applicazioni, firma, notarizzazione, avviso di Gatekeeper), dati in
`~/Library/Application Support/dev.platypusdb.desktop`, password nel Portachiavi (servizio
`platypusdb`), icona nella barra dei menu, notifiche con il permesso al primo uso, file e
`platypus://` consegnati come URL all'app già aperta, Apple silicon e Intel, schermi Retina.

---

## 5. Percorsi da utente

Ogni percorso lo fa un tester su una versione installata (non dal dev server), con un profilo
pulito (`-Restore` o una `PLATYPUS_DATA_DIR` nuova), una volta in italiano e una in inglese, e una
volta da sola tastiera. Dove esiste uno scenario automatico, la lista copre solo ciò che quello non
prova (installer, file e database veri, sistema, giudizio sulla resa). Atteso generale: nessun
errore in `platypus.log`, nessun testo in lingua sbagliata, il focus sempre visibile.

Ogni percorso ha uno scenario completo `s-jrn-<percorso>-<en|it|keys>` (in inglese, in italiano
con il controllo che nessun testo resti in inglese o tagliato, e da sola tastiera), con un riavvio
vero dell'app a metà; restano a mano solo le parti fuori dalla portata degli E2E.

| Percorso | Scenari automatici |
|---|---|
| Primo avvio | `s-jrn-first-start-*`, `qa2-jrn-ok-first-day`, `qa2-jrn-ok-keyboard`, `languages-sqlite`, `qa3-url-italian` |
| Import da DBeaver | `s-jrn-import-dbeaver-*`, `qa2-jrn-ok-import-organise`, `qa2-jrn-import-relative-sqlite` |
| Giornata di lavoro | `s-jrn-workday-*`, `qa2-jrn-restore-table-tab`, `qa2-jrn-new-editor-focus`, `qa2-jrn-toolbar-wrap-transaction`, `qa2-jrn-prod-drop-uncommitted` |
| Analisi | `s-jrn-analysis-*`, `qa2-jrn-ok-analyst`, `qa2-jrn-count-locale` |
| Sviluppo | `s-jrn-development-*`, `qa2-jrn-ok-developer`, `qa3-schema-it-compare-warnings` |
| Agenti | `s-jrn-agents-*`, `qa2-jrn-ok-agent-mcp` |
| Italiano | `qa2-jrn-it-backend-errors`, `qa2-jrn-it-lint-messages`, `qa2-jrn-it-menu-wording`, `qa2-jrn-it-query-manager` |

### 5.1 Primo avvio

1. Avviare con profilo vuoto. Atteso: schermata iniziale nella lingua del sistema. - [ ]
2. Da tastiera: Tab fino a "Nuova connessione", Invio, scegliere il tipo scrivendo, compilare, Invio. Atteso: connessione salvata. - [ ]
3. Provare con un PostgreSQL e un MySQL veri (non solo SQLite). Atteso: connessione riuscita; errori di password leggibili. - [ ]
4. Aprire una tabella, modificare una cella, Ctrl+S. Atteso: la modifica persiste. - [ ]
5. **Riavvio**: chiudere con Alt+F4 e riaprire. Atteso: schede e connessione come prima, nessuna richiesta di password. - [ ]
6. Cambiare lingua (italiano ↔ inglese) e riaprire. Atteso: menu, errori e finestre nella lingua scelta. - [ ]

### 5.2 Import da DBeaver

1. Esportare un progetto vero di DBeaver (con una connessione con password salvata, una SSH, una a file) e importarlo dal primo avvio. Atteso: connessioni, cartelle, tipi (produzione/test) e tunnel ci sono; le password salvate, dove leggibili, sono prese. - [ ]
2. Importare lo stesso progetto una seconda volta. Atteso: nessun duplicato, o una scelta chiara. - [ ]
3. Spostare una connessione in una cartella nuova, segnare la produzione, la replica in sola lettura. Atteso: una scrittura su produzione chiede conferma; sulla replica è rifiutata. - [ ]
4. **Riavvio**. Atteso: progetti, cartelle, posizioni, tipi e sola lettura ci sono. - [ ]

### 5.3 Giornata di lavoro

1. Aprire tre connessioni, due editor, una tabella. Eseguire una query oltre la soglia di notifica con un'altra app in primo piano. Atteso: notifica (2.2). - [ ]
2. Salvare uno script, rinominarlo, cercarlo dalla ricerca globale. Atteso: si trova. - [ ]
3. Riaprire una query vecchia dalla cronologia. Atteso: il testo è giusto. - [ ]
4. Con il blocco per inattività attivo, lasciare l'app ferma oltre il tempo. Atteso: al ritorno chiede il master password; le connessioni di produzione sono chiuse. - [ ]
5. **Riavvio** con una transazione aperta. Atteso: avviso alla chiusura; dopo la riapertura nessuna transazione rimasta. - [ ]
6. Cambiare zoom e tema; riavviare. Atteso: restano. - [ ]

### 5.4 Analisi

1. Caricare un risultato grande (almeno 100.000 righe). Atteso: la griglia scorre senza scatti. - [ ]
2. SQL sul risultato, statistiche di colonna, grafico, esportazione in PNG e in Excel. Atteso: file che si aprono con l'app giusta. - [ ]
3. Importare un `.xlsx` e un `.parquet` veri trascinandoli. Atteso: tabelle con i tipi giusti. - [ ]
4. "Confronta con l'esecuzione precedente" dopo un cambio dati. Atteso: righe cambiate evidenziate. - [ ]
5. **Riavvio**. Atteso: schede di analisi e risultati salvati ci sono. - [ ]

### 5.5 Sviluppo

1. Script con `@include` di un secondo file relativo, eseguito da un file. Atteso: gli oggetti vengono creati. - [ ]
2. Controllo dello schema, applicazione di una correzione, Migration status su una cartella vera (Flyway; Liquibase, Prisma o Alembic se ci sono). Atteso: stato giusto. - [ ]
3. Confronto di schema tra due database e script di sincronizzazione. Atteso: script da rivedere prima dell'esecuzione. - [ ]
4. Dati finti su tabelle con chiavi esterne. Atteso: riferimenti validi. - [ ]
5. **Riavvio**. Atteso: la scheda dello script torna con il suo file. - [ ]
6. Da tastiera: Ctrl+T apre un editor e vi si scrive subito; Escape poi Tab esce dall'editor. Atteso: come in [SHORTCUTS.md](SHORTCUTS.md). - [ ]

### 5.6 Agenti

1. Impostazioni → Agenti: registrare Claude Code (o un altro client installato davvero). Atteso: il comando proposto funziona e il client elenca il server `platypus`. - [ ]
2. Dal client leggere una tabella. Atteso: dati; la password della connessione non compare mai. - [ ]
3. Scrittura con accesso "Sola lettura". Atteso: rifiutata. Consentire le scritture dalla connessione e riprovare. Atteso: conferma nell'app, poi riuscita. - [ ]
4. Segnare la connessione come produzione. Atteso: di nuovo solo lettura. - [ ]
5. Registro delle chiamate dell'agente, nella cartella dei dati. Atteso: ogni chiamata c'è. - [ ]
6. **Riavvio** dell'app e del client. Atteso: il server riparte (`platypus-db.exe --mcp`) con lo stesso profilo. - [ ]
7. Dopo un aggiornamento dell'app (1.4). Atteso: il client funziona senza riconfigurarlo. - [ ]
