<!-- source: 728e6511c5f7 -->
[← Bancos de dados](README.md)

# DuckDB

O DuckDB roda dentro do app, como o SQLite: sem servidor, host, usuário nem senha. Ao contrário do
SQLite, o app não carrega a biblioteca do DuckDB; ele **a baixa na primeira vez** que um banco de
dados ou um arquivo de dados do DuckDB é aberto.

## Primeiro uso: a biblioteca do DuckDB

Quando você escolhe **DuckDB** para uma nova conexão e a biblioteca não está neste computador, um
popup se abre sobre o formulário: *Baixar o DuckDB?* Ele mostra o pacote (DuckDB 2.0.0, da release do
DuckDB no GitHub, licença MIT), o tamanho e a origem, e verifica o download com um SHA-256.
**Baixar** a coloca na pasta de dados do app, uma vez para todos os bancos de dados e arquivos do
DuckDB; **Cancelar** deixa o formulário utilizável, e a oferta volta em *Testar conexão* e na
primeira abertura. *Configurações › Drivers* lista a biblioteca e a remove.

O app a baixa para Windows (x64, ARM64), Linux (x64, ARM64) e macOS (uma biblioteca para os dois
processadores). O app usa exatamente a versão com que foi compilado (2.0.x) e oferece essa se outra
estiver instalada. Offline, com a biblioteca ausente, a conexão não consegue abrir.

## Conectar

Escolha **DuckDB** e informe o caminho do arquivo do banco de dados, ou cole uma URL:

| Você cola | Significado |
|---|---|
| `duckdb:///home/ana/analytics.duckdb` | esse arquivo, criado se não existir (a pasta dele deve existir) |
| `duckdb:///C:/data/analytics.duckdb` | um arquivo em uma unidade do Windows |
| `duckdb:///:memory:` | o banco de dados em memória que as sessões do app compartilham |
| `:memory:<name>` como caminho | um banco de dados em memória próprio, com esse nome |

**Arquivos de dados como bancos de dados.** Um caminho que termina em `.csv`, `.tsv`, `.json`,
`.ndjson`, `.jsonl`, `.parquet`, `.xlsx`, `.xlsm`, `.xls` ou `.ods` abre como um banco de dados em
memória sobre esse arquivo (você também pode soltar um arquivo na janela). CSV, TSV, JSON, NDJSON e
Parquet viram uma view com o nome do arquivo, que o DuckDB lê de novo a cada consulta; cada planilha
de uma pasta de trabalho vira uma tabela, carregada uma vez. Um CSV ou TSV é verificado antes (cada
linha deve ter tantos valores quanto o cabeçalho), a codificação do texto é detectada (ou você a
informa), e as opções de importação permitem pular linhas ruins.

As extensões não são baixadas nem carregadas por conta própria; só existem as embutidas na
biblioteca (Parquet, JSON).

## Privilégios

Os do sistema operacional: acesso de leitura ao arquivo, e de gravação nele e na pasta dele para
alterar qualquer coisa. Um processo por vez pode abrir um arquivo do DuckDB para gravação, então
enquanto o app mantém um arquivo aberto outro programa (a linha de comando `duckdb`, por exemplo)
não pode usá-lo; as sessões do app em um arquivo compartilham uma instância. Por isso o modo
terminal não é oferecido.

## O que funciona

- Edição de dados na grade, por chave primária (ou uma chave única em colunas não nulas, ou uma
  chave virtual). Um arquivo de dados é lido para a memória: uma view de CSV, JSON ou Parquet não é
  uma tabela e não pode ser editada, e as edições nas planilhas de uma pasta de trabalho não são
  gravadas de volta no arquivo.
- Commit e rollback manuais; cancelamento de uma instrução (`duckdb_interrupt`).
- O Explain mostra o plano (`EXPLAIN (FORMAT JSON)`); **Explain analyze** executa a instrução em uma
  transação que ele desfaz, e é recusado enquanto você tem uma transação aberta.
- Aba Parâmetros: as configurações do DuckDB. Sem abas de sessões, locks, painel, usuários ou
  instruções.
- Busca de metadados, DDL de objetos, ferramentas de schema. O DuckDB altera pouca coisa no lugar,
  então mudar uma tabela a cria de novo e copia as linhas dela.
- As importações passam pelo appender do DuckDB, na transação da sessão.

## Agentes

Um agente somente leitura, e uma conexão somente leitura, abrem o arquivo do banco de dados somente
para leitura, quando nenhuma outra sessão do app já o mantém aberto; um banco de dados em memória
ou um arquivo de dados nunca é somente leitura, e a verificação da instrução impede gravações ali. As
sessões do DuckDB dos agentes não podem ler nem gravar outros arquivos nem carregar extensões, e
fecham o arquivo depois de cada chamada para que o app possa abri-lo. `query_across_databases`
roda em um DuckDB próprio, que o app baixa como este. Veja [Agentes e a CLI](../../../../AGENTS.md).

## Limites conhecidos

- Os arquivos Excel são lidos com o leitor próprio do app, não com a extensão Excel do DuckDB.
- Uma versão do DuckDB diferente da 2.0.x não é carregada.

## Testado em

A suíte de conformidade roda em um arquivo do DuckDB, com a biblioteca que corresponde aos bindings
do app: transações, planos, edição de tabelas, valores especiais, números de 128 bits e sem sinal, e
horas com fuso.

Veja também: [Conectar](../connect.md), [Solução de problemas](../troubleshooting.md).
