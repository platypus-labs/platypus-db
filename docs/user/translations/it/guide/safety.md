<!-- source: db93b8bc91b0 -->
[← Guida utente](README.md)

# Lavorare in sicurezza: produzione, sola lettura, conferme, mascheramento

Tutto quello che c'è qui è anche in [FEATURES.md](../FEATURES.md#transactions-and-safety).

![Impostazioni, la pagina Sicurezza](../../screenshots/guide/safety.png)

## Segnare una connessione come produzione

1. Modifica la connessione e apri la pagina **Aspetto e sicurezza**.
2. Imposta l'ambiente su **Produzione** (le altre scelte sono Sviluppo e Test). Produzione chiede
   conferma prima delle modifiche e parte in commit manuale.
3. Se vuoi, scegli un colore, per riconoscere facilmente la connessione.

Da quel momento ogni modifica su quella connessione viene elencata in una finestra intitolata
"Modifiche su `<name>` (produzione)", con le istruzioni, e viene eseguita solo dopo la tua
conferma. Impostazioni (`Ctrl+,`) › **Sicurezza** › **Chiedi prima delle modifiche sulle
connessioni di produzione** lo disattiva per l'app. Gli agenti non vanno mai oltre la sola lettura
su una connessione di produzione.

## Rendere una connessione di sola lettura

1. Nella stessa pagina, spunta **Sola lettura (rifiuta le modifiche ai dati)**.
2. Un'istruzione che può modificare i dati non viene eseguita. L'editor mostra "`<name>` è in sola
   lettura e questo può modificare i dati." senza via d'uscita. Lo stesso vale per Explain con
   analyze ("questa istruzione può modificare i dati."), per un file incluso con `@include`, per una
   cella di notebook ("questa cella potrebbe modificare dati."), per il client a riga di comando e
   per un'esecuzione su più connessioni, che elenca la connessione come saltata ("Connessione in
   sola lettura").
3. Non sono offerti affatto: **Importa i dati…**, **Importa i dati in una nuova tabella…**,
   **Incolla un CSV come nuova tabella…**, **Genera dati…**, **Esegui file SQL…**, la creazione di
   oggetti, il salvataggio nel designer delle tabelle e la modifica delle righe nella griglia. Gli
   agenti restano in sola lettura.

Il pulsante lucchetto di una scheda dell'editor rende di sola lettura solo quella scheda, senza
cambiare la connessione salvata. È l'unico caso che chiede conferma: l'editor mostra lo stesso
messaggio con un pulsante **Esegui comunque**, e il client a riga di comando chiede "Run anyway?".

La stessa pagina ha altre due caselle: **Niente export** (i suoi dati non vanno in file, altri
database o agenti) e **Niente modifica dei dati** (le righe cambiano solo con SQL, non nella
griglia, negli import o nei dati generati).

## Conferme

Impostazioni › **Sicurezza** › **Prima delle modifiche**:

- **UPDATE o DELETE senza WHERE** (anche `TRUNCATE`): **Chiedi prima di eseguire** (il
  predefinito), **Non eseguire mai** o **Esegui senza chiedere**. La finestra chiede "Modificare
  tutte le righe?" e dice più o meno quante righe.
- **Avvisa quando una transazione resta aperta per (minuti)**: 10 per impostazione predefinita, 0
  non avvisa mai.
- **Aggiungi un LIMIT alle query che non lo hanno (righe)**: 0 (il predefinito) non lo aggiunge
  mai.
- **Chiedi prima che gli agenti modifichino dati**: vedi [Agenti](agents.md).

## Annullare una modifica

Attivo per impostazione predefinita, **Conserva le righe che un UPDATE o DELETE modifica, per
rimetterle** (Impostazioni › Sicurezza › **Blocco e copie**) conserva le righe di un `UPDATE` o
`DELETE` su una tabella, fino a 10.000, eseguito dall'editor. Apri il **Registro delle
transazioni** nella barra degli strumenti dell'editor e fai clic su **Ripristina…**, oppure
esegui **Snapshot prima delle modifiche…** dalla palette dei comandi, per ottenere lo script che le
rimette a posto.

## Mascherare i dati sensibili

1. Premi `Ctrl+Alt+M` (o esegui **Maschera i dati sensibili** dalla palette dei comandi): i valori
   delle colonne sensibili appaiono come puntini mentre lo schermo è condiviso. Le copie mantengono
   i valori reali.
2. Impostazioni › **Sicurezza** › **Mascheramento** stabilisce quali colonne sono sensibili, per
   nome (`*password*`, `*token*`, `*email*` e altri per impostazione predefinita; `*` è un
   qualsiasi insieme di caratteri) e per tipo. Puoi anche segnare una colonna nella scheda della
   sua tabella.

Anche gli agenti leggono le colonne sensibili mascherate, a meno che tu non spunti **Mostra agli
agenti i valori delle colonne sensibili** per quella connessione in Impostazioni › Agenti ›
**Sicurezza**. Viene mascherato anche un campo sensibile dentro un documento MongoDB
(`user.password`). Quando un agente chiede su cosa stai lavorando, viene mascherata anche una
colonna letta con un altro nome (`SELECT email AS contact`); se l'istruzione la nasconde dietro una
subquery, viene mascherato tutto il risultato condiviso.

Una regola o un segno rimosso fuori dall'app (un agente con strumenti sui file che modifica
`settings.toml` o `catalog.db`) non arriva agli agenti finché non lo accetti: l'app te lo chiede, e
Impostazioni › Agenti › **Sicurezza** lo elenca, con un modo per accettarlo o annullarlo. Quello
aggiunto fuori dall'app vale subito. Questo copre le modifiche a quei file e nient'altro. Un
agente che può eseguire comandi come te può leggere il database per altre vie, o il portachiavi
del sistema in cui vive il sigillo: per tenere i valori lontani da un agente così, eseguilo come
un altro utente e dai alla connessione un utente del database senza accesso a quelle colonne.

Una risposta vale per ciò che la domanda elencava: se i file cambiano ancora mentre è aperta,
**Accept** non accetta nulla e l'elenco viene mostrato di nuovo.

## Bloccare l'app

Con una password principale impostata (vedi [Connettersi](connect.md#password-principale)),
Impostazioni › **Sicurezza** › **Blocca l'app dopo questi minuti di inattività** (0, il
predefinito, non blocca mai) blocca PlatypusDB e chiude le connessioni di produzione. **Blocca
PlatypusDB** nella palette dei comandi lo blocca subito. Inserisci la password principale per
sbloccare: le connessioni di produzione si riconnettono quando vengono usate.
