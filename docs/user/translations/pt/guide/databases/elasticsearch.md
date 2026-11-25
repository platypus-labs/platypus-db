<!-- source: 2a42aace96dc -->
[← Bancos de dados](README.md)

# Elasticsearch e OpenSearch

Um só driver fala com os dois pela API HTTP deles (`reqwest` com rustls). O servidor diz qual é
quando você conecta, e a conexão o mostra (`Connected: OpenSearch 2.17.1`). O driver faz parte do
app; nada é baixado.

## Conectar

| Você cola | Lido como |
|---|---|
| `elasticsearch://user:pass@host:9200/logs-*` | HTTP simples; o caminho é um padrão de índice |
| `elasticsearchs://user:pass@host:9200` | HTTPS (verificado: `verify-full`) |
| `opensearch://…` e `opensearchs://…` | o mesmo, lembrado como OpenSearch para que a URL seja escrita de volta assim |

A porta padrão é 9200. O campo **Banco de dados** é um padrão de índice opcional (`logs-*`, ou vários
separados por vírgulas) que restringe o que o navegador lista; vazio, lista todo índice que não é
oculto.

- **Login**: um usuário e senha usam a autenticação Basic. Com o usuário vazio, o campo de senha é
  uma **chave de API** (o valor `encoded`), enviada como `Authorization: ApiKey …`.
- **TLS**: HTTPS para URLs `elasticsearchs://` e `opensearchs://` e para modos SSL de `require` para
  cima; `elasticsearch://` e `prefer` são HTTP simples. `require` não verifica o certificado;
  `verify-ca` e `verify-full` verificam. Um arquivo de CA e um certificado de cliente vão nos campos
  de TLS.

## Privilégios

Os que as requisições exigem. O navegador lê os índices, aliases, data streams e mappings que o login
consegue ver; não há abas de administração.

## O que funciona

- **Requisições como o console Dev Tools do Kibana as escreve**: uma linha como
  `GET /orders/_search` (a barra é opcional; métodos `GET`, `POST`, `PUT`, `DELETE`, `HEAD`), depois
  o corpo JSON. `_bulk` e `_msearch` aceitam corpos NDJSON que terminam em uma linha em branco ou na
  próxima requisição.
- **SQL**: qualquer outra coisa é SQL (`_sql` no Elasticsearch, `_plugins/_sql` no OpenSearch), com
  colunas tipadas e o cursor do servidor para resultados longos. Os erros mantêm o motivo do
  servidor, e um erro de SQL aponta o lugar dele na instrução.
- Uma busca devolve documentos como os resultados do MongoDB: uma coluna para cada campo de nível
  superior, com o documento em foco como uma árvore (`F7`). As agregações vêm como resultados à
  parte.
- O navegador lista índices como tabelas (saúde, shards e réplicas na dica) e aliases e data streams
  como views. Um índice abre como uma busca de todos os documentos dele, lida 1.000 linhas por vez
  com um point in time e `search_after` (um scroll no OpenSearch), então a leitura não para nos
  10.000 hits. Um campo sem coluna mapeada vai para uma última coluna `_other` como JSON.
- **A grade edita documentos**: uma célula alterada é um `_update` por `_id`, uma nova linha é um
  `_create` (um `_id` que existe é recusado pelo servidor) ou um documento gerado, uma linha excluída
  é um `DELETE`. A visualização mostra as requisições, que rodam uma após a outra; uma que o servidor
  recusa interrompe as demais, e as anteriores permanecem. O *Script para desfazer* devolve o
  `_source` antigo (um documento com `_id` gerado precisa ser excluído à mão).
- `_delete_by_query` e `_update_by_query` sem uma query (ou `match_all`), e `DELETE /_all`, perguntam
  antes como um `DELETE` sem `WHERE`. Conexões somente leitura executam apenas as requisições e o
  SQL que leem.
- Cancelar interrompe uma busca no servidor. O DDL de objeto de um índice é a requisição que o cria
  de novo.
- Os detalhes estão em [FEATURES.md](../../../../FEATURES.md#elasticsearch-and-opensearch).

## O que não funciona

- Sem transações (sem commit manual) e sem explain.
- Sem ferramentas SQL: sem editor de tabelas, diagrama ER, comparação de schemas, query builder,
  importação de SQL, snapshots antes de alterações nem modo terminal.
- O visualizador de dados não tem barra de filtro e ordenação.

## Agentes

`run_query` e `sample_rows` funcionam com requisições do console e SQL, e uma requisição que altera
dados é perguntada antes. Recusados no Elasticsearch e no OpenSearch: `explain`, `get_er_diagram`,
`propose_edits` e `query_across_databases`. Veja [Agentes e a CLI](../../../../AGENTS.md).

## Limites conhecidos

- O app recusa o Elasticsearch anterior ao 7.10 e o OpenSearch anterior ao 1.0: ao contrário dos
  outros bancos de dados, a conexão falha com "older than … the oldest version supported".
- Testado no Elasticsearch 8 e no OpenSearch 2, não no 7.x.

## Testado em

A suíte de conformidade roda no Elasticsearch 8.15.3 (login `elastic`) e no OpenSearch 2.17.1:
informações do servidor, SQL com tipos e paginação, buscas que passam da primeira página, o catálogo
e os mappings, salvamento das alterações da grade e desfazer, cancelamento de uma busca lenta, e os
erros do servidor.

Veja também: [Conectar](../connect.md), [Solução de problemas](../troubleshooting.md).
