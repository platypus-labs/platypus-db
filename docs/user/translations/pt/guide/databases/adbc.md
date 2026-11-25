<!-- source: 6fd0f54f48e3 -->
[← Bancos de dados](README.md)

# Snowflake, BigQuery, Databricks, Trino e outros drivers ADBC

Esses bancos de dados são alcançados por drivers **ADBC** (Arrow Database Connectivity), bibliotecas
que o app carrega quando você conecta. Snowflake, BigQuery, Databricks e Trino têm um tile cada;
qualquer outro banco de dados com um driver ADBC conecta por **Outro driver ADBC**.

## Primeiro uso: o download do driver

Os quatro bancos de dados citados precisam do driver deles neste computador. Quando você escolhe um
para uma nova conexão e o driver não está instalado, um popup se abre sobre o formulário: *Snowflake
precisa de um driver* (e assim por diante). Ele diz que é um driver ADBC, baixado uma vez para todas
as conexões desse banco de dados, e mostra o pacote, a versão, a licença e de onde vem: o
[registry de drivers dbc](https://docs.columnar.tech/dbc/) (Columnar). **Baixar** o obtém, verifica
o SHA-256 dele (um pacote sem um é recusado) e o guarda com os dados do app; **Cancelar** deixa o
formulário utilizável, e a oferta volta em *Testar conexão* e na primeira abertura.

A oferta lê só o que está neste computador, então aparece também offline; o download precisa de
rede. *Configurações › Drivers* lista os drivers instalados e os remove, procura versões mais novas
uma vez por dia (ou em *Procurar atualizações*), instala ou volta a outra versão com *Versões…*, e
pode instalar atualizações por conta própria. Depois de uma mudança, as conexões salvas que usam o
driver são testadas (conectar, versão, schemas, `SELECT 1`), com *Voltar* se uma falhar. A escolha
de outro driver não baixa nada: você a aponta para um arquivo de biblioteca.

## Conectar

Cada banco de dados tem os próprios campos:

| Banco de dados | Campos |
|---|---|
| Snowflake | **Conta** (por exemplo `xy12345.eu-west-1`), usuário, senha, warehouse, papel, banco de dados, schema |
| BigQuery | **Projeto**, dataset, e um arquivo de chave de conta de serviço (JSON); vazio significa as Application Default Credentials do Google. Sem senha |
| Databricks | **Nome do host do servidor**, **Caminho HTTP**, catálogo, schema; o campo de senha é o **token de acesso** |
| Trino | **Host**, porta (8080 por padrão), **usuário**, catálogo, schema, senha (opcional) e TLS |
| Outro driver ADBC | o arquivo da **biblioteca do driver**, um símbolo de **ponto de entrada** opcional, uma URI, usuário e senha; toda outra opção que você adicionar é passada ao driver |

As opções de **TLS do Trino** são *Automático* (HTTPS na porta 443, senão HTTP), *Desativado* (HTTP),
*On, verify certificate* e *On, unverified*.

**Outro driver ADBC**: a biblioteca é o arquivo do driver que você baixou ou compilou. O ponto de
entrada só é necessário para uma biblioteca que não exporta `AdbcDriverInit` (para a biblioteca ADBC
do DuckDB, é `duckdb_adbc_init`). A URI e as opções são o que esse driver documentar. Um arquivo
ausente, ou que não é um driver, é relatado quando você conecta.

## Privilégios

O que a conta ou o token tiver no serviço. O navegador lê os catálogos, schemas, tabelas e colunas
com a chamada de metadados do ADBC.

## O que funciona

- Consultas, resultados, o navegador e o autocompletar. O resultado chega em streaming, em batches
  Arrow.
- Cancelar: o app pede ao driver que cancele; drivers escritos em Go só cancelam as próprias
  chamadas, então uma instrução que não para é abandonada junto com a sessão dela, e o app abre
  uma nova.
- DDL de objetos só no **Trino** (`SHOW CREATE`). Snowflake, BigQuery, Databricks e outros drivers
  não mostram DDL.
- As chaves estrangeiras que apontam para uma tabela não são listadas (seria preciso ler as chaves
  de todas as tabelas).
- Exportações, e importações de arquivos (as importações tipadas são testadas no Trino).

## O que não funciona

- **Sem commit manual**: cada instrução faz commit por si só, e desativar o autocommit diz que o
  commit manual com um driver ADBC não é suportado.
- **Sem explain**, e sem abas de administração.
- **Sem edição de dados na grade**: um salvamento verifica cada alteração em uma transação, e essas
  conexões não têm uma. Os resultados continuam somente leitura; use SQL para alterar.
- O modo terminal não está disponível.

## Agentes

`run_query`, `sample_rows` e as outras ferramentas funcionam, dentro das verificações. Sem uma sessão
somente leitura no servidor, os agentes somente leitura dependem só da verificação da instrução. As
instruções que agem fora dela são perguntadas antes de executar: o `query()` do Trino e as funções
`SYSTEM$` do Snowflake. Veja [Agentes e a CLI](../../AGENTS.md).

## Limites conhecidos

- Um login de data warehouse em nuvem diferente dos campos acima (par de chaves, single sign-on) não
  está no formulário; o driver genérico aceita as opções do próprio driver.
- Snowflake, BigQuery e Databricks estão no código e compartilham a camada de drivers com o Trino,
  mas não têm teste contra os serviços reais.

## Testado em

A suíte de conformidade roda o **Trino 484** pelo driver baixado dele (tipos, resultados longos,
cancelamento, metadados, erros, instruções que terminam com um terminador, importações tipadas, e
linhas e UUIDs) e o **driver ADBC genérico** no driver ADBC do Apache SQLite e no do DuckDB. Snowflake,
BigQuery e Databricks não são testados.

Veja também: [Conectar](../connect.md), [Solução de problemas](../troubleshooting.md).
