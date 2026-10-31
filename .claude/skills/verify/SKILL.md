---
name: verify
description: Verifica completa di PlatypusDB prima di dire "fatto" o di unire in main - fmt, build del frontend, clippy, test, build di release, E2E filtrati e conformance sui container Docker. Argomento facoltativo - il filtro degli scenari E2E.
disable-model-invocation: true
argument-hint: "[filtro E2E]"
---

Punto 2 di "Dopo un'implementazione" in CLAUDE.md. Fai girare i comandi dal subagente `runner`
(Haiku), non nella sessione principale.

1. Filtro E2E: se c'è un argomento usa `$ARGUMENTS`. Altrimenti ricavalo dagli scenari del
   gruppo e delle aree toccate (`git diff --name-only origin/main...HEAD`, nomi in
   `apps/desktop/e2e/scenarios/`). Il runner accetta una sola sottostringa per esecuzione: con
   più aree, più esecuzioni. **Mai tutti gli E2E.**
2. Passa al `runner`, in quest'ordine:
   ```bash
   cargo fmt --all --check
   npm --prefix apps/desktop run build
   cargo clippy --workspace --all-targets
   cargo test --workspace --exclude platypus-desktop
   npm --prefix apps/desktop run tauri build -- --no-bundle
   npm --prefix apps/desktop run e2e -- <filtro>
   docker compose -f docker/compose.yml up -d --wait
   cargo test -p platypus-conformance -- --nocapture
   ```
   Build di release, E2E e conformance si fanno sempre prima di unire in `main`, anche se il
   gruppo non tocca UI o driver (punto 6 di CLAUDE.md).
3. Con un errore: correggilo (o delega all'`implementer` se è meccanico) e rilancia solo dal
   passo fallito in poi; alla fine rilancia tutto una volta.
4. Riporta: comando per comando passato/fallito, i database saltati dalla conformance (un
   `skipping` è un problema da segnalare) e cosa non è stato verificato.
