<!-- source: 641268954234 -->
[← Bancos de dados](README.md)

# Apache Cassandra e ScyllaDB

Um só driver, construído sobre o crate `scylla` (com rustls), fala CQL pelo protocolo nativo com os
dois. O servidor diz qual é, e a conexão o mostra. O driver faz parte do app; nada é baixado. O
editor aceita CQL.

## Conectar

| Você cola | Lido como |
|---|---|
| `cassandra://cassandra:s%40cret@node1:9043/shop` | simples, porta 9043, keyspace `shop` |
| `scylla://node1` | o mesmo driver, para o ScyllaDB |
| `cassandras://…` e `scyllas://…` | com TLS (verificado: `verify-full`) |

A porta padrão é 9042. O campo **Host** é um contact point, ou vários pares `host:port` separados
por vírgulas. O campo **Keyspace (opcional)** é o keyspace em que a sessão começa; o navegador lista
todos de qualquer modo.

- **Um só contact point, ou um túnel**: esse nó sozinho atende toda instrução. Um cluster anuncia os
  endereços próprios dos seus nós, que um contêiner, um NAT ou um túnel escondem, então o driver não
  os segue. Com vários contact points, o driver recebe todos e pode usar qualquer um.
- **Login**: um usuário e senha, para o autenticador de senha do servidor. Sem usuário, a conexão é
  anônima.
- **TLS**: o CQL não consegue negociá-lo, então o TLS está ativado para URLs `cassandras://` e
  `scyllas://` e para modos SSL de `require` para cima; `prefer` é simples. `require` não verifica o
  certificado; `verify-ca` e `verify-full` verificam. Um arquivo de CA e um certificado de cliente
  vão nos campos de TLS.
- **As instruções não têm timeout** próprio do driver: uma roda até terminar ou até você cancelar.

## Privilégios

O navegador lê o `system_schema`, que o Cassandra 3 e posteriores e o ScyllaDB compartilham; o login
vê o que o servidor permite que ele leia ali. Não há abas de administração. As gravações exigem as
permissões que o CQL pede.

## O que funciona

- Scripts CQL, divididos pelo editor (strings `$$ … $$`, comentários `//` e um `BEGIN BATCH … APPLY
  BATCH` são entendidos), com resultados tipados lidos página por página enquanto você rola, usando
  o estado de paginação do servidor.
- O navegador: keyspaces como schemas, tabelas, views materializadas, tipos definidos pelo usuário,
  funções e agregados, com as chaves de partição e de clustering de cada tabela e os índices dela; o
  DDL de cada objeto.
- **A grade edita linhas**, pela chave primária completa (colunas de partição e de clustering): uma
  linha editada é um `UPDATE`, uma nova linha um `INSERT` que exige todas as colunas de chave, uma
  linha excluída um `DELETE`. A visualização mostra as instruções, que rodam uma após a outra; uma
  que o servidor recusa interrompe as demais. O *Script para desfazer* devolve os valores antigos.
  As colunas de chave não podem ser alteradas (exclua a linha e insira outra), e contadores são
  recusados.
- Transações leves (`IF …`) são relatadas como tais, com o resultado.
- Cancelar interrompe a leitura de um resultado. Os erros mantêm a mensagem do servidor.
- `USE keyspace` muda o keyspace da sessão, que o driver mantém.

## O que não funciona

- **Sem transações**: cada instrução tem efeito por si só, e não há commit manual.
- **As gravações CQL são upserts e não relatam contagem de linhas**, então uma instrução que não
  devolve linhas não relata nenhuma alterada; um update de uma linha que alguém excluiu nesse meio
  tempo a grava de novo.
- O visualizador de dados não tem barra de filtro e ordenação, e não mostra contagem de linhas.
- Sem explain, e sem ferramentas SQL: sem editor de tabelas, diagrama ER, comparação de schemas,
  query builder, importação de SQL, snapshots antes de alterações nem modo terminal (o `cqlsh` só
  aceita a senha na linha de comando).

## Agentes

`run_query` e `sample_rows` (um `SELECT … LIMIT n` em CQL) funcionam; uma instrução que altera
dados é perguntada antes. Recusados no Cassandra: `explain` e `propose_edits`; `estimate_change`
responde com uma nota. Veja [Agentes e a CLI](../../AGENTS.md).

## Limites conhecidos

- O Cassandra anterior ao 4.0 e o ScyllaDB anterior ao 5.4 são recusados na conexão, com "older
  than … the oldest version supported", ao contrário dos outros bancos de dados, que conectam com um
  aviso.
- Sem joins, sem `OFFSET`, e as linhas só são encontradas pela chave primária, como o CQL as tem.

## Testado em

A suíte de conformidade roda no Apache Cassandra 5 e no ScyllaDB 2025.1, sem login: informações do
servidor, scripts, todo tipo como coluna tipada, paginação, transações leves, erros, cancelamento, o
catálogo e o DDL dele (também funções e agregados), salvamento das alterações da grade e desfazer, um
login errado ou ausente sendo recusado, e um endereço de túnel. O Cassandra 4.0 e o ScyllaDB 5.4 são
as versões mais baixas que o driver aceita e não são executadas pela suíte.

Veja também: [Conectar](../connect.md), [Solução de problemas](../troubleshooting.md).
