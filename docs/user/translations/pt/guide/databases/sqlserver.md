<!-- source: 3830a0c639a7 -->
[← Bancos de dados](README.md)

# SQL Server e Azure SQL

O driver fala TDS por conta própria (um `tiberius` com patches), então não há nada para instalar nem
baixar. Ele foi escrito para o SQL Server 2017 (14.0) e posteriores, e para o Azure SQL (não testado
contra o serviço: veja o fim desta página).

## Conectar

| Você cola | Lido como |
|---|---|
| `sqlserver://sa:pass@host:1433/master?encrypt=true&trustServerCertificate=true` (ou `mssql://`) | a forma de URL; porta padrão 1433 |
| `jdbc:sqlserver://host:1433;databaseName=shop;encrypt=true` | uma URL JDBC (driver da Microsoft, jTDS ou o antigo `jdbc:microsoft:`) |
| `Server=tcp:db.example.com,1433;Database=shop;User Id=sa;Password=…;Encrypt=True` | uma connection string ADO.NET ou ODBC |

Em uma URL, `database` ou `databaseName` indica o banco de dados, e `encrypt` e
`trustServerCertificate` são lidos como as strings JDBC e ADO.NET os leem (a menos que `sslmode`
também seja informado).

**Instâncias nomeadas** (`host\SQLEXPRESS`) não são procuradas pelo SQL Server Browser: informe a
porta da instância. Uma string colada com uma instância e sem porta diz isso.

**TLS**: `disable` desativa a criptografia. `prefer` (o padrão) criptografa e não verifica o
certificado (os servidores costumam ter um autoassinado), e volta para sem criptografia se o
servidor não tem criptografia. `require` criptografa sem verificar. `verify-ca` e `verify-full`
verificam a cadeia **e** o nome do host, porque a biblioteca TLS sempre verifica o nome; aponte
`sslrootcert` para o arquivo da sua CA, ou os certificados confiáveis do sistema são usados.

**O login**:

- *Login do SQL Server*: usuário e senha.
- *Login do Windows* (`Integrated Security=SSPI` em uma string colada): só quando o PlatypusDB roda
  no Windows; em outros sistemas é um erro.
- *Microsoft Entra ID*: um token gerado pela ferramenta `az`, que precisa estar instalada e com a
  sessão iniciada. É assim que se chega ao login Entra do Azure SQL.

**Azure SQL**: o gateway pode enviar o cliente ao nó do próprio banco de dados; o app segue um desses
redirecionamentos, exceto por um túnel SSH, que leva a um só endereço.

## Privilégios

Navegar exige o que o login consegue ver. As abas de administração leem:

- **Sessões** e **Locks**: `sys.dm_exec_sessions`, `dm_exec_requests`, `dm_exec_connections` e
  `dm_tran_locks`. Para ver o que outros logins fazem, o login precisa de `VIEW SERVER STATE`.
- **Instruções mais custosas**: `sys.dm_exec_query_stats`, o cache de planos.
- **Tabelas e índices**: `sys.tables`, `dm_db_partition_stats`, `dm_db_index_usage_stats` e
  `sys.stats`.
- **Parâmetros**: `sys.configurations`; *Abrir no editor* escreve uma instrução `sp_configure`.
- **Painel**: contadores de desempenho, tamanhos de `sys.databases` e `sys.master_files`, e grupos
  de disponibilidade onde houver; um número que o login não pode ler mostra "—".
- **Usuários e papéis**: os logins, papéis e grants do servidor.
- **Manutenção**: `UPDATE STATISTICS`, `ALTER INDEX ALL … REBUILD` e `DBCC CHECKTABLE`.

## O que funciona

- Edição de dados na grade, por chave primária (ou uma chave única em colunas não nulas, ou uma
  chave virtual).
- Commit e rollback manuais; cancelar uma instrução em execução (uma attention do TDS) mantém a
  sessão, a transação e as tabelas temporárias dela.
- Vários conjuntos de resultados de um só batch, e mensagens `PRINT`.
- Vários bancos de dados por conexão: uma conexão que nomeia o banco de dados mostra só esse;
  *Mostrar todas as bases de dados* lista os demais.
- O Explain mostra o plano **estimado** (`SHOWPLAN`). O Explain analyze é recusado.
- *Encerrar sessão* é `KILL`. Cancelar a instrução de outra sessão não é possível no SQL Server,
  então a aba só oferece encerrar a sessão.
- As importações usam a cópia em massa do SQL Server (inserções `OPENJSON` para tabelas com colunas
  `money`, `xml` ou legadas).
- Ferramentas de schema (editor de tabelas, diagrama ER, comparação), também para colunas
  calculadas e com collation, índices ricos e tabelas temporais. Índices XML e espaciais ficam fora
  de uma comparação.
- O modo terminal executa o `sqlcmd`, se ele estiver no PATH.

## Agentes

No SQL Server, só a verificação da instrução impede gravações de agentes somente leitura (não há
sessão somente leitura no servidor); a transação de um agente sofre rollback. `server_activity` e
`top_queries` funcionam. Veja [Agentes e a CLI](../../AGENTS.md).

## Limites conhecidos

- Anterior ao 2017: o app conecta e avisa que alguns recursos podem não funcionar.
- Sem Explain analyze e sem busca de instâncias nomeadas.

## Testado em

A suíte de conformidade roda no SQL Server 2017 e 2022 (contêineres), e o driver ODBC genérico é
experimentado contra o mesmo contêiner 2022. O Azure SQL não é testado: as regras de redirecionamento,
de login Entra e de criptografia dele estão no código, mas não foram executadas contra o serviço.

Veja também: [Conectar](../connect.md), [Solução de problemas](../troubleshooting.md).
