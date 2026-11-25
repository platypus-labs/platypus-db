<!-- source: 8b43e38d9f7d -->
[← Bancos de dados](README.md)

# Redis

O driver é construído sobre o crate `redis` (RESP3 quando o servidor o fala, com rustls para TLS).
Ele abre o Redis 6 e posteriores, e o Redis Stack para chaves JSON. O editor aceita **comandos do
Redis**, um por linha. O driver faz parte do app; nada é baixado.

## Conectar

| Você cola | Lido como |
|---|---|
| `redis://:s%40cret@cache:6380/3` | TCP simples, só senha, banco de dados 3 |
| `redis://app:pw@cache/0` | um usuário de ACL e a senha dele |
| `rediss://app:pw@cache` | TLS (verificado: `verify-full`) |

A porta padrão é 6379. O caminho (ou o campo Banco de dados) é o **número do banco de dados**, `0`
quando vazio; qualquer outra coisa é um erro. O usuário e a senha são os da ACL: uma senha sozinha
entra como o usuário default.

O **TLS** está ativado para URLs `rediss://` e para modos SSL de `require` para cima (`redis://`
significa sem TLS e `prefer` não o ativa, porque o Redis aceita TLS em uma porta própria). `require`
não verifica o certificado; `verify-ca` e `verify-full` verificam. Um arquivo de CA e um certificado
de cliente vão nos campos de TLS. A sessão abre uma conexão por banco de dados que usa, porque um
`SELECT` em uma conexão compartilhada moveria todos os outros usuários dela.

## Privilégios

As regras de ACL do login decidem o que o navegador e os comandos podem fazer. O navegador lê o
`INFO` e, para cada tipo de chave, executa `SCAN … TYPE`; navegar por um tipo executa `BROWSE`,
nunca `KEYS`. Não há abas de administração.

## O que funciona

- Comandos no editor, com aspas como o `redis-cli` as usa; linhas com `#` e `--` são comentários. As
  respostas são linhas, com respostas aninhadas como JSON na árvore de documento (`F7`).
- `SELECT n` move a sessão para outro banco de dados. `MULTI` enfileira comandos até o `EXEC`, que os
  envia como uma só transação e mostra cada resposta.
- O navegador lista os bancos de dados que têm chaves, cada um com uma tabela por tipo de chave:
  `string`, `hash`, `list`, `set`, `zset`, `stream` e `json` com o RedisJSON. Abrir uma executa
  `BROWSE db0.hash`, opcionalmente com `MATCH` e `LIMIT`, lendo chaves grandes um pedaço por vez.
- **A grade edita linhas**: valores, TTLs, nomes de chaves, campos de hash, membros e scores; novas
  linhas adicionam chaves e itens, linhas excluídas os removem. A visualização mostra os comandos,
  que rodam como um só `MULTI`/`EXEC`; um comando que o servidor recusa não desfaz os outros. Um
  *Script para desfazer* devolve os valores. Uma chave, campo ou membro que já existe impede o
  salvamento antes de qualquer coisa executar.
- `FLUSHDB` e `FLUSHALL` perguntam antes, como um `UPDATE` sem `WHERE`; em conexões de produção, os
  comandos que alteram o servidor (`CONFIG SET`, `SHUTDOWN`) perguntam como o DDL. Conexões somente
  leitura executam apenas comandos que leem.
- Cancelar interrompe um comando bloqueante (`BLPOP`, `XREAD BLOCK`).
- O modo terminal executa o `redis-cli` se ele estiver no PATH, com a senha no ambiente dele.
- Todos os comandos e tipos de chave estão descritos em [FEATURES.md](../../FEATURES.md#redis).

## O que não funciona

- Assinaturas (`SUBSCRIBE` e semelhantes) e `MONITOR` são recusados: tomariam conta da conexão.
- Sem commit manual: `MULTI`/`EXEC` é a única transação.
- Sem explain, sem DDL de objetos, sem ferramentas SQL (editor de tabelas, diagrama ER, comparação
  de schemas, query builder, importação de SQL), sem snapshots antes de alterações.

## Agentes

`run_query` e `sample_rows` (`BROWSE … LIMIT n`) funcionam; qualquer comando que não seja leitura é
perguntado antes. Os comandos que mostram as credenciais do servidor (`ACL GETUSER`, `ACL LIST` e um
`CONFIG GET` cujo padrão corresponde a uma configuração de senha) são recusados para agentes
somente leitura. Recusados no Redis: `explain`, `get_er_diagram`, `propose_edits` e
`query_across_databases`. Veja [Agentes e a CLI](../../AGENTS.md).

## Limites conhecidos

- Anterior ao 6.0: ele conecta com um aviso de que alguns recursos podem não funcionar.
- A contagem de chaves de uma tabela só aparece quando o banco de dados tem até 10.000 chaves.
- Valores que não são UTF-8 aparecem entre aspas, com escapes.

## Testado em

A suíte de conformidade roda no Redis Stack 7.4 (com RedisJSON): metadados, `BROWSE` em todos os
tipos, comandos e seus erros, salvamento e desfazer de edições da grade, cancelamento de comandos
bloqueantes, usuários de ACL e seleção de banco de dados, e uma conexão perdida que reabre. O Redis
6 é a versão mais baixa que o driver aceita, mas não é executada pela suíte.

Veja também: [Conectar](../connect.md), [Solução de problemas](../troubleshooting.md).
