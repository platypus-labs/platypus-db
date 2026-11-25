<!-- source: 81c6c1d2c8d1 -->
[← Bancos de dados](README.md)

# MongoDB

O driver é o driver Rust oficial do MongoDB (com rustls, então sem OpenSSL). Ele abre o MongoDB 5.0 e
posteriores. O editor aceita a **sintaxe do mongosh**, não SQL. O driver faz parte do app; nada é
baixado.

## Conectar

| Você cola | Lido como |
|---|---|
| `mongodb://user:pass@host:27017/shop?authSource=admin&replicaSet=rs0` | a forma de URL; porta padrão 27017 |
| `mongodb://a:27017,b:27018/?authSource=admin` | uma lista de seeds, mantida inteira no campo Host |
| `mongodb+srv://user:pass@cluster0.example.net/shop` | SRV: os hosts vêm do DNS |

O que não cabe nos campos do formulário vai no **campo Banco de dados**, depois de um `?`, como
opções da connection string: `shop?authSource=admin&replicaSet=rs0`, com `srv=true` para
`mongodb+srv`. O nome antes do `?` é o banco de dados em que as instruções rodam até um `use` mudá-lo
(`test` quando vazio).

- **Login**: um usuário e senha, autenticados contra o `admin`, a menos que `authSource` diga o
  contrário. Uma URL colada cujo caminho nomeia um banco de dados o usa como auth source, como a
  especificação da connection string diz. As opções que guardam um segredo (uma senha em
  `authMechanismProperties`, por exemplo) não são mantidas quando você cola uma URL; o formulário
  lista o que deixou de fora.
- **TLS**: `tls=true` ou `ssl=true` na URL significa `verify-full`; `tls=false` significa `disable`.
  O MongoDB não tem TLS oportunista, então `prefer` (o padrão) mantém o que a connection string
  implica: TLS ativado para `mongodb+srv`, desativado nos demais. `require` ativa o TLS e aceita
  qualquer certificado; `verify-ca` e `verify-full` verificam a cadeia e o nome do host.
  `sslrootcert` (ou `tlsCAFile`) indica o arquivo da sua CA; um certificado de cliente pode ser
  informado no formulário, ou com `tlsCertificateKeyFile` no campo Banco de dados. As opções de TLS
  da connection string são mantidas em todos os modos.
- **Por um túnel SSH** só o servidor que está atrás dele é alcançado (`directConnection=true`).

## Privilégios

O navegador lista os bancos de dados e coleções que o login pode listar, e amostra documentos para
mostrar os campos. Nada mais é lido para administração: não há abas de administração. As instruções
que você escreve exigem os papéis de que precisam.

## O que funciona

- Instruções na sintaxe do mongosh, com os valores do shell (`ObjectId(...)`, `ISODate(...)`,
  `NumberLong(...)`, `/regex/i`) e Extended JSON:
  - **Leitura**: `find` com `sort`, `limit`, `skip`, `projection`, `collation`, `hint`, `min`, `max`,
    `maxTimeMS`, `readConcern` e semelhantes, `findOne`, `aggregate`, `countDocuments`,
    `estimatedDocumentCount`, `distinct`.
  - **Gravação**: `insertOne`/`insertMany`, `updateOne`/`updateMany`, `replaceOne`,
    `deleteOne`/`deleteMany`, `bulkWrite`, `findOneAndUpdate`/`findOneAndReplace`/`findOneAndDelete` e
    `findAndModify`, com as opções deles (`arrayFilters`, `collation`, `hint`, `let`, `upsert`,
    `ordered`, `writeConcern`). Uma opção que o PlatypusDB não conhece é recusada pelo nome, nunca
    descartada.
  - **Índices e coleções**: `createIndex`, `createIndexes`, `dropIndex`, `dropIndexes`, `hideIndex`,
    `unhideIndex`, `getIndexes`, `createCollection`, `createView`, `drop`, `renameCollection`,
    `convertToCapped`, `stats`, `validate`, `latencyStats`, `dataSize`, `storageSize`, `totalIndexSize`,
    `totalSize`, `isCapped`, `exists`.
  - **Planos**: `.explain()` depois de uma instrução ou `db.c.explain().find(…)` antes dela, com um
    nível de detalhe, e o **Explain** do editor. Um plano nunca aplica uma gravação.
  - **O servidor**: `db.runCommand`, `db.adminCommand`, `db.aggregate`, `db.serverStatus`, `db.currentOp`,
    `db.killOp`, o profiler, usuários e papéis (`db.createUser` e semelhantes), `rs.status`, `rs.conf`,
    `sh.status`, `show dbs`, `show collections`, `show users`, `show roles`, `show profile`, `show logs`
    e `use`.
- Os resultados são uma grade dos campos de nível superior, com o documento em foco como uma árvore
  ao lado (`F7`). O navegador lista bancos de dados, coleções, campos amostrados dos documentos e
  índices; o editor completa coleções, métodos e campos.
- Cancelar interrompe uma instrução em execução; um resultado abandonado libera a conexão.
- **Commit manual** em um replica set ou em um cluster com sharding (veja
  [Transações](#transações)).
- **Edição de documentos na grade** (veja [Editar documentos na grade](#editar-documentos-na-grade)).
- A definição de uma coleção como DDL.
- **Gerar dados** preenche uma coleção com documentos falsos.
- O mesmo detalhe está em [FEATURES.md](../../FEATURES.md#mongodb).

## O que não funciona

- **Só alguns resultados são editáveis na grade**: os documentos de uma coleção (veja
  [Editar documentos na grade](#editar-documentos-na-grade)). Todo outro resultado, como uma
  agregação, uma view ou um `findOne`, continua somente leitura. Altere esses documentos com
  `updateOne`, `insertOne` e semelhantes.
- **Sem transações em um servidor standalone**: o MongoDB só as executa em um replica set ou em um
  cluster com sharding. O commit manual é recusado ali, dizendo isso.
- **Sem JavaScript**: o editor executa instruções, então `forEach`, `map`, `next`, `mapReduce`, os
  construtores de bulk mais antigos (use `bulkWrite`) e variáveis são recusados, dizendo isso. O
  `watch` também é recusado: um change stream tomaria conta da conexão. Um `.comment()` em uma
  leitura é deixado de fora: o do próprio app serve para achar a operação a cancelar.
- O `bulkWrite` não é atômico, como no mongosh: o que rodou antes de uma falha permanece, e o erro
  diz até onde chegou. Inserções vizinhas viajam juntas, cada outra gravação em uma viagem própria.
- Sem SQL: sem editor de tabelas, diagrama ER, comparação de schemas, query builder, importação de
  SQL, snapshots antes de alterações nem modo terminal (o `mongosh` só aceita a senha na linha de
  comando).

## Editar documentos na grade

A grade edita o resultado de um `find` simples em uma coleção: o visualizador de dados, ou uma
instrução que você digita, como `db.orders.find({ status: 'paid' }).sort({ _id: 1 }).limit(20)`. A
coleção pode ser nomeada com `getCollection("my orders")` ou `db.getSiblingDB("shop").orders`. Depois
de um `use`, a coleção é a do banco de dados para o qual você mudou. Uma projeção serve quando só lista
campos a incluir e mantém o `_id`. Views, agregações, `findOne`, `distinct`, contagens, uma projeção
sem `_id` e uma projeção que deixa campos de fora continuam somente leitura.

Um documento é encontrado pelo `_id`. Um resultado sem `_id` é somente leitura. **Visualizar SQL**
mostra o mongosh que vai rodar, uma instrução por documento:

- um documento editado é `updateOne({ _id: … }, { $set: { … } })`. Uma célula NULL grava `null`.
  Quando um número muda, o update é um pipeline, `[{ $set: { … } }]`: o número é gravado com um
  `$switch` sobre o tipo que o documento guarda (veja abaixo), e todo outro valor como um `$literal`,
  para que uma string como `$total` ou um documento com chaves `$` seja salvo como está;
- um novo documento é `insertOne({ … })`. As células deixadas NULL não são enviadas. Sem um `_id`, o
  app cria um `ObjectId`;
- um documento excluído é `deleteOne({ _id: … })`.

Os valores são gravados com o tipo BSON da coluna deles: `bool`, `string`, `objectId`, `date`, e
`document` e `array` a partir do JSON que a grade mostra. Uma data é escrita como
`2024-01-02 10:30:00`, com ou sem o fuso horário (UTC sem um). Os números dentro de um documento ou
de um array mantêm o tipo que o JSON lhes dá: um número inteiro é um int (um long além de 32 bits),
um número com fração ou expoente é um double.

Um número mantém o tipo que o documento guarda, `int`, `long`, `double` ou `decimal`, sempre que esse
tipo comportar o novo valor com exatidão: o tipo de uma coluna é só o que os documentos dela têm em
comum (ints e longs formam uma coluna `long`, qualquer um deles com doubles uma `double`), então ele
não diz o que um documento guarda. Um int editado para `7` continua um int, um long um long, um
double um double. Um valor que o tipo guardado não comporta recebe o tipo da coluna, ou o primeiro
tipo numérico que o comporta: `3.5` em um campo int vira um double, `3000000000` um long. Um long além
de 2^53 nunca é gravado como um double, o que perderia dígitos. Nada é convertido no servidor: o app
só grava o número nos tipos que ele sabe que o comportam com exatidão.

Estas coisas são recusadas antes de qualquer coisa rodar, com o motivo:

- uma alteração no `_id`: o MongoDB nunca o altera. Exclua o documento e insira-o de novo;
- uma coluna de tipos mistos (`mixed`): o texto de um valor não distingue uma string de um número;
- dados binários e os outros tipos que a grade não consegue gravar de volta com exatidão
  (expressões regulares, timestamps, JavaScript);
- um campo com um `.` no nome ou um nome que começa com `$`: o `$set` o leria como um caminho ou um
  operador;
- um valor que não cabe no tipo dele, como `many` em uma coluna `int` ou um array em uma coluna
  `document`.

Cada instrução deve alterar exatamente um documento. Se uma não encontra nenhum, porque o documento
foi excluído ou o `_id` dele mudou desde que a grade o leu, o salvamento para e diz qual comando.

**Tudo ou nada exige um replica set.** Em um replica set ou em um cluster com sharding, um
salvamento roda em uma transação própria e só faz commit se todos os comandos funcionaram. Se você
tem uma transação aberta (commit manual), o salvamento roda nela e espera o Commit. Se um comando
falha, o MongoDB não tem savepoints: a transação inteira sofre rollback, com as alterações que você
ainda não tinha confirmado, e o erro diz isso. Em um servidor standalone não há transações: os
comandos rodam um após o outro, e uma falha interrompe o resto. Os comandos anteriores permanecem
salvos, e o erro diz quantos.

O script para desfazer que segue um salvamento devolve os documentos: `$set` dos valores antigos para
edições, `deleteOne` para novos documentos, `insertOne` dos campos carregados para os excluídos. As
linhas que começam com `//` dizem o que ele não consegue restaurar com exatidão: campos que o
resultado não mostrou, colunas de tipos que a grade não consegue gravar, campos que eram NULL (voltam
como `null`, ou ficam de fora), e números cujo tipo a edição pode ter mudado (voltam como números do
tipo que têm então, pelo mesmo `$switch`).

## Transações

O commit manual funciona em um replica set ou em um cluster com sharding (um `mongos`); um replica
set de um só nó basta. Em um servidor standalone ele é recusado: o botão **Auto-commit** fica
desativado e diz por quê. Uma conexão de produção com um replica set abre em commit manual, com
Commit e Rollback. A conexão mantém uma sessão: a primeira alteração depois que você passa ao commit
manual, ou depois de um Commit ou Rollback, abre uma transação nela (um insert, update ou delete,
`createCollection`, `createIndex`, um salvamento da grade), e Commit ou Rollback a encerra. As
leituras antes dela não abrem nenhuma: elas veem o que foi confirmado, como no autocommit, então
navegar pelos dados nunca deixa uma transação aberta para o servidor encerrar depois do tempo de vida
dela. Com uma transação aberta, as suas leituras rodam nela e veem as gravações dela; as outras
sessões só as veem depois do Commit.

- **Na transação**, quando uma está aberta (as leituras) ou ao abri-la (o resto): `find`, `findOne`,
  `aggregate`, `countDocuments`, `distinct`, os inserts,
  updates, replaces e deletes, `bulkWrite`, os métodos `findOneAnd…`, `createCollection` e
  `createIndex`, e `db.runCommand` ou `db.adminCommand` com um comando que lê ou grava documentos
  (`find`, `aggregate`, `insert`, `update`, `delete`, `findAndModify`, …). O MongoDB só aceita um
  índice em uma transação em uma coleção criada na mesma transação: em uma que já existe, o servidor
  o recusa e encerra a transação.
- **Fora dela**, como no modo autocommit, sem tocá-la: o que lê metadados ou fala com o servidor.
  `show …`, `use`, `db.stats()`, `db.getCollectionNames()`, `getIndexes`, as estatísticas da coleção,
  `estimatedDocumentCount`, `.explain()`, os outros comandos de `db.runCommand` e `db.adminCommand`,
  `rs.*`, `sh.*`, usuários e papéis, o profiler, o navegador e o autocompletar. As leituras dos bancos
  de dados `admin`, `config` e `local` e das coleções `system.*` também rodam fora.
- **Recusados no commit manual**: `drop`, `renameCollection`, `dropIndex`, `dropIndexes`, `hideIndex`,
  `convertToCapped`, `createView`, um `createCollection` de série temporal, um `aggregate` com `$out`
  ou `$merge`, e gravações em `admin`, `config`, `local` ou `system.*`. O MongoDB não os executa em
  uma transação, e executados fora dela seriam aplicados na hora, o que o Rollback não poderia
  desfazer. O erro diz isso; nada é enviado e a transação aberta continua como estava. Passe ao
  autocommit para executá-los.
- Um resultado lido em uma transação continua carregando enquanto você rola, um batch por vez, e
  outras instruções podem rodar nesse meio tempo. Um resultado lido antes de a transação abrir
  continua carregando fora dela. As colunas dele vêm dos primeiros documentos. Depois de um Commit ou
  Rollback, o resto dele não pode ser lido: execute a instrução de novo.

**Quando o servidor encerra uma transação.** O MongoDB aborta uma transação que fica aberta por mais
de `transactionLifetimeLimitSeconds`, 60 segundos por padrão. Também aborta uma em alguns erros: uma
chave duplicada, um conflito de gravação com outra sessão, uma instrução que ele recusa em uma
transação, um limite de tempo (`maxTimeMS`) atingido. Outros erros, como um estágio de pipeline
desconhecido, a deixam aberta. Depois de um erro, o PlatypusDB pergunta ao servidor se a transação
ainda existe. Quando não existe, o erro da instrução ou do Commit diz que o servidor encerrou a
transação e que as alterações dela foram descartadas (com o limite de tempo quando foi esse o motivo),
a transação aparece como fechada, e a próxima instrução abre uma nova. Cancelar uma instrução em uma
transação desfaz a transação: o servidor encerra uma transação cuja operação é morta, e o que a
instrução fez é desconhecido.

## Agentes

`run_query`, `sample_rows` (`find({}).limit(n)`) e o resto funcionam com instruções do mongosh; uma
instrução que não é uma consulta simples é perguntada antes. Recusados no MongoDB: `explain`,
`get_er_diagram` e `query_across_databases`; `estimate_change` responde com uma nota.

`propose_edits` propõe documentos a inserir, atualizar ou excluir em uma coleção (não em uma view): a
coleção abre no seu visualizador de dados com um `find` dos `_id` propostos e as alterações pendentes,
que você salva ou descarta como as suas próprias edições. Um documento é nomeado só pelo `_id`; um
`_id` `ObjectId` é enviado como os 24 dígitos hexadecimais dele e lido como um `ObjectId` porque o
`_id` da coleção é um. Os valores são verificados pelas regras do salvamento da grade (veja acima),
antes de qualquer coisa abrir. As colunas sensíveis são mascaradas só por campo de nível superior: um
campo sensível dentro de um documento não é distinguido. Veja [Agentes e a CLI](../../AGENTS.md).

## Limites conhecidos

- Anterior ao 5.0: ele conecta com um aviso de que alguns recursos podem não funcionar.
- O driver é um pool que se reconecta sozinho, então uma conexão perdida não é relatada como nos
  outros bancos de dados.

## Testado em

A suíte de conformidade roda no MongoDB 5.0 e 8.0: informações do servidor, metadados, cancelamento,
resultados abandonados, erros com a posição deles, documentos falsos, e valores em documentos
posteriores que diferem do primeiro. As transações são testadas em um replica set de um só nó do
MongoDB 8.0: commit, rollback, leituras dentro e fora, um resultado de vários batches, metadados no
commit manual, e uma transação que o servidor encerrou pelo limite de tempo.

Veja também: [Conectar](../connect.md), [Solução de problemas](../troubleshooting.md).
