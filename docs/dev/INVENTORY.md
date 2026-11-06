[← README](../../README.md)

# Inventario visivo

Uno screenshot di ogni schermata e dialog dell'app, in quattro varianti: tema scuro e chiaro, interfaccia in inglese e in italiano. Serve a chi rivede l'aspetto dell'app (gruppo R della [roadmap](ROADMAP.md)) per annotare, schermata per schermata, cosa non va.

## Come si lancia

Servono la build di release (`npm --prefix apps/desktop run tauri build -- --no-bundle`) e Windows (gli E2E guidano WebView2). I quattro scenari partono solo con `PLATYPUS_INVENTORY` impostata, quindi un normale `npm run e2e` li salta.

```bash
PLATYPUS_INVENTORY=1 PLATYPUS_E2E_SHOTS=<cartella> npm --prefix apps/desktop run e2e -- inventory
```

Il filtro `inventory` lancia tutte e quattro le varianti (circa 40 secondi ciascuna); `inventory-dark-en`, `inventory-dark-it`, `inventory-light-en`, `inventory-light-it` ne lanciano una sola. Per provare solo alcune schermate: `PLATYPUS_INVENTORY_ONLY=er-diagram,transfer`.

Con `PLATYPUS_INVENTORY_SIZE=1280x720` (o `2560x1440`, o qualunque `LxA`) la finestra ha quella misura invece di 1440×900 e il nome della cartella la riporta: `inventory/dark-en-1280x720/`. Serve alla revisione UX (gruppo T), che guarda l'app piccola e grande. La misura è impostata con `Emulation.setDeviceMetricsOverride` (il viewport della pagina, non la finestra di Tauri).

Le schermate della revisione UX (dal costruttore di query in poi) usano il PostgreSQL di `docker/compose.yml` (`docker compose -f docker/compose.yml up -d --wait`, porta 55432) per l'amministrazione del server; se non risponde, quella schermata viene saltata con la ragione nel log.

Le immagini finiscono in `<PLATYPUS_E2E_SHOTS>/inventory/<variante>/<NN>-<schermata>[-<parte>].png` (1440×900 se non si indica la misura), per esempio `inventory/light-it/17-er-diagram.png`. Non si committano. Una schermata che non si raggiunge viene saltata con una riga `SKIPPED` nel log e un `_failed-<schermata>.png` con lo stato della pagina: le altre proseguono.

## Dove sta il codice

- `apps/desktop/e2e/inventory.mjs`: il database di esempio, la lista delle schermate e le funzioni che le raggiungono.
- `apps/desktop/e2e/scenarios/inventory-<tema>-<lingua>.mjs`: i quattro scenari, che differiscono solo per tema e lingua scritti in `settings.toml`.

Una schermata nuova è una voce di `screens` in `inventory.mjs`: un nome e una funzione che porta l'app lì e chiama `shoot()` (con un nome di parte, se ne fotografa più d'una).

## Schermate

La colonna "Problemi trovati" la compila chi rivede.

| Schermata | Problemi trovati |
|---|---|
| Benvenuto (nessuna scheda aperta) | [chiaro] il badge DEV (verde pallido) ha poco contrasto; il blocco Script del navigator mostra righe vuote ("Nessuno script salvato", "Nessun risultato salvato") senza azione; [it] la scorciatoia Ctrl+Maiusc+T esce dal badge del tasto; il pannello scorciatoie cambia posizione quando compare "Recenti" (salto di layout) |
| Navigator espanso (connessione, schema, tabelle, viste) | riga selezionata marrone/arancio piena, troppo pesante (scuro); i conteggi ("Tables 5") sono grigi a ~10px, poco leggibili; il puntino di stato si sovrappone all'angolo dell'icona del driver |
| Editor SQL con griglia dei risultati | voce selezionata del completamento arancio pieno, fuori tono; la barra degli strumenti ha ~14 icone senza etichetta né raggruppamento chiaro; il suggerimento della griglia è in monospazio e si taglia con editor stretto; [it] "FROM result" non tradotto nel suggerimento |
| Cronologia delle query | il pulsante cronologia va a capo su una seconda riga sotto la barra (toolbar troppo stretta con il pannello aperto); il suggerimento della griglia si taglia ("G..."); lista cronologia a 11px con icone azione a destra non allineate; il menu di completamento ha un'ombra sproporzionata |
| Stato di errore (query che fallisce) | l'errore è una striscia rossa semplice, non il blocco di stato condiviso (icona, titolo, azione utile come Copia o Vai alla riga); testo monospazio senza titolo; nessun segno di errore sulla scheda |
| Stato vuoto (risultato senza righe) | la griglia a 0 righe mostra solo intestazioni e colonne vuote: nessun testo "nessuna riga" né blocco di stato condiviso |
| Griglia con una cella modificata e non salvata | cella modificata e cella attiva quasi indistinguibili (stesso arancio, bordo contro riempimento); la barra della griglia ha 6 pulsanti-icona senza etichetta più il suggerimento; Pivot, Struttura e Modifica la tabella hanno stili di pulsante diversi |
| Pagina dell'oggetto: Proprietà, Colonne, Indici, Vincoli, Chiavi esterne, Riferimenti, Usato da, DDL | le tabelle di Proprietà e Chiavi esterne sono larghe ~540px con vuoto a destra, Colonne è a tutta larghezza; in Colonne le celle non sono allineate in verticale (nome in alto, Tag e Sensibile al centro) e il tipo è in monospazio viola; il campo Nota è una textarea nuda con maniglia di ridimensionamento; intestazioni di tabella più piccole del corpo; contatori delle tab grigi quasi invisibili; il riquadro DDL ha un bordo diverso dalle tabelle |
| Editor di tabella (Modifica la tabella…) | intestazione diversa dalla pagina oggetto (campo nome al posto del titolo); checkbox nativi non stilizzati; campi alti ~30px contro le righe più basse della pagina oggetto; "Aggiungi colonna" è un link di testo senza icona; Salva… disabilitato marrone poco leggibile |
| Diagramma ER | manca la minimappa; i pulsanti della barra (zoom, Auto layout, SVG, PNG) e dell'intestazione sono senza icone; la barra del diagramma ha sfondo e bordo diversi dai pulsanti dell'intestazione; [scuro] intestazioni delle tabelle marroni con poco stacco dal fondo |
| Piano di esecuzione: dialog "Apri un piano" e piano aperto | dialog: textarea con bordo arancio pieno diverso dagli altri campi, select nativo, Apri disabilitato marrone, larghezza diversa dagli altri dialog; piano: riga selezionata marrone piena; il pannello destro mostra solo "Operation / QUERY PLAN" senza contenuto utile; tab Tree/Graph/Text/Table senza icone |
| Analisi / profilo dei dati di una tabella | valori troncati con "…" (Quartili "2,946.75 · 2,…", Minimo "2026-10-0…"); istogrammi arancio pieno molto accesi; barra di scorrimento verticale nativa dentro la pagina; riquadri KPI (numeri grandi) e righe colonna (testo 11px) con tipografia molto diversa |
| Dashboard | la barra superiore non ha il blocco intestazione (icona, titolo) delle altre pagine; Aggiungi widget, Aggiorna, Salva sono testo nudo senza bordo né icona, mentre altrove sono pulsanti; lo stato vuoto non ha un'azione (solo testo grassetto, nessun pulsante "Aggiungi widget"); il campo nome è un input molto grande |
| Dialog di esportazione | [it] il dialog va in overflow orizzontale: "Esporta in una cartella…" è tagliato e compare una barra di scorrimento orizzontale; larghezza ~360px (altri dialog 520-900px); pulsanti a 11px più piccoli di quelli di Importa; select nativi; titolo e padding diversi dagli altri dialog; lo sfondo resta nitido, scurimento debole |
| Dialog di importazione | ha una linea di separazione sopra i pulsanti che gli altri dialog non hanno; Importa è disabilitato marrone; "Salva come profilo…" grigio quasi illeggibile; larghezza ~900px contro ~360px dell'export; campo file con bordo arancio e segnaposto lungo |
| Trasferimento (Copia lo schema in un database) | descrizione a 11px lunga e fitta con elenco di tabelle; select nativi; larghezza ~520px diversa dagli altri dialog; pulsanti piccoli; nessun riepilogo strutturato delle tabelle |
| Scelta del driver | nomi troncati con "…" ("Elasticsearch / ...", "Apache Cassan...", "Other ADBC driv...", "Other ODBC dri..."); icone dei driver di stili diversi (loghi colorati, tile scuri, icona DB generica per SQL Server, Oracle, Yugabyte, ODBC); la griglia a 4 colonne ha l'ultima riga con una cella sola |
| Form di connessione: Generale, Aspetto e sicurezza, Rete, Init | l'altezza del dialog cambia tra le schede e i pulsanti saltano; etichette allineate a destra con larghezze diverse e griglia irregolare (Database/Pooler, User/Password); in Aspetto e sicurezza mancano le intestazioni di sezione presenti in Generale; checkbox con testi lunghi su due righe non allineati; indicatore dei passi "1 Database — 2 Connessione" a 11px |
| Rail dei progetti e dialog del progetto | dialog: campo Iniziali vuoto senza segnaposto; selettore colore con 8 pallini più un rettangolo "personalizzato" non spiegato; elenco connessioni con icone da 24px sproporzionate rispetto al testo 11px; checkbox nativi; nella rail il pulsante + tratteggiato ha uno stile diverso dagli altri elementi |
| Palette dei comandi | le lettere corrispondenti evidenziate in arancio e sparse ("Show or hide") rendono le righe difficili da leggere; l'ultima riga è tagliata dal piè di pagina ("Previous project"); barra di scorrimento nativa; le azioni disabilitate (Pivot) hanno un motivo lungo a destra non allineato alle altre colonne |
| Impostazioni: ogni pagina e ogni sotto-scheda (Aspetto, Navigator, Editor SQL, Risultati, Barra, Schede, Temi, Scorciatoie, Sicurezza, Connessioni, Prestazioni, Password principale, Sincronizzazione, Aggiornamento dei driver, Agenti, Driver, Plugin, Diagnostica) | le sotto-schede di Aspetto sono troncate anche in inglese ("The...", "Dens...", "Lay...", "Icons and animati...", "Wind..."); la barra laterale taglia "Keyboard shortc..." e nasconde Plugin e Diagnostica sotto il bordo (serve scorrere); tre barre di scorrimento nidificate con frecce native; icone ripetute (Barra e Impostazioni, Temi e Aspetto, Schede e Navigator); Temi quasi vuota (due pulsanti, nessun elenco di temi); Scorciatoie mostra l'identificatore grezzo "dashboard.widget" e molte voci vuote (—); Sincronizzazione con spaziatura verticale stretta (etichette attaccate ai campi) e segnaposto tagliato; "LIMIT of Generate SELECT" con maiuscole incoerenti; Driver e Plugin senza elenco a schede (testo lungo a 11px); Impostazioni è un dialog e non una pagina con intestazione come da mockup; [it] voci tagliate "Barra degli stru...", "Scorciatoie da ta...", "Password princi...", "Aggiornamenti d..." e sotto-schede "Te...", "Lingua e te...", "Disposizio...", "Fines..."; [it] nomi dei temi non tradotti ("Platypus Dark", "Solarized Light"); [chiaro] poco stacco tra dialog e fondo grigio |
| Pannello degli agenti (pagina Agenti delle Impostazioni, ogni scheda) | "Not installed" va a capo su due righe nella colonna Stato; i percorsi dei file di configurazione vanno a capo e si troncano ("claude_deskto p_config.json") in monospazio 10px; tre livelli di barre di scorrimento (dialog, pagina, blocco JSON); paragrafo introduttivo lungo a 11px prima delle schede; [it] "Non installato" a capo e "non ancora presente" spezza i percorsi |
| Test delle connessioni | dialog stretto con lista a 11px; lo stato è indicato solo dal colore del pallino (nessuna icona); scurimento dello sfondo debole |
| Dialog di conferma (eliminare una tabella) | il pulsante Drop è rosso/rosa salmone, diverso dal rosso usato altrove per gli errori; "Open in editor" è un link di testo arancio al posto di un pulsante secondario; larghezza ~640px diversa dagli altri dialog |

### Schermate della revisione UX (gruppo T)

Ogni voce è un nome di schermata di `inventory.mjs` e le immagini che produce:

| Schermata | Cosa fotografa |
|---|---|
| `query-builder` | costruttore visuale: una tabella; due tabelle unite con colonne scelte |
| `notebook` | notebook vuoto; due celle SQL eseguite |
| `chart-pivot` | grafico a barre e a torta; dialog del pivot; risultato del pivot |
| `schema-tools` | Confronta con… (dialog e risultato); Controlla lo schema |
| `server-admin` | Attività del server su PostgreSQL, una foto per sezione (sessioni, blocchi, utenti, parametri…) |
| `lock-screen` | schermata di blocco (si abilita e poi si toglie la password principale) |
| `masking` | griglia con i dati sensibili mascherati e l'indicatore in alto |
| `snapshots` | registro delle transazioni e script di ripristino |
| `agents-mcp` | pagina Agenti delle impostazioni, una foto per client |
| `sync-sharing` | Sincronizzazione, menu e dialog di condivisione del progetto, apertura di un progetto condiviso |
| `scheduled-tasks` | task pianificati: stato vuoto e form di un nuovo task |
| `transfer-profiles` | profili di trasferimento (stato vuoto) |
| `table-menu-submenu` | menu di una tabella e ogni suo sottomenu aperto |
| `pinned-tab` | menu della scheda e barra con una scheda fissata |
| `grid-editing` | editor di cella, anteprima SQL, riga nuova |
| `column-hover` | riquadro del profilo di colonna sull'intestazione |
| `connection-ssh` | form di connessione, pagina Rete con il tunnel SSH |
| `import-connections` | dialog di importazione delle connessioni |

Le pagine delle impostazioni sono già in `settings`.

## Priorità

I dieci problemi da correggere per primi, in ordine di importanza.

1. **Impostazioni**: sotto-schede e voci della barra laterale troncate con "…" (anche in inglese: "The...", "Dens...", "Keyboard shortc..."); Plugin e Diagnostica nascosti in fondo alla lista; tre barre di scorrimento nidificate.
2. **Dialog di esportazione [it]**: "Esporta in una cartella…" tagliato e barra di scorrimento orizzontale nel dialog.
3. **Scelta del driver**: nomi troncati ("Elasticsearch / ...", "Apache Cassan...", "Other ADBC driv...") e icone dei driver di stili diversi.
4. **Stato di errore, stato vuoto e Dashboard vuota**: striscia rossa, griglia a 0 righe e dashboard senza azione non usano il blocco di stato condiviso (icona, titolo, azione).
5. **Dialog (esportazione, importazione, trasferimento, conferma, test)**: larghezze, padding, altezza dei pulsanti e separatore diversi; select nativi; scurimento dello sfondo debole.
6. **Pagina dell'oggetto**: tabelle a ~540px contro Colonne a tutta larghezza, celle non allineate in verticale, textarea Nota nuda, contatori delle tab quasi invisibili.
7. **Barre degli strumenti dell'editor SQL e della griglia**: 14+ icone senza etichetta né gruppi; con il pannello Cronologia il pulsante va a capo su una seconda riga.
8. **Arancio e marrone pieni nel tema scuro**: voce di completamento, istogrammi del profilo, selezione di navigator e piano; cella modificata e cella attiva indistinguibili nella griglia.
9. **Dashboard e diagramma ER**: pulsanti di testo nudo o senza icone, intestazione diversa dalle altre pagine, minimappa mancante.
10. **Form di connessione e dialog del progetto**: altezza del dialog che cambia tra le schede, griglia irregolare, sezioni mancanti in "Aspetto e sicurezza"; campo Iniziali senza segnaposto; [it] "Ctrl+Maiusc+T" esce dal badge del tasto (benvenuto).


## Correzioni (30 ottobre 2026)

I dieci problemi prioritari sono stati corretti nel gruppo R, poi verificati sugli screenshot rifatti:
impostazioni (sotto-schede e pagine senza tagli, un solo scorrimento, schede dei temi a tutta
larghezza), nomi dei driver su due righe, dialog con la stessa testata, larghezza, piè e velo,
stati vuoti e di errore comuni, tabelle della pagina dell'oggetto a tutta larghezza, barre
dell'editor raggruppate con i separatori, accento tenue al posto dei pieni arancio, cella
modificata distinta da quella attiva, intestazioni di dashboard e diagramma ER come le altre
pagine, form di connessione ad altezza fissa con le sezioni di "Aspetto e sicurezza".

Restano, fuori dal gruppo:

- un menu "⋯" per i pulsanti della barra dell'editor che non entrano quando la finestra è stretta
  e le etichette sono attive;
- alcune etichette inglesi da riscrivere (le chiavi dei testi, usate anche dagli scenari E2E):
  "Query with SQL", "Take out", "Take out of a role", "Made for {place}", "Keep {name} after
  all?", "Turns {schema} as of {from} into {to}", "Reload from result", "Run script button".
