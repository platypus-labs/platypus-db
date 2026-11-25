<!-- source: d4ffd9bf1ac2 -->
[← Guia do usuário](README.md)

# Executar consultas e ler os resultados

![Autocompletar de JOIN a partir das chaves estrangeiras](../../screenshots/autocomplete.png)

## Executar uma instrução

1. Dê um duplo clique em uma conexão (ou em uma tabela) para abrir um editor SQL.
2. Digite uma instrução. `Ctrl+Space` completa palavras-chave, tabelas e colunas; `JOIN ` oferece as
   tabelas relacionadas a partir das chaves estrangeiras.
3. Pressione `Ctrl+Enter` para executar a instrução sob o cursor (ou a seleção), ou `Alt+X` para
   executar o script inteiro. `Ctrl+\` executa em uma nova aba de resultados.
4. `Ctrl+.` cancela uma instrução em execução.

As instruções terminam em um `;`, ou em uma linha em branco antes de uma palavra que começa outra.
Quando uma instrução falha, o erro é indicado no texto e o painel de erros oferece **Ir para o
erro**. `Ctrl+Shift+F` formata o script; `Ctrl+S` o salva como um arquivo `.sql`.

## Parâmetros e variáveis

`:id` e `?` em uma instrução, e variáveis `${name}`, são pedidos em uma caixa de diálogo antes da
execução e ficam em um painel Variáveis. O valor de uma `${name}` entra no SQL como está, então uma
instrução com variáveis é verificada de novo, já preenchida (veja [Segurança](safety.md)).

## Ler os resultados

- A grade lê as linhas uma página por vez. Clique no cabeçalho de uma coluna para ordenar: isso
  escreve o `ORDER BY` no editor e executa de novo; um segundo clique o inverte, um terceiro
  restaura o da própria consulta.
- Abra uma tabela pelo navegador para percorrê-la; um `WHERE` digitado no editor a filtra.
- `F7` abre o painel de valor: textos longos, JSON e XML formatados, hex e imagens de colunas
  binárias.
- Uma célula que contém um arquivo (um PDF, uma imagem, um ZIP e outros, reconhecidos pelos seus
  bytes) mostra o tipo e o tamanho, como `PDF · 2.3 MB`. O menu da grade oferece **Salvar PDF…** (ou
  o tipo do arquivo) e, com várias células de uma coluna, **Salvar N arquivos em uma pasta…**. O
  painel de valor tem o mesmo botão.
  Uma célula de texto que contém um arquivo (uma URL `data:`, base64 ou hex) não tem rótulo na
  grade, o que exigiria ler todas as células de texto para saber: o menu da grade e o painel de
  valor a mostram, para as células que você seleciona.
- `Ctrl+F` procura nas linhas carregadas; `Ctrl+Shift+C` copia a seleção com os cabeçalhos.
- Um resultado pode passar da grade para um gráfico, pela barra de status.

![Células que contêm arquivos, com seu tipo e tamanho](../../screenshots/guide/file-cell.png)

## Entender uma consulta lenta

Pressione `Ctrl+Shift+E` em uma instrução para ver o plano dela. Veja
[FEATURES.md](../FEATURES.md#query-plans-and-performance).

## Mais

Scripts salvos, snippets, histórico, modo vim, execução de um script em várias conexões, o query
builder e a análise de dados estão em [FEATURES.md](../FEATURES.md#sql-editor). A grade de
resultados está em [FEATURES.md](../FEATURES.md#results-and-the-data-viewer).
