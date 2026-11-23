# PlatypusDB's MCP server (`platypus-db --mcp`, over stdio) from the Linux .deb of a release,
# for directories that start a server in a container to check it, such as Glama. It runs
# without a display; with no saved connections it answers with the tools and an empty list.
# A template like the other manifests: `node scripts/packaging.mjs` writes it filled, then
#   docker build -t platypusdb-mcp <out>/mcp
#   docker run -i --rm platypusdb-mcp
FROM debian:trixie-slim
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates curl \
 && curl -fsSL -o /tmp/platypusdb.deb "@URL_LINUX_DEB@" \
 && apt-get install -y --no-install-recommends /tmp/platypusdb.deb \
 && rm -rf /tmp/platypusdb.deb /var/lib/apt/lists/*
ENTRYPOINT ["platypus-db", "--mcp"]
