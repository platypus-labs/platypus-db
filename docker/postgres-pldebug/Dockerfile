# PostgreSQL 17 with the PL/pgSQL debugger (pldebugger, the `pldbgapi` extension), for the
# routine debugger. The image is the dev `postgres` one plus the PGDG package: its library
# must be preloaded (`shared_preload_libraries = plugin_debugger`, set in compose.yml).
FROM postgres:17

RUN apt-get update \
    && apt-get install -y --no-install-recommends "postgresql-${PG_MAJOR}-pldebugger" \
    && rm -rf /var/lib/apt/lists/*
