<!-- source: 2a42aace96dc -->
[← Database](README.md)

# Elasticsearch e OpenSearch

Un solo driver parla con entrambi tramite la loro API HTTP (`reqwest` con rustls). Il server dice quale
dei due è quando ti connetti, e la connessione lo mostra (`Connected: OpenSearch 2.17.1`). Il driver
fa parte dell'app; non si scarica nulla.

## Connessione

| Incolli | Lo legge come |
|---|---|
| `elasticsearch://user:pass@host:9200/logs-*` | HTTP in chiaro; il percorso è un index pattern |
| `elasticsearchs://user:pass@host:9200` | HTTPS (verificato: `verify-full`) |
| `opensearch://…` e `opensearchs://…` | lo stesso, ricordato come OpenSearch così l'URL viene riscritto in quel modo |

La porta predefinita è 9200. Il campo **Database** è un index pattern facoltativo (`logs-*`, oppure
più pattern separati da virgole) che restringe ciò che elenca il navigator; vuoto, elenca ogni indice
che non sia nascosto.

- **Accesso**: utente e password usano l'autenticazione Basic. Con l'utente vuoto, il campo password
  è una **API key** (il valore `encoded`), inviata come `Authorization: ApiKey …`.
- **TLS**: HTTPS per gli URL `elasticsearchs://` e `opensearchs://` e per le modalità SSL da
  `require` in su; `elasticsearch://` e `prefer` sono HTTP in chiaro. `require` non controlla il
  certificato; `verify-ca` e `verify-full` sì. Un file CA e un certificato client vanno nei campi TLS.

## Privilegi

Quelli che servono alle richieste. Il navigator legge gli indici, gli alias, i data stream e i mapping
che l'utente può vedere; non ci sono schede di amministrazione.

## Cosa funziona

- **Richieste come le scrive la console Dev Tools di Kibana**: una riga come `GET /orders/_search`
  (la barra è facoltativa; metodi `GET`, `POST`, `PUT`, `DELETE`, `HEAD`), poi il suo corpo JSON.
  `_bulk` e `_msearch` accettano corpi NDJSON che finiscono a una riga vuota o alla richiesta
  successiva.
- **SQL**: tutto il resto è SQL (`_sql` su Elasticsearch, `_plugins/_sql` su OpenSearch), con colonne
  tipizzate e il cursore del server per i risultati lunghi. Gli errori mantengono il motivo del server
  e un errore SQL indica la sua posizione nell'istruzione.
- Una ricerca restituisce documenti come i risultati di MongoDB: una colonna per ogni campo di primo
  livello, con il documento selezionato come albero (`F7`). Le aggregazioni arrivano come risultati a
  sé.
- Il navigator elenca gli indici come tabelle (stato, shard e repliche nel tooltip) e gli alias e i
  data stream come viste. Un indice si apre come una ricerca di tutti i suoi documenti, letta 1.000
  righe alla volta con un point in time e `search_after` (uno scroll su OpenSearch), così la lettura
  non si ferma a 10.000 risultati. Un campo senza colonna mappata finisce in una colonna `_other`
  finale come JSON.
- **La griglia modifica i documenti**: una cella cambiata è un `_update` per `_id`, una riga nuova è
  un `_create` (un `_id` che esiste viene rifiutato dal server) o un documento generato, una riga
  eliminata è un `DELETE`. L'anteprima mostra le richieste, che girano una dopo l'altra; una che il
  server rifiuta ferma le restanti, e le precedenti restano. *Script di annullamento* rimette il
  vecchio `_source` (un documento con un `_id` generato va eliminato a mano).
- `_delete_by_query` e `_update_by_query` senza una query (o con `match_all`), e `DELETE /_all`,
  chiedono conferma prima, come un `DELETE` senza `WHERE`. Le connessioni in sola lettura eseguono
  solo le richieste e l'SQL che leggono.
- Annulla ferma una ricerca sul server. Il DDL di un oggetto indice è la richiesta che lo crea di
  nuovo.
- I dettagli sono in [FEATURES.md](../../FEATURES.md#elasticsearch-and-opensearch).

## Cosa non funziona

- Niente transazioni (niente commit manuale) e niente explain.
- Niente strumenti SQL: niente editor delle tabelle, diagramma ER, confronto degli schemi, query
  builder, importazione SQL, snapshot prima delle modifiche, né modalità terminale.
- Il visualizzatore dei dati non ha la barra di filtro e ordinamento.

## Agenti

`run_query` e `sample_rows` funzionano con le richieste della console e con SQL, e di una richiesta
che cambia dati si chiede prima. Rifiutati su Elasticsearch e OpenSearch: `explain`,
`get_er_diagram`, `propose_edits` e `query_across_databases`. Vedi [Agenti e la CLI](../../AGENTS.md).

## Limiti noti

- L'app rifiuta Elasticsearch precedente alla 7.10 e OpenSearch precedente alla 1.0: a differenza
  degli altri database, la connessione fallisce con "older than … the oldest version supported".
- Testato su Elasticsearch 8 e OpenSearch 2, non sulla 7.x.

## Testato su

La conformance suite gira su Elasticsearch 8.15.3 (login `elastic`) e OpenSearch 2.17.1: informazioni
sul server, SQL con tipi e paging, ricerche che vanno oltre una pagina, il catalogo e i mapping, il
salvataggio delle modifiche della griglia e il loro annullamento, l'annullamento di una ricerca lenta,
e gli errori del server.

Vedi anche: [Connettersi](../connect.md), [Risoluzione dei problemi](../troubleshooting.md).
