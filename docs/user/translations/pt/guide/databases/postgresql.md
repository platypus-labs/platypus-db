<!-- source: a2b0bfe9784d -->
[← Bancos de dados](README.md)

# PostgreSQL, CockroachDB, YugabyteDB, TimescaleDB e os demais

Um só driver, construído sobre o `tokio-postgres`, fala o protocolo do PostgreSQL. Ele abre o
PostgreSQL 12 e posteriores, e os servidores e serviços que falam o mesmo protocolo. Nada é baixado:
o driver faz parte do app.

## Conectar

Escolha **PostgreSQL** na grade de bancos de dados, ou cole uma URL: o formulário se preenche sozinho.

| Você cola | Lido como |
|---|---|
| `postgres://user:pass@host:5432/db?sslmode=require` (ou `postgresql://`) | a forma de URL; a porta padrão é 5432 |
| `jdbc:postgresql://host/db?sslmode=require` | uma URL JDBC, como o DBeaver e o DataGrip as guardam |
| `host=db port=5432 dbname=shop user=app sslmode=require` | uma connection string da libpq |
| `Host=db;Database=shop;Username=app;Password=…` | uma connection string do Npgsql |

Parâmetros de URL que o app conhece: `sslmode`, `sslrootcert`, `sslcert`, `sslkey`,
`application_name`, `connect_timeout` e `pgbouncer=true`. Qualquer outro parâmetro é deixado de
fora, e o formulário diz qual. Uma lista de hosts para failover (`h1:5432,h2:5432`) mantém o
primeiro host.

O **TLS** segue o `sslmode` da libpq:

| Modo | O que acontece |
|---|---|
| `disable` | sem TLS |
| `prefer` (o padrão) | TLS se o servidor oferecer, senão sem criptografia; o certificado não é verificado |
| `require` | TLS, certificado não verificado |
| `verify-ca` | TLS, a cadeia de certificados é verificada, o nome do host não |
| `verify-full` | TLS, a cadeia e o nome do host são verificados |

A cadeia é verificada contra o arquivo em `sslrootcert`, se houver, senão contra os certificados
confiáveis do sistema. Um certificado de cliente (`sslcert` e `sslkey`) é enviado em todo modo que
usa TLS.

**O login** é por senha, ou por um token que a ferramenta de linha de comando de uma nuvem gera a
cada conexão: AWS IAM (`aws`), Google Cloud IAM (`gcloud`) e Microsoft Entra ID (`az`) estão na
escolha *Autenticação* da conexão. A ferramenta precisa estar instalada e com a sessão iniciada no
seu computador. Ou por **Kerberos**, abaixo.

**Kerberos (GSSAPI).** Escolha *Ticket Kerberos (GSSAPI)* em *Senha*. O campo de senha some: nenhuma
senha é pedida, salva ou enviada. Se o servidor pedir uma, ou deixar você entrar sem pedir
Kerberos, a conexão é recusada e diz isso.

- **O nome do serviço** é `postgres`, a menos que você o preencha (o `krb_srvname` do servidor). O
  ticket pedido é `service/host`, no seu realm padrão: o host deve ser o nome que está no principal
  do serviço, não um endereço IP nem um alias, pois o app não o resolve. Por um túnel SSH ou um port
  forward, continua sendo o nome do servidor.
- **O usuário** é o papel do PostgreSQL, que o servidor mapeia a partir do seu principal
  (`include_realm` e `pg_ident.conf`). Se ficar vazio, o app usa o nome do seu usuário do sistema.
- **O ticket** no Linux vem do `kinit`, no cache que `KRB5CCNAME` indica (ou no padrão), ou de um
  keytab: com `KRB5_CLIENT_KTNAME` definida, o Kerberos obtém o ticket sozinho. Essas variáveis
  precisam estar definidas onde o app inicia. No Windows é o ticket da sua sessão do Windows.
- **Sem um ticket** o teste diz o que executar: `kinit`, as duas variáveis e `klist`, seguidos do
  texto do próprio Kerberos. O mesmo vale para um ticket expirado, um serviço desconhecido e um KDC
  inalcançável.
- **`psql`, `pg_dump` e `pg_restore`** entram do mesmo modo (`PGKRBSRVNAME`, `PGREQUIREAUTH=gss` e
  nenhum `PGPASSWORD`). A cópia deles da libpq precisa ter sido compilada com GSSAPI.
- **Quais builds o têm.** Os pacotes do Windows (SSPI) e do Linux (GSSAPI, MIT Kerberos). Os do
  macOS não: nenhum Mac o compilou ainda, e o formulário diz isso. O recurso Cargo `kerberos` do
  `platypus-driver-pg` o ativa; sem ele, nada do Kerberos é ligado ou executado.
- **A criptografia GSS** (`hostgssenc`, `gssencmode`) não é usada: use TLS para criptografar, com
  linhas `hostssl` ou `host`. Um `krbsrvname` em uma URL colada é deixado de fora, como qualquer
  outro parâmetro que o app não conhece.

**Poolers.** Atrás do PgBouncer, do Supavisor ou do pooler do Neon em modo de transação, escolha
*Transaction mode* no formulário (ou adicione `pgbouncer=true` à URL). As instruções então rodam sem
nome, porque a próxima transação pode estar em outra sessão do servidor. O formulário sugere isso
para hosts terminados em `.pooler.supabase.com` e para hosts do Neon com `-pooler` no nome. Nesse
modo o depurador PL/pgSQL fica desativado, e as sessões somente leitura dos agentes dependem só da
verificação da instrução.

Os outros servidores têm tiles próprios, que só definem padrões:

| Tile | Padrões que ele define |
|---|---|
| CockroachDB | porta 26257 |
| YugabyteDB | porta 5433, usuário e banco de dados `yugabyte` |
| TimescaleDB | nenhum |
| Supabase | usuário e banco de dados `postgres`, TLS `require` |
| Neon, AlloyDB | TLS `require` |

## Privilégios

Navegar exige só o que o servidor mostra ao login. As abas de administração leem estes:

- **Sessões** e **Locks**: `pg_stat_activity` e `pg_locks`. O app lista as sessões de clientes que
  o servidor devolve; quanto das sessões e instruções de outros usuários o servidor mostra ao login
  é regra do servidor, que o app não verifica. *Cancelar instrução* e *Encerrar sessão* chamam
  `pg_cancel_backend` e `pg_terminate_backend`; o servidor decide se o login pode.
- **Instruções mais custosas**: a extensão `pg_stat_statements`. Sem ela, a aba diz como ativá-la
  (adicioná-la a `shared_preload_libraries`, reiniciar, `CREATE EXTENSION`).
- **Tabelas e índices**: `pg_stat_user_tables` e `pg_stat_user_indexes`.
- **Parâmetros** e **Painel**: `pg_settings`, `pg_stat_database`, `pg_stat_activity` e as views de
  replicação; os standbys só são listados para um membro de `pg_monitor`. Um número que o login não
  pode ler mostra "—" e o motivo.
- **Usuários e papéis**: `pg_roles` e `pg_auth_members`; alterá-los exige os direitos que as
  instruções exigem.
- **Manutenção**: `VACUUM`, `ANALYZE` e `REINDEX` na tabela.
- **Backup… e Restaurar…** executam `pg_dump`, `pg_restore` e `psql`, que precisam estar no PATH.
- **Depurar…** exige a extensão `pldebugger` no servidor.

## O que funciona

- Edição de dados na grade, por chave primária (ou uma chave única em colunas não nulas, ou uma
  chave virtual que você define). Arrays e valores compostos editam um elemento ou campo por linha.
- Transações com commit manual, e instruções canceladas no servidor.
- Explain e Explain analyze; o plano é lido do JSON ou do texto do PostgreSQL.
- Vários bancos de dados por conexão, partições, geometrias PostGIS, arrays e tipos compostos.
- As importações e cópias para o PostgreSQL usam `COPY`.
- Todas as abas de administração acima, o depurador PL/pgSQL e as ferramentas de schema (editor de
  tabelas, diagrama ER, comparação).

**CockroachDB e YugabyteDB** abrem com o mesmo driver, mas o app descobre qual servidor é, e as
ferramentas de servidor que dependem das views do próprio PostgreSQL ficam desativadas: nada de
abas de sessões, locks, painel, instruções, tabelas e índices, parâmetros, usuários ou manutenção, e
nada de depurador. O CockroachDB tem o próprio formato de plano (Explain e Explain analyze
funcionam) e as importações vão como instruções `INSERT` em vez de `COPY`. Uma tabela do CockroachDB
sem chave é editada pelo seu `rowid` oculto. TimescaleDB (testado), Supabase, Neon e AlloyDB (não
testados contra os serviços) são PostgreSQL para o app, com tudo o que está acima; o que um serviço
hospedado restringe para o seu papel é regra dele.

## Agentes

Agentes somente leitura e conexões somente leitura abrem a sessão com
`default_transaction_read_only` (PostgreSQL, CockroachDB e YugabyteDB), então o próprio servidor
recusa gravações; atrás de um pooler em modo de transação, só a verificação da instrução o faz.
`server_activity` e `top_queries` funcionam no PostgreSQL propriamente dito, não no CockroachDB nem
no YugabyteDB. Veja [Agentes e a CLI](../../AGENTS.md).

## Limites conhecidos

- Anterior ao 12: o app conecta e avisa que alguns recursos podem não funcionar.
- O depurador fica desativado atrás de um pooler em modo de transação.
- O modo terminal exige o `psql` no PATH.

## Testado em

A suíte de conformidade roda no PostgreSQL 12, 14, 16 e 17, no TimescaleDB 2.30 sobre o PostgreSQL
17, no CockroachDB 26.2 e no YugabyteDB 2025.2, além do PgBouncer em modo de transação, de um
certificado de cliente sobre TLS mútuo e do depurador em um servidor com `pldebugger`. Supabase, Neon
e AlloyDB não são testados: são o driver do PostgreSQL com padrões.

O Kerberos é testado com um ticket real de um KDC real (MIT Kerberos, no perfil `kerberos` do
`docker/compose.yml`) no PostgreSQL 17, a partir de um cliente Linux com GSSAPI: um ticket do
`kinit`, um keytab, TLS, `psql`, sem ticket, um ticket expirado e um nome de serviço errado. **O
login pelo SSPI do Windows não foi tentado com um ticket real:** um computador Windows fora de um
domínio não alcança o KDC de teste sem mudar configurações do sistema (`ksetup`). No Windows, só o
que acontece sem um ticket é testado. Uma linha `sspi` do servidor (PostgreSQL no Windows) também
não foi tentada.

Veja também: [Conectar](../connect.md), [Solução de problemas](../troubleshooting.md).
