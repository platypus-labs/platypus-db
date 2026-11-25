<!-- source: 1206da228de0 -->
[← Bancos de dados](README.md)

# IBM Db2 e ODBC (Windows)

O Db2, e qualquer banco de dados com um driver ODBC ou uma fonte de dados (DSN), são alcançados pelo
gerenciador de drivers ODBC do sistema. Isso funciona **só no Windows** por enquanto: no macOS e no
Linux, uma conexão Db2 ou ODBC diz que os drivers ODBC são alcançados só no Windows.

## Primeiro uso: o driver ODBC da IBM (Db2)

Quando você escolhe **IBM Db2** para uma nova conexão e nenhum driver ODBC da IBM está registrado
neste computador, um popup se abre sobre o formulário: *IBM Db2 precisa de IBM Data Server Driver
for ODBC and CLI*. Ele mostra o pacote (versão 12.1.4), o tamanho, os termos da IBM e de onde vem, e
verifica o download com um SHA-256. **Baixar e instalar** o obtém na pasta de dados do app e o
registra com `db2cli install -setup`; **o Windows pede permissão de administrador (Controle de
Conta de Usuário)** para isso. **Cancelar** deixa o formulário utilizável, e a oferta volta em
*Testar conexão* e na primeira abertura.

Um driver que já esteja no sistema vale: o app procura um chamado `IBM DB2 ODBC DRIVER…` ou
`IBM DATA SERVER…`, seja ele registrado pelo próprio app ou por um cliente IBM completo, e usa o mais
novo pelo nome. A opção `driver` da conexão substitui essa escolha. *Configurações › Drivers* lista
e remove o driver baixado, e mostra os drivers e fontes de dados ODBC do sistema.

## Conectar

**IBM Db2**: host, porta (50000 por padrão), **banco de dados** (host e banco de dados são ambos
obrigatórios), usuário e senha. O app monta a connection string sozinho (`DATABASE`, `HOSTNAME`,
`PORT`, `PROTOCOL=TCPIP`, `UID`, `PWD`). TLS: os modos `disable` e `prefer` conectam sem ele;
`require` e acima adicionam `Security=SSL`, e `sslrootcert` vira `SSLServerCertificate`. Os
servidores Db2 escutam o TLS em uma porta diferente da simples, então defina a porta. Por um túnel
SSH, usa-se a ponta local do túnel.

A **Plataforma** escolhe o tipo de servidor Db2:

- *Db2 for Linux, UNIX and Windows (LUW)*, o padrão, também para servidores 8.x antigos.
- *Db2 for z/OS*: o mesmo driver da IBM, porta 446 por padrão, e o **banco de dados** é o nome da
  location. O driver da IBM só alcança o z/OS com uma licença do Db2 Connect neste computador.
- *Db2 for IBM i* (AS/400): o *IBM i Access ODBC Driver* da própria IBM (IBM i Access Client
  Solutions, instalado a partir da IBM, não pelo app; o driver iSeries Access mais antigo também
  funciona). O host é o nome do sistema, o banco de dados é opcional e define a biblioteca padrão,
  não há porta, e o TLS acrescenta `SSL=1`. Ele não passa por um túnel SSH, pois o driver abre várias
  portas do servidor.

**Outro driver ODBC** (a escolha genérica): informe um destes

- uma **fonte de dados (DSN)** que o sistema conhece, digitada ou tirada das sugestões;
- o nome de um **driver** (sugerido a partir dos drivers ODBC do sistema) com uma **string de
  conexão** contendo o resto, como `Server=host;Database=sales`;
- uma **string de conexão** sozinha, usada como foi escrita (ela pode citar o próprio `DRIVER=`).

Os campos de usuário e senha são adicionados como `UID` e `PWD`, a menos que a string já os tenha.
Uma string ADO.NET ou ODBC colada com `Driver={…}` é lida no formulário correspondente quando o
driver é SQL Server, PostgreSQL, MySQL/MariaDB ou ClickHouse; qualquer outro driver ou um `DSN=`
vira uma conexão ODBC genérica com a string como está.

## Privilégios

O que o login do banco de dados tiver. O navegador usa as chamadas de catálogo do ODBC (`SQLTables`,
`SQLColumns`, `SQLPrimaryKeys`, `SQLForeignKeys`). Não há abas de administração.

## O que funciona

- Consultas e resultados, com os tipos que o driver relata.
- Commit e rollback manuais.
- Cancelar: a chamada de cancelamento do ODBC não é alcançável, então uma instrução que ainda não
  começou é abandonada junto com a sessão dela (o app abre uma nova); uma que está devolvendo linhas
  para quando o stream dela é descartado.
- O navegador, a partir das chamadas de catálogo. O SQL do Db2 é escrito com as aspas do Db2 (nomes
  sem aspas viram maiúsculas, as linhas são limitadas com `FETCH FIRST`); o driver genérico escreve
  SQL padrão.

## O que não funciona

- Sem explain, sem DDL de objetos, sem abas de administração, sem carga em massa (as importações
  usam `INSERT`).
- **Valores de texto e binários com mais de 64 KiB são um erro**, em vez de um buffer enorme.
- **A edição de dados na grade** não é testada nesses bancos de dados. A grade não a bloqueia, mas
  sem savepoints um salvamento que falha no modo de commit manual desfaz a transação inteira da
  sessão e diz isso.
- O modo terminal não está disponível.

## Agentes

Agentes somente leitura dependem só da verificação da instrução. Veja
[Agentes e a CLI](../../AGENTS.md).

## Limites conhecidos

- Só no Windows.
- O Db2 não tem contêiner na configuração de testes, então a connection string do Db2 e a detecção
  do driver são verificadas só por testes unitários.

## Testado em

O driver ODBC genérico roda contra o SQL Server 2022 pelo driver `SQL Server` do próprio Windows:
tipos, resultados longos, erros com o SQLSTATE, metadados, commit e rollback, cancelamento de uma
instrução em espera, literais e tipos do próprio SQL Server, e importações. O IBM Db2 não é testado
contra um servidor.

Veja também: [Conectar](../connect.md), [Solução de problemas](../troubleshooting.md).
