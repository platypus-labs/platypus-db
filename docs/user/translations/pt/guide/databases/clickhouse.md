<!-- source: b5b424fa73d7 -->
[← Bancos de dados](README.md)

# ClickHouse

O driver fala com a **interface HTTP** do ClickHouse e lê os resultados no formato de stream Arrow.
Ele faz parte do app; nada é baixado. Ele navega, consulta e projeta tabelas, mas não edita linhas
na grade.

## Conectar

| Você cola | Lido como |
|---|---|
| `clickhouse://default@ch.example.com/logs` | HTTP na porta 8123, banco de dados `logs` |
| `clickhouse://ch.example.com/logs?sslmode=require` | HTTPS; sem porta na URL, 8443 |
| `jdbc:clickhouse://host:8123/db` | uma URL JDBC |
| `https://host:8443` em um formulário do ClickHouse | a URL HTTP(S) do servidor |

Use a porta **HTTP**, não a nativa (9000). O usuário é `default` quando vazio; a senha é enviada no
cabeçalho `X-ClickHouse-Key`. O campo Banco de dados executa `USE <database>` depois de conectar.

O **TLS** é escolhido pelo modo, já que o HTTP não consegue negociá-lo: `disable` e `prefer` (o
padrão) conectam em **HTTP simples**; `require` e os modos `verify` usam HTTPS. Com HTTPS, a porta
padrão é 8443 quando a URL não informa nenhuma; se você preenche o formulário, digite a porta você
mesmo. `require` não verifica o certificado, `verify-ca` verifica a cadeia, `verify-full` também o
nome do host; `sslrootcert` indica o arquivo da sua CA, senão os certificados confiáveis do sistema
são usados.

Uma conexão é uma sessão HTTP: `SET`, `USE` e as tabelas temporárias valem entre instruções.

## Privilégios

Navegar lê `system.databases`, `system.tables`, `system.columns` e `system.data_skipping_indices`; o
login vê o que o ClickHouse permite que ele veja ali. As abas leem:

- **Parâmetros**: `system.settings` e `system.server_settings`.
- **Painel**: `system.metrics`, `system.events`, `system.parts` (tamanhos dos bancos de dados) e
  `system.replicas`; o que o login não pode ler mostra "—".
- **Usuários e papéis**: `system.users`, `system.roles`, `system.grants` e `system.role_grants`.
  Os usuários definidos nos arquivos de configuração do servidor também são listados; o servidor
  recusa alterá-los.

## O que funciona

- Consultas, resultados, o navegador, o autocompletar, o DDL de objetos, a busca de metadados e o
  designer de tabelas (incluindo a cláusula de engine e os índices de skipping).
- O Explain mostra o plano de `EXPLAIN indexes = 1`; não existe Explain analyze.
- Cancelar interrompe a instrução no servidor com `KILL QUERY`.
- Abas Parâmetros, Painel e Usuários e papéis. Sem abas de sessões, locks, instruções nem tabelas e
  índices.
- Exportações, e importações no ClickHouse como instruções `INSERT`.

## O que não funciona

- **Sem edição de dados na grade**: ela diz "tabelas ClickHouse não são editadas diretamente".
  Salvar verificaria a contagem de linhas de cada alteração dentro de uma transação, e o ClickHouse
  não tem transações nem contagem de linhas para updates. Altere as linhas com SQL.
- **Sem transações**: a barra de commit manual fica oculta; cada instrução tem efeito sozinha.
- Sem chaves estrangeiras nem constraints únicas (a chave primária só ordena os dados).

## Agentes

Um agente somente leitura, e uma conexão somente leitura, enviam toda instrução, exceto `SET`, com
`readonly = 1`, então o servidor recusa gravações e as funções de tabela que alcançam outros
servidores ou os arquivos dele (`url`, `s3`, `remote`, `file` e semelhantes). A instrução de um
agente também é verificada contra uma lista de funções de tabela que só leem dados locais. Veja
[Agentes e a CLI](../../../../AGENTS.md). O modo terminal não está disponível: o `clickhouse-client` usa
a porta nativa.

## Limites conhecidos

- HTTP simples, a menos que o modo seja `require` ou mais forte: o padrão `prefer` não criptografa.
- Só a interface HTTP é usada; um servidor que expõe apenas a porta nativa não pode ser alcançado.

## Testado em

A suíte de conformidade roda no ClickHouse 26.3. Versões mais antigas do ClickHouse não são
testadas, e o app não tem versão mínima para ele.

Veja também: [Conectar](../connect.md), [Solução de problemas](../troubleshooting.md).
