<!-- source: aa482a6e3843 -->
[← Guida utente](README.md)

# Progetti e condivisione

Un progetto è un gruppo di connessioni, script e schede aperte, mostrato nella barra a sinistra. I
dettagli sono in [FEATURES.md](../../../FEATURES.md#workspace) e
[FEATURES.md](../../../FEATURES.md#sharing-and-sync).

## Creare un progetto

1. Esegui **Nuovo progetto…** dalla palette dei comandi (`Ctrl+K`).
2. Dagli un nome, poi scegli le connessioni da mostrare al suo interno. Una connessione si può
   aggiungere anche dal suo modulo: **Progetti**, in fondo alla pagina **Aspetto e sicurezza**.
3. `Ctrl+1` mostra tutte le connessioni, da `Ctrl+2` a `Ctrl+9` i progetti nell'ordine della barra.

## Condividere una query

1. In una scheda dell'editor SQL, esegui **Condividi query…** (menu della scheda o palette dei
   comandi).
2. Copia il link `platypus://…`, oppure salva un file `.platypus`. Contiene l'SQL, il suo titolo e,
   se lo chiedi, i valori dei parametri; nomina il database ma mai una password.
3. Il collega lo apre con **Apri un link condiviso…** o **Apri una query condivisa…**, oppure
   trascinando il file sulla finestra. L'app trova la connessione salvata corrispondente oppure ne
   propone una nuova già compilata, e mostra la query senza eseguirla.

## Condividere un progetto con un team

1. Fai clic con il tasto destro sul progetto nella barra e scegli **Condividi progetto…**, poi
   scegli una cartella che il team tiene in Git o su un'unità condivisa. Contiene le connessioni
   (senza credenziali) e gli script.
2. Ogni collega fa clic su **Apri progetto condiviso…** nella schermata di benvenuto, sceglie la
   cartella e inserisce le proprie password.
3. Dopo una modifica nella cartella, esegui **Rileggi la cartella condivisa** dal menu del
   progetto.

Sicuro per costruzione: una connessione nuova che arriva da un progetto condiviso dà agli agenti
**nessun accesso** sul tuo computer finché non scegli diversamente, e le istruzioni che il file
esegue a ogni connessione vengono mostrate prima e vengono eseguite solo dopo che hai approvato
quel testo esatto.

## Portare le connessioni su un altro computer

Nella schermata di benvenuto, **Esporta le connessioni** scrive un file `.platypusconn`: scegli le
connessioni e, se vuoi, una password di almeno 8 caratteri, che cifra il file e gli permette di
portare con sé le password salvate. Sull'altro computer, **Importa le connessioni** lo legge. Senza
password il file non contiene alcuna credenziale.

Per tenere allineati impostazioni, connessioni e script tra i tuoi computer, usa Impostazioni ›
**Sincronizzazione** con una cartella in un cloud drive. Vedi
[FEATURES.md](../../../FEATURES.md#sharing-and-sync).

## Script e Git

Quando la cartella degli script di un progetto è un repository Git, il pannello Script mostra il
branch e le modifiche per script, e offre **Commit…**, **Pull** e **Push**. Git deve essere
installato.
