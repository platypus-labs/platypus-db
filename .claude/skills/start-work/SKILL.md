---
name: start-work
description: Avvio di una sessione di lavoro su PlatypusDB - aggiorna la codebase, controlla le correzioni aperte, sceglie un gruppo disponibile della roadmap, prende il lock e crea il branch. Argomento facoltativo - la lettera del gruppo.
disable-model-invocation: true
argument-hint: "[lettera del gruppo]"
---

Segui "Prima di iniziare" di CLAUDE.md. In sintesi, in quest'ordine:

1. `git fetch origin`, `git checkout main`, `git pull --ff-only`. Se il working tree non è
   pulito, fermati e chiedi.
2. graphify: se `uv tool list` mostra una versione aggiornata più di 7 giorni fa (o non lo sai),
   `uv tool upgrade graphifyy`; se la versione cambia, `graphify install --platform windows`.
3. Leggi "Correzioni aperte" in docs/dev/ROADMAP.md. Se ci sono righe, proponi di correggere quelle
   e fermati qui finché l'utente non decide.
4. Leggi "Lock dei gruppi" e "Ondate consigliate". Per ogni `🔒` controlla la scadenza:
   `git log -1 --format=%cd --date=short origin/<branch>`, scaduto se più vecchio di 3 giorni
   rispetto a oggi.
   - Con argomento (`$ARGUMENTS`): verifica che quel gruppo sia disponibile e che le dipendenze
     siano pronte.
   - Senza argomento: proponi il gruppo secondo le regole del punto 3 di CLAUDE.md (ondata più
     bassa, poi P1/P2 aperte, poi più funzionalità aperte), con una riga di motivazione, e
     aspetta la conferma dell'utente.
5. Lock: su `main` modifica solo la riga del gruppo in `🔒 Paolo · <branch> · <AAAA-MM-GG>`
   (sempre "Paolo", mai "Claude"; data di oggi; branch `group-<lettera>-<tema>`).
   Attenzione a non introdurre un BOM nel file.
   `git add docs/dev/ROADMAP.md`, commit `chore(roadmap): lock gruppo <X>` senza righe di co-autore,
   `git push origin main`. Se rifiutato: `git pull --rebase`, ricontrolla la riga; se l'ha presa
   qualcun altro, torna al punto 4. Mai `--force`.
6. Crea il branch da `main` (o dal branch del gruppo da cui dipende, vedi CLAUDE.md) e pushalo
   con `git push -u origin <branch>`.
7. Riassumi all'utente: gruppo, branch, funzionalità da fare (dalla tabella del gruppo) e la
   suddivisione dei modelli prevista (punto 6 di CLAUDE.md: subagenti `implementer` Sonnet,
   `runner` Haiku, progettazione e review su Opus).
