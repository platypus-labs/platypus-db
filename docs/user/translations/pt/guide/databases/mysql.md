<!-- source: 6f25d7181e09 -->
[← Bancos de dados](README.md)

# MySQL e MariaDB

Um só driver (o cliente `mysql_async`) abre o MySQL 5.7 e posteriores e o MariaDB 10.6 e
posteriores. Os dois têm um tile na grade de bancos de dados; o driver identifica a qual servidor
chegou e o mostra na conexão (`Connected: MariaDB 11.4`). Nada é baixado.

## Conectar

| Você cola | Lido como |
|---|---|
| `mysql://user:pass@host:3306/db?sslMode=REQUIRED` (ou `mariadb://`) | a forma de URL; porta padrão 3306 |
| `jdbc:mysql://host:3307/db`, `jdbc:mariadb://…` | uma URL JDBC |
| `Server=db;Port=3306;Database=shop;Uid=app;Pwd=…` | uma connection string do Connector/NET, se tiver também `Driver=` ou se você a colou em um formulário do MySQL |

Parâmetros de URL que o app conhece: `sslmode` (também escrito `ssl-mode`, `sslMode`, `ssl_mode`),
`sslrootcert`, `sslcert`, `sslkey`, `application_name` e `connect_timeout`.

**TLS**: `disable` o desativa. `prefer` (o padrão) usa TLS e volta para sem criptografia quando o
servidor não tem ou só oferece versões que o cliente recusa (as imagens do MySQL 5.7 oferecem); ele
nunca volta atrás quando o login é por token. `prefer` e `require` não verificam o certificado.
`verify-ca` verifica a cadeia contra `sslrootcert` e não o nome do host; `verify-full` verifica os
dois. Os nomes do próprio MySQL (`REQUIRED`, `VERIFY_IDENTITY`) são entendidos. Um certificado de
cliente exige `sslcert` e `sslkey`.

**O login** é por senha, ou por um token do AWS IAM, do Google Cloud IAM ou do Microsoft Entra ID
gerado pela ferramenta `aws`, `gcloud` ou `az`; o token vai por TLS como o servidor pede. O
Kerberos (o `auth_gssapi` do MariaDB, o plugin Kerberos do MySQL) ainda não é suportado.

A conexão é sempre TCP: um socket Unix ou um named pipe do Windows não é usado.

## Privilégios

Navegar exige o que o login consegue ver. As abas de administração leem:

- **Sessões**: a lista de processos, de `performance_schema.threads` no MySQL 8 e de
  `information_schema.PROCESSLIST` no MariaDB, no MySQL 5.7 ou em um servidor com
  `performance_schema` desativado. Quanto das sessões de outros usuários o servidor mostra é regra
  do servidor, que o app não verifica.
- **Locks**: `performance_schema.data_lock_waits`, `data_locks` e `metadata_locks` no MySQL 8 (ver as
  esperas de outras sessões exige `PROCESS`; sem ele a lista não tem esperas e diz isso),
  `INNODB_LOCK_WAITS` em `information_schema` no MariaDB e no MySQL mais antigo.
- **Instruções mais custosas**: `performance_schema.events_statements_summary_by_digest`. Se o
  `performance_schema` estiver desativado, a aba diz para iniciar o servidor com ele.
- **Tabelas e índices**: `information_schema`, mais `performance_schema` para saber quanto cada
  índice é lido; sem ele, o uso dos índices é desconhecido.
- **Cancelar instrução** e **Encerrar sessão** executam `KILL QUERY` e `KILL CONNECTION`.
- **Parâmetros** e **Painel**: variáveis do servidor e contadores de status.
- **Usuários e papéis**: as contas e os grants do servidor.
- **Manutenção**: `ANALYZE`, `OPTIMIZE` e `CHECK TABLE`.
- **Backup… e Restaurar…** executam `mysqldump` e `mysql`, que precisam estar no PATH.

## O que funciona

- Edição de dados na grade, por chave primária (ou uma chave única em colunas não nulas, ou uma
  chave virtual).
- Commit e rollback manuais, e cancelamento de uma instrução no servidor.
- Explain: o MySQL 8.0.16 e posteriores dão `EXPLAIN FORMAT=TREE`; o MySQL mais antigo e o MariaDB
  dão `EXPLAIN FORMAT=JSON`. **Explain analyze** é `EXPLAIN ANALYZE` no MySQL 8.0.18 e posteriores e
  `ANALYZE FORMAT=JSON` no MariaDB; o MySQL 5.7 e o 8.0.0 a 8.0.17 o recusam.
- Todas as abas de administração acima, e as ferramentas de schema (editor de tabelas, diagrama ER,
  comparação).
- As importações usam `LOAD DATA LOCAL` quando o servidor permite (`local_infile` ativado), senão
  instruções `INSERT`.
- Os erros citam o servidor de onde vêm, p. ex. `Unknown column 'x' in 'field list' (MySQL 5.7.44)`.

## Agentes

Agentes somente leitura e conexões somente leitura executam `SET SESSION TRANSACTION READ ONLY`, e as
consultas deles rodam em uma transação somente leitura. `server_activity` e `top_queries`
funcionam. Uma `@variable`, uma tabela temporária ou um `SET` do MySQL ou MariaDB feito por uma
chamada de agente continua lá na seguinte. Veja [Agentes e a CLI](../../AGENTS.md).

## Limites conhecidos

- Anterior ao MySQL 5.7 ou ao MariaDB 10.6: ele conecta, com um aviso de que alguns recursos podem
  não funcionar.
- O editor de tabelas salva com o DDL do MySQL, que faz commit a cada instrução: um script que
  falha pode deixar parte dele aplicada.
- O modo terminal exige o `mysql` no PATH.

## Testado em

A suíte de conformidade roda no MySQL 5.7, 8.0 e 8.4 e no MariaDB 10.6 e 11.4, com um certificado de
cliente sobre TLS mútuo no MySQL 8.4, e com sessões que o servidor derrubou.

Veja também: [Conectar](../connect.md), [Solução de problemas](../troubleshooting.md).
