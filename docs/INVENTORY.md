[← README](../README.md)

# Inventario visivo

Uno screenshot di ogni schermata e dialog dell'app, in quattro varianti: tema scuro e chiaro, interfaccia in inglese e in italiano. Serve a chi rivede l'aspetto dell'app (gruppo R della [roadmap](ROADMAP.md)) per annotare, schermata per schermata, cosa non va.

## Come si lancia

Servono la build di release (`npm --prefix apps/desktop run tauri build -- --no-bundle`) e Windows (gli E2E guidano WebView2). I quattro scenari partono solo con `PLATYPUS_INVENTORY` impostata, quindi un normale `npm run e2e` li salta.

```bash
PLATYPUS_INVENTORY=1 PLATYPUS_E2E_SHOTS=<cartella> npm --prefix apps/desktop run e2e -- inventory
```

Il filtro `inventory` lancia tutte e quattro le varianti (circa 40 secondi ciascuna); `inventory-dark-en`, `inventory-dark-it`, `inventory-light-en`, `inventory-light-it` ne lanciano una sola. Per provare solo alcune schermate: `PLATYPUS_INVENTORY_ONLY=er-diagram,transfer`.

Le immagini finiscono in `<PLATYPUS_E2E_SHOTS>/inventory/<variante>/<NN>-<schermata>[-<parte>].png` (1440×900), per esempio `inventory/light-it/17-er-diagram.png`. Non si committano. Una schermata che non si raggiunge viene saltata con una riga `SKIPPED` nel log e un `_failed-<schermata>.png` con lo stato della pagina: le altre proseguono.

## Dove sta il codice

- `apps/desktop/e2e/inventory.mjs`: il database di esempio, la lista delle schermate e le funzioni che le raggiungono.
- `apps/desktop/e2e/scenarios/inventory-<tema>-<lingua>.mjs`: i quattro scenari, che differiscono solo per tema e lingua scritti in `settings.toml`.

Una schermata nuova è una voce di `screens` in `inventory.mjs`: un nome e una funzione che porta l'app lì e chiama `shoot()` (con un nome di parte, se ne fotografa più d'una).

## Schermate

La colonna "Problemi trovati" la compila chi rivede.

| Schermata | Problemi trovati |
|---|---|
| Benvenuto (nessuna scheda aperta) | |
| Navigator espanso (connessione, schema, tabelle, viste) | |
| Editor SQL con griglia dei risultati | |
| Cronologia delle query | |
| Stato di errore (query che fallisce) | |
| Stato vuoto (risultato senza righe) | |
| Griglia con una cella modificata e non salvata | |
| Pagina dell'oggetto: Proprietà, Colonne, Indici, Vincoli, Chiavi esterne, Riferimenti, Usato da, DDL | |
| Editor di tabella (Modifica la tabella…) | |
| Diagramma ER | |
| Piano di esecuzione: dialog "Apri un piano" e piano aperto | |
| Analisi / profilo dei dati di una tabella | |
| Dashboard | |
| Dialog di esportazione | |
| Dialog di importazione | |
| Trasferimento (Copia lo schema in un database) | |
| Scelta del driver | |
| Form di connessione: Generale, Aspetto e sicurezza, Rete, Init | |
| Rail dei progetti e dialog del progetto | |
| Palette dei comandi | |
| Impostazioni: ogni pagina e ogni sotto-scheda (Aspetto, Navigator, Editor SQL, Risultati, Barra, Schede, Temi, Scorciatoie, Sicurezza, Connessioni, Prestazioni, Password principale, Sincronizzazione, Aggiornamento dei driver, Agenti, Driver, Plugin, Diagnostica) | |
| Pannello degli agenti (pagina Agenti delle Impostazioni, ogni scheda) | |
| Test delle connessioni | |
| Dialog di conferma (eliminare una tabella) | |
