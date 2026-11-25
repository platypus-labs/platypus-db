<!-- source: eeb36912ad18 -->
[← Bancos de dados](README.md)

# SQLite

O SQLite roda dentro do app (`rusqlite` com o SQLite embutido): sem servidor, sem host, sem usuário,
sem senha, nada para baixar.

## Conectar

Escolha **SQLite** e informe o caminho do arquivo, ou cole uma URL:

| Você cola | Significado |
|---|---|
| `sqlite:///home/ana/app.db` | o arquivo `/home/ana/app.db` |
| `sqlite:///C:/data/app.db` | um arquivo em uma unidade do Windows |
| `sqlite:///:memory:` | um banco de dados em memória |
| `file:app.db?mode=ro` no campo de caminho | uma URI do SQLite, passada ao SQLite como foi escrita |

O arquivo é criado quando não existe mas a pasta dele existe, como um banco de dados vazio. Uma
pasta inexistente é um erro (mais provavelmente um erro de digitação ou uma unidade não montada), e
um caminho relativo que não existe também: informe o caminho completo. Um arquivo que não é um banco
de dados SQLite é recusado na conexão.

Uma conexão somente leitura (e um agente somente leitura) abre o arquivo somente para leitura, então
o próprio SQLite recusa uma gravação. As configurações de SSL, SSH e proxy não se aplicam a um banco
de dados embutido.

## Privilégios

Os do sistema operacional: o app precisa ler o arquivo, e gravar nele e na pasta dele (para o
journal) para alterar qualquer coisa. Um arquivo ocupado é aguardado antes de um erro.

## O que funciona

- Edição de dados na grade, por chave primária (ou uma chave única em colunas não nulas, ou uma
  chave virtual). Uma coluna `INTEGER PRIMARY KEY` conta como chave, embora o SQLite a relate como
  anulável.
- Commit e rollback manuais; cancelamento de uma instrução em execução (`sqlite3_interrupt`).
- O Explain mostra só o plano de consulta do SQLite: não existe Explain analyze.
- Aba Parâmetros: os pragmas que importam, com os padrões documentados do SQLite.
- Sem abas de sessões, locks, painel, usuários ou instruções.
- O editor de tabelas, o diagrama ER e a comparação. O SQLite altera pouca coisa no lugar, então
  mudar uma tabela a cria de novo e copia as linhas dela; a comparação e a reconstrução mantêm
  índices de expressão e parciais, chaves diferidas e o contador `AUTOINCREMENT`.
- Busca de metadados, DDL de objetos, e exportações, importações e cópias como em qualquer banco de
  dados SQL.
- O modo terminal executa o `sqlite3`, se ele estiver no PATH.

## Agentes

Agentes somente leitura e conexões somente leitura abrem o arquivo somente para leitura, e executam
as consultas com `query_only`. `estimate_change` não tem estimativa de linhas para dar no SQLite.
Veja [Agentes e a CLI](../../../../AGENTS.md).

## Limites conhecidos

- Outro processo pode estar segurando o arquivo: o app espera até 5 segundos por um arquivo ocupado,
  depois o SQLite relata o erro.
- Uma instrução de agente que chama `load_extension(…)` é perguntada antes, como uma alteração.

## Testado em

A suíte de conformidade roda no SQLite embutido no app, incluindo transações, planos, edição de
tabelas, a reconstrução que mantém o `AUTOINCREMENT` e arquivos abertos somente para leitura.

Veja também: [Conectar](../connect.md), [Solução de problemas](../troubleshooting.md).
