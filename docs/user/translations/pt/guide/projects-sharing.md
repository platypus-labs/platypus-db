<!-- source: aa482a6e3843 -->
[← Guia do usuário](README.md)

# Projetos e compartilhamento

Um projeto é um grupo de conexões, scripts e abas abertas, mostrado na barra à esquerda. Os
detalhes estão em [FEATURES.md](../FEATURES.md#workspace) e
[FEATURES.md](../FEATURES.md#sharing-and-sync).

## Criar um projeto

1. Execute **Novo projeto…** na paleta de comandos (`Ctrl+K`).
2. Dê um nome a ele e escolha as conexões que aparecerão nele. Uma conexão também pode ser
   adicionada pelo próprio formulário: **Projetos**, no fim da página **Aparência e segurança**.
3. `Ctrl+1` mostra todas as conexões, `Ctrl+2` a `Ctrl+9` os projetos na ordem da barra.

## Compartilhar uma consulta

1. Em uma aba do editor SQL, execute **Partilhar consulta…** (menu da aba ou paleta de comandos).
2. Copie o link `platypus://…`, ou salve um arquivo `.platypus`. Ele contém o SQL, o título e, se
   você pedir, os valores dos parâmetros; nomeia o banco de dados, mas nunca uma senha.
3. O colega o abre com **Abrir uma ligação partilhada…** ou **Abrir uma consulta partilhada…**, ou
   soltando o arquivo na janela. O app encontra a conexão salva correspondente ou oferece uma nova
   já preenchida, e mostra a consulta sem executá-la.

## Compartilhar um projeto com uma equipe

1. Clique com o botão direito no projeto, na barra, e escolha **Compartilhar projeto…**, depois
   escolha uma pasta que a equipe mantém no Git ou em uma unidade compartilhada. Ela contém as
   conexões (sem credenciais) e os scripts.
2. Cada colega clica em **Abrir projeto compartilhado…** na tela de boas-vindas, escolhe a pasta e
   informa as próprias senhas.
3. Depois de uma mudança na pasta, execute **Ler a pasta compartilhada novamente** no menu do
   projeto.

Seguro por padrão: uma conexão nova vinda de um projeto compartilhado dá aos agentes **nenhum
acesso** no seu computador até você escolher outra coisa, e as instruções que o arquivo executa a
cada conexão são mostradas antes e só rodam depois que você aprova esse texto exato.

## Levar conexões para outro computador

Na tela de boas-vindas, **Exportar conexões** grava um arquivo `.platypusconn`: escolha as conexões
e, se quiser, uma senha de no mínimo 8 caracteres, que criptografa o arquivo e permite que ele leve
as senhas salvas. No outro computador, **Importar conexões** o lê. Sem uma senha, o arquivo não
contém nenhuma credencial.

Para manter configurações, conexões e scripts em sincronia entre os seus computadores, use
Configurações › **Sincronização** com uma pasta em uma unidade na nuvem. Veja
[FEATURES.md](../FEATURES.md#sharing-and-sync).

## Scripts e Git

Quando a pasta de scripts de um projeto é um repositório Git, o painel Scripts mostra o branch e as
alterações de cada script, e oferece **Commit…**, **Pull** e **Push**. O Git precisa estar
instalado.
