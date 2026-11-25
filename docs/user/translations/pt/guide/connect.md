<!-- source: 284fda1cc34a -->
[← Guia do usuário](README.md)

# Conectar a um banco de dados

![Escolhendo o banco de dados para uma nova conexão](../../screenshots/connections.png)

## A partir de uma URL

1. Clique em **Nova conexão** (ou no `+` do navegador).
2. Cole a URL abaixo dos logotipos: `postgres://`, `mysql://`, `sqlserver://`, `clickhouse://`,
   `libsql://`, `mongodb://`, `redis://`, `rediss://`, `elasticsearch://`, `opensearch://`, uma
   URL JDBC ou uma string ADO.NET / ODBC. O formulário abre preenchido para esse banco de dados.
3. Coloque a senha no campo **Senha**, não na URL: o formulário avisa se a URL contém uma.
4. Clique em **Testar conexão**, depois em **Salvar**.

Ou escolha o banco de dados pelos logotipos (a busca no alto aceita `Enter` para o primeiro
resultado). A escolha define o driver; **Voltar** retorna aos logotipos.

## O formulário

O formulário tem quatro páginas: **Geral**, **Aparência e segurança**, **Rede** e **Inicialização**.
Um ponto depois do nome de uma página indica que ela contém configurações.

- **Geral**: host, porta, usuário, banco de dados e de onde vem a senha (abaixo).
- **Aparência e segurança**: o ambiente (Desenvolvimento, Teste ou Produção), uma cor, a caixa
  **Somente leitura (recusa alterações nos dados)**, as caixas que impedem exportações e edição de
  dados, e o acesso dos agentes. Veja [Segurança](safety.md) e [Agentes](agents.md).
- **Rede**: túnel SSH, modo SSL e certificados, proxy, port forward e comandos em volta da
  conexão.
- **Inicialização**: um arquivo de variáveis, um schema ou fuso horário padrão, e instruções que
  rodam a cada conexão.

## Senhas

Em **Geral**, a senha vem de uma destas fontes:

- **Salva no chaveiro**: o chaveiro do sistema. **Testar conexão** a envia só ao servidor para o
  qual ela foi salva: se você mudar o host, a porta ou os servidores SSH do caminho, o app pede que
  você digite a senha para esse teste. Outro banco de dados do mesmo servidor PostgreSQL ou MySQL
  não é outro servidor.
- **Perguntar sempre**: digitada a cada conexão. Agentes não conseguem abrir uma conexão assim.
- **Arquivo .pgpass** (só PostgreSQL; o host, a porta, o banco de dados e o usuário da conexão
  escolhem a linha, como faz a libpq).
- **Comando de shell**: um comando que imprime a senha.

Qualquer campo pode ler uma variável de ambiente escrita como `${env:NAME}`. Os gerenciadores de
senhas (1Password, Bitwarden, HashiCorp Vault, AWS Secrets Manager) estão na mesma lista de todo
banco de dados cujo formulário pede uma senha ou um token; nenhum é limitado a um só banco de
dados. Informe a referência do segredo: o app monta o comando de shell a partir dela (`op read`,
`bw get password`, `vault kv get`, `aws secretsmanager get-secret-value`) e o executa a cada
conexão. A ferramenta de linha de comando deles precisa estar instalada e com a sessão iniciada.
Nada disso foi executado contra uma conta real de 1Password, Bitwarden, Vault ou AWS: só os
comandos que o app monta e o próprio comando de shell são testados.

Alguns bancos de dados entram sem nenhuma senha, e a mesma lista os oferece: um token da
ferramenta de linha de comando de uma nuvem (AWS IAM e Google Cloud IAM para PostgreSQL e MySQL,
Microsoft Entra ID para esses dois e para o SQL Server), o login do Windows para o SQL Server, e um
**Ticket Kerberos (GSSAPI)** para PostgreSQL no Windows e no Linux. Execute `kinit` antes, ou
defina `KRB5CCNAME` ou `KRB5_CLIENT_KTNAME`; no Windows o ticket é o da sua sessão. O ticket é a
única coisa que vai para o servidor, e um teste sem ele diz o que executar. A
[página do PostgreSQL](databases/postgresql.md) tem os detalhes, e o que é e o que não é testado:
um ticket real já fez login a partir do Linux, ainda não pelo SSPI do Windows.

### Senha mestra

Uma senha mestra mantém as senhas salvas em um arquivo criptografado, em vez do chaveiro.

1. Abra as Configurações (`Ctrl+,`) › **Senha mestra**.
2. Clique em **Definir uma senha mestra…**, digite-a duas vezes (no mínimo 8 caracteres) e
   confirme.
3. Para voltar ao chaveiro, clique em **Desativar…** na mesma página. **Bloquear agora** bloqueia
   na hora.

Ela não pode ser recuperada: sem ela, as senhas salvas se perdem. Agentes não conseguem digitá-la
(veja [Solução de problemas](troubleshooting.md#um-agente-não-vê-uma-conexão)).

## TLS

Na página **Rede**, o **Modo SSL** aceita os valores da libpq: `disable`, `prefer`, `require`,
`verify-ca` e `verify-full`. `prefer` e `require` criptografam sem verificar o certificado;
`verify-ca` e `verify-full` o verificam, então pode ser preciso um **Certificado CA (PEM)**. Para
servidores que autenticam clientes por certificado, informe o **Certificado de cliente (PEM)** e a
**Chave do cliente (PEM)** (PostgreSQL e MySQL).

## Túnel SSH

![Um túnel SSH na página Rede](../../screenshots/guide/connect-network.png)

1. Na página **Rede**, marque **Conectar por um túnel SSH**.
2. Preencha **Host SSH**, a porta e **Usuário SSH**.
3. Autentique com uma senha, um **Arquivo da chave privada** (e sua frase secreta) ou o **Agente
   SSH**. Para um bastion, adicione um **Jump host** (**Passando antes por um jump host**).
4. Clique em **Testar conexão**.

A chave do servidor é verificada em `~/.ssh/known_hosts`: um servidor desconhecido é aprendido na
primeira vez, e uma chave que mudou desde então é recusada. Um **Perfil de rede** compartilha um
túnel e uma configuração de SSL entre conexões.

## Proxy e port forward

Também em **Rede**: um proxy SOCKS5 ou HTTP (`socks5://user:password@host:1080`,
`http://host:3128`), ou um port forward por `kubectl` ou por uma sessão do AWS Systems Manager (a
CLI precisa estar instalada e com a sessão iniciada). Um port forward vai sozinho: remova o túnel
SSH ou o proxy.

## Próximo passo

Detalhes de cada banco de dados: [Bancos de dados](databases/README.md). Login em nuvens, poolers,
importações do DBeaver, DataGrip e pgAdmin (incluindo conexões do Db2) e exportação de conexões
estão em [FEATURES.md](../FEATURES.md#connections). Se não conectar, veja
[Solução de problemas](troubleshooting.md#erros-de-conexão).
