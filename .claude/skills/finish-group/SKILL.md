---
name: finish-group
description: Chiusura del lavoro su un gruppo di PlatypusDB - refactor, verifica, code review, documentazione, merge in main e rilascio del lock nella roadmap.
disable-model-invocation: true
---

Segui "Dopo un'implementazione" e "Chiusura del lock" di CLAUDE.md, in quest'ordine. Fermati e
chiedi all'utente se un passo fallisce e non è ovvio come correggerlo.

1. **Refactor**: lancia il subagente `refactorer` sul branch corrente.
2. **Verifica**: esegui la procedura di `/verify` (leggi `.claude/skills/verify/SKILL.md`).
3. **Code review**: `/code-review` sul branch; correggi i problemi trovati, poi riverifica le
   parti toccate.
4. **Documentazione**: riga "Stato" del gruppo in docs/ROADMAP.md; docs/EXTENDING.md se nasce un
   punto di estensione; docs/FEATURES.md per le funzionalità utente; README (inglese e italiano)
   solo se cambia il quadro generale. Commit separato `docs: ...`.
5. **Merge di `origin/main` nel branch**: `git fetch origin`, `git merge origin/main`, risolvi i
   conflitti, poi **rifai la verifica del passo 2** (deve essere verde dopo il merge).
6. **Prima di unire in main, chiedi conferma all'utente** mostrando: commit del branch, esito
   della verifica, stato che avrà il gruppo. Poi:
   ```bash
   git checkout main && git pull --ff-only
   git merge --no-ff <branch>
   git push origin main
   ```
   Se il push viene rifiutato, ripeti dal passo 5. Mai `--force`.
7. **Rilascio del lock**: aggiorna la riga del gruppo in "Lock dei gruppi" con
   - `✅ completato` solo se tutte le funzionalità P1, P2 e P3 sono fatte (escluse le righe
     `⏭ serve un Mac` e `⛔ bloccato`); anche la riga "Stato" diventa `**Stato: completato**`;
   - `✅ fatto (<priorità completate>)` se resta qualcosa;
   - `libero` se nessun livello di priorità è completo.
   Commit `chore(roadmap): rilascio gruppo <X>` (solo docs/ROADMAP.md, nessun BOM, nessuna riga
   di co-autore), `git push origin main`.
8. Problemi trovati lungo la strada e non corretti: aggiungili a "Correzioni aperte".
