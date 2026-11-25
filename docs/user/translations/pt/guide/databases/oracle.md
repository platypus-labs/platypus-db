<!-- source: fd0bf4386fb3 -->
[← Bancos de dados](README.md)

# Oracle

O driver usa a biblioteca cliente da Oracle (ODPI-C, pelo crate `oracle`), carregada quando você
conecta. A biblioteca é o Oracle Instant Client, que o app baixa no primeiro uso.

## Primeiro uso: Instant Client

Quando você escolhe **Oracle** para uma nova conexão e o cliente não está neste computador, um popup
se abre sobre o formulário: *Oracle precisa de Oracle Instant Client*. Ele mostra o pacote (Basic
Light 23.26.4), o tamanho, a licença (Oracle Free Use Terms and Conditions) e de onde vem, e
verifica o download com um SHA-256. **Baixar** o obtém na pasta de dados do app; **Cancelar** deixa o
formulário utilizável, e a oferta volta em *Testar conexão* e na primeira abertura. *Configurações ›
Drivers* lista o cliente e o remove.

- O app o baixa no Windows (x64) e no Linux (x64 e ARM64). No macOS não baixa nada: instale o
  Instant Client da Oracle e coloque a pasta dele no caminho de bibliotecas.
- Se o app não baixou um, ele usa um Instant Client ou cliente completo que o sistema consiga
  carregar.
- O Windows precisa do Microsoft Visual C++ Redistributable, e o Linux da biblioteca `libaio`
  (`libaio1`, `libaio1t64` ou `libaio`, conforme a distribuição); quando o cliente não carrega, o
  erro cita a peça que falta.

## Conectar

Não existe URL `oracle://`; cole uma URL JDBC no lugar, como
`jdbc:oracle:thin:@host:1521/service` (as formas thin e OCI são lidas), ou preencha o formulário:

- **Host** e **porta** (padrão 1521), e **Nome do serviço** no campo Banco de dados (por exemplo
  `FREEPDB1`). Eles são escritos como uma string Easy Connect, `host:port/service`.
- **Sem um host**, o campo Banco de dados é um net service name do `tnsnames.ora`, ou um connect
  descriptor completo, escrito como está.
- **Usuário e senha.** Sem usuário, a conta do sistema operacional ou uma wallet Oracle faz o login.
  O usuário pode ser escrito à moda do SQL*Plus, `sys as sysdba`, ou você pode escolher o **Papel**
  (`SYSDBA`, `SYSOPER`, `SYSBACKUP`, `SYSDG`, `SYSKM`, `SYSASM`).
- **TLS**: os modos `disable` e `prefer` usam TCP simples; `require` e acima usam `tcps://` com os
  certificados confiáveis do sistema. `require` ignora a correspondência do nome do servidor;
  `verify-ca` e `verify-full` a verificam. O timeout de conexão se aplica. Por um túnel SSH, usa-se
  a ponta local do túnel.

## Privilégios

Navegar lê as views `ALL_*`, então o login vê os objetos sobre os quais tem direitos. Cancelar envia
um break; se a instrução ainda estiver em execução depois de alguns segundos, o app a cancela de uma
segunda sessão com `ALTER SYSTEM CANCEL SQL`, que exige esse privilégio, e, na falta dele, abre mão
da sessão (a transação dela sofre rollback e uma nova conexão é aberta).

## O que funciona

- Edição de dados na grade, por chave primária (ou uma chave única em colunas não nulas, ou uma
  chave virtual).
- Commit e rollback manuais.
- Blocos PL/SQL: um script termina um bloco com uma linha `/`. O texto de `DBMS_OUTPUT` volta como
  mensagens.
- Navegador (tabelas, views, views materializadas, sequences, sinônimos, funções, procedures,
  triggers, tipos e pacotes), DDL de objetos e ferramentas de schema.
- O Explain mostra o plano de `EXPLAIN PLAN` e `DBMS_XPLAN`, só estimado: não existe Explain
  analyze.
- Erros de compilação, números e horas em nanossegundos, JSON, vetores, cursores, LOBs longos e
  colunas de arquivo são lidos de volta como o banco de dados os guarda.
- Sem abas de sessões, locks, painel, instruções, usuários ou parâmetros.
- O modo terminal não está disponível.

## Agentes

Agentes somente leitura dependem só da verificação da instrução: o Oracle não tem aqui uma sessão
somente leitura no servidor. Uma instrução que chama um pacote Oracle, como `UTL_HTTP`, é perguntada
antes de executar, como uma alteração. Veja [Agentes e a CLI](../../../../AGENTS.md).

## Limites conhecidos

- O app não compara a versão do servidor com um mínimo; o cliente que ele baixa é o 23.26.4.
- Para um servidor anterior ao 12, o navegador lê o dicionário de outro modo (sem
  `ORACLE_MAINTAINED`, sem `FETCH FIRST`). Só o Oracle Free 23.26 roda nos testes, então esse
  caminho nunca foi exercitado. O Autonomous Database e outros serviços de nuvem da Oracle também
  não são testados.
- O Instant Client não é baixado no macOS.

## Testado em

A suíte de conformidade roda no Oracle Free 23.26 (contêiner `gvenzl/oracle-free`), com o Instant
Client baixado ou o do sistema: tipos, resultados longos, erros que apontam o engano, PL/SQL,
navegador, commit e rollback, salvamento de muitas linhas editadas, cancelamento (também de um sleep
PL/SQL), planos, DDL que roda de volta, conexão como `SYSDBA` e comparação de schemas. Outras
versões não são testadas.

Veja também: [Conectar](../connect.md), [Solução de problemas](../troubleshooting.md).
