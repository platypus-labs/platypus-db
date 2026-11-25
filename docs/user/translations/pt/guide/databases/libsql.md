<!-- source: 7b39ba524d58 -->
[← Bancos de dados](README.md)

# libSQL e Turso

O driver fala o protocolo **Hrana 3** por HTTP(S) com um servidor libSQL (`sqld`) ou com o Turso. O
SQL é o do SQLite, então o navegador, os tipos e os planos também são os do SQLite. O driver faz
parte do app; nada é baixado.

## Conectar

| Você cola | Lido como |
|---|---|
| `libsql://db-acme.turso.io?authToken=…` | HTTPS para esse host, o token como senha |
| `libsql://:token@localhost:8080?sslmode=disable` | HTTP simples, o token na posição da senha |
| `libsql://localhost:8080?tls=0&authToken=t` | `tls=0` é HTTP simples, `tls=1` é HTTPS |
| `wss://…`, `ws://…` | o mesmo servidor por HTTP(S): `wss` é TLS, `ws` não é |
| `https://host` ou `http://host` em um formulário do libSQL | o endereço HTTP do servidor |

Sem porta, usa-se o padrão do esquema: 443, ou 80 quando o TLS está desativado.

- **Token**: o campo de senha, ou `authToken` na URL.
- **TLS**: uma URL `libsql://` é HTTPS com o certificado e o nome do host verificados
  (`verify-full`), porque a URL carrega o token. O tile do Turso no formulário também começa em
  `verify-full`. Para um `sqld` local sem TLS, escolha `disable`, ou use `tls=0` na URL.
  `sslrootcert` indica o arquivo da sua CA; senão, os certificados confiáveis do sistema são usados.

## Privilégios

O que o token permitir: o app envia as instruções que você executa e as consultas de catálogo do
SQLite para o navegador.

## O que funciona

- Edição de dados na grade, por chave primária (ou uma chave única em colunas não nulas, ou uma
  chave virtual).
- Commit e rollback manuais. Uma instrução roda em um stream que o servidor mantém aberto entre as
  requisições (um `baton`), então uma transação e as tabelas temporárias duram. Se o servidor derruba
  o stream fora de uma transação, o app abre outro e segue; dentro de uma transação, a conexão se
  declara perdida, e a transação com ela.
- O Explain mostra o plano de consulta do SQLite; não existe Explain analyze.
- Busca de metadados e DDL de objetos, e ferramentas de schema como no SQLite.
- Sem abas de sessões, locks, painel, usuários ou parâmetros.

## O que não funciona

- **Cancelar**: o HTTP não tem como interromper uma instrução em execução, então Cancelar responde
  que cancelar instruções do libSQL não é suportado.
- Os resultados chegam inteiros do servidor, não linha a linha.
- O modo terminal não está disponível: os servidores libSQL não têm um cliente de linha de comando
  para executar scripts.

## Agentes

Agentes somente leitura e conexões somente leitura dependem só da verificação da instrução; o
servidor não é instruído a recusar gravações. Veja [Agentes e a CLI](../../../../AGENTS.md).

## Limites conhecidos

- Uma falha de rede depois que uma instrução foi enviada pode deixar incerto se ela foi executada; o
  app não a envia de novo.

## Testado em

A suíte de conformidade roda no `sqld` v0.24.33 (contêiner `ghcr.io/tursodatabase/libsql-server`),
sem TLS. A nuvem do Turso não é testada.

Veja também: [Conectar](../connect.md), [Solução de problemas](../troubleshooting.md).
