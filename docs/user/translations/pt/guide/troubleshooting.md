<!-- source: 25c359215e93 -->
[← Guia do usuário](README.md)

# Solução de problemas e FAQ

Comece por **Testar conexão** no formulário da conexão: ele relata a falha com a mensagem do próprio
servidor e informa a versão do servidor quando isso ajuda.

## Erros de conexão

### Não foi possível alcançar o host

Verifique primeiro o host e a porta (o formulário diz "O host não foi informado." ou "A porta deve
ser um número de 1 a 65535."). Depois olhe o que há entre você e o servidor: um túnel SSH, um proxy,
um port forward, ou um comando em volta da conexão que sobe uma VPN (página **Rede**). Cada um
acrescenta a própria mensagem, abaixo.

### TLS

Na página **Rede**, o **Modo SSL** decide o que é verificado (veja [Conectar](connect.md#tls)).

- Um servidor que recusa uma conexão sem criptografia precisa de `require` ou mais; um servidor com
  um certificado em que o seu computador não confia precisa de `verify-ca` ou `verify-full` com o
  **Certificado CA (PEM)**, ou de `require` para criptografar sem verificar.
- "invalid certificate" e "invalid root certificates" indicam um arquivo PEM que não pode ser lido:
  confira se é o certificado em si, e não uma chave.
- "a client certificate needs both the certificate and its key": preencha **Certificado de cliente
  (PEM)** e **Chave do cliente (PEM)** juntos. "invalid client certificate or key" indica que eles
  não combinam ou não são PEM.
- Redis, Elasticsearch e Cassandra usam TLS a partir de um modo SSL `require` ou mais.

### SSH

Os erros começam com `SSH <user>@<host>:<port>:`.

- `no answer within 10 s` ou `cannot reach <host>:<port>`: o servidor SSH (ou o proxy na frente
  dele) não pode ser alcançado daqui.
- `cannot read the key <path>`: o **Arquivo da chave privada** não existe, não pode ser lido, ou
  precisa da sua frase secreta.
- `no SSH agent is running`: inicie o seu agente SSH, ou use um arquivo de chave ou uma senha.
- Uma senha ou chave recusada é a resposta do servidor: confira **Usuário SSH** e o método.
- Um servidor cuja chave mudou desde a primeira conexão é recusado. As chaves são as de
  `~/.ssh/known_hosts`, compartilhado com o OpenSSH: remova a linha antiga de lá se a mudança for
  esperada.

### Proxy

Os erros citam o proxy, como `SOCKS5 proxy host:1080: …` ou `HTTP proxy host:3128: …`.

- `no answer within 10 s`: o proxy não responde.
- `the proxy asks for a user and password, or refused them`: escreva-os na URL
  (`http://user:password@host:3128`).
- `cannot reach <host>:<port>: …`: o proxy respondeu, mas não conseguiu alcançar o banco de dados.
- Uma URL que não é entendida é relatada como `proxy "<url>": …`. Um port forward vai sozinho:
  remova o túnel SSH ou o proxy.

### Chaveiro

- `keychain error: …`, ou "the system has no keychain to save the password in": no Linux é preciso
  que um Secret Service, como o GNOME Keyring ou o KWallet, esteja em execução. Sem um, defina uma
  senha mestra (Configurações › **Senha mestra**) ou escolha **Perguntar sempre** para a senha.
- "Nenhuma senha está salva para esta conexão: ela será salva no chaveiro." é só um aviso.
- As senhas importadas de outra ferramenta sem a senha (o DBeaver com a própria senha mestra, por
  exemplo) são pedidas no primeiro uso.

### Senha mestra

- "PlatypusDB está bloqueado" ou "Digite a senha mestra para usá-las.": digite-a e clique em
  **Desbloquear**. O bloqueio por inatividade fica em Configurações › **Segurança**; as conexões de
  produção se reconectam quando são usadas.
- Ela não pode ser recuperada. Se for perdida, as senhas salvas se perdem: defina as senhas de novo
  em cada conexão.
- Para voltar ao chaveiro, Configurações › **Senha mestra** › **Desativar…** (ele pede a senha
  atual).

## Um download de driver falha

DuckDB, Snowflake, BigQuery, Databricks, Trino, outros drivers ADBC, Oracle e IBM Db2 baixam a sua
biblioteca na primeira vez em que são usados. O popup diz "Baixando o driver `<name>`…" com um
**Cancelar**, e uma falha mostra uma mensagem com **Tentar de novo**.

- `the download failed (check the internet connection)` ou `cannot reach the driver registry`:
  não há rota até o registry; verifique a rede, o proxy ou o firewall, depois **Tentar de novo**.
- `the downloaded driver is corrupted (its SHA-256 does not match)`: o arquivo foi alterado ou
  cortado no caminho; tente de novo e relate o problema se ele persistir.
- `no <name> driver for this platform`, ou "PlatypusDB downloads DuckDB on Windows, macOS and Linux,
  on x64 and ARM processors; not on this system.": não há build para o seu sistema.
- `the driver's license (…) does not allow downloading it here`: o app não baixa drivers cuja
  licença o proíbe.
- O IBM Db2 só é alcançado por ODBC no Windows, por enquanto; registrar o driver dele pede permissão
  de administrador (Controle de Conta de Usuário). Responda sim, ou o driver não é registrado.
- **Cancelar** deixa o formulário utilizável; a oferta volta em **Testar conexão** ou na primeira
  abertura.

Configurações › **Drivers** lista o que foi baixado, remove um driver e mostra os drivers ODBC do
sistema. Uma remoção que o Windows recusou é concluída na próxima inicialização.

## Um agente não vê uma conexão

Siga esta lista de cima para baixo.

1. **O acesso está desativado.** O agente responde "`<name>` is not available to agents". Em
   Configurações › **Agentes** › **Segurança**, defina a conexão como **Somente leitura** ou
   **Leitura e gravação**. Uma conexão vinda de um projeto compartilhado começa em **Desativado** no
   seu computador.
2. **Uma senha mestra.** O agente responde "the saved password of `<name>` is locked by
   PlatypusDB's master password, which agents cannot enter". Dê a essa conexão um comando de senha
   ou um `.pgpass`, ou desative a senha mestra.
3. **A senha é pedida a cada vez.** O agente responde "`<name>` asks for its password at each
   connection, and agents cannot type it". Salve-a no chaveiro, ou use um `.pgpass` ou um comando
   de senha.
4. **Gravações recusadas.** As conexões de produção e as somente leitura nunca passam de somente
   leitura; nas outras, a alteração espera a sua resposta no app ([Agentes](agents.md)).
5. **O cliente não foi reiniciado** depois de **Instalar**, ou a entrada está faltando:
   Configurações › **Agentes** › **Configuração** diz se o PlatypusDB está no arquivo do cliente.
6. **Uma cópia portátil ou outro perfil.** O agente lê as conexões do perfil a partir do qual foi
   iniciado; um cliente configurado com o app instalado não vê a pasta `data` de uma cópia portátil.

## A cópia portátil

- É um executável com uma pasta `data` vazia ao lado. Se a pasta não estiver lá na inicialização, o
  app usa as pastas do sistema, então uma pasta `data` renomeada parece conexões perdidas: dê a ela
  o nome de volta.
- Tudo fica em `data`, inclusive os logs; a pasta se move com o executável. As senhas salvas ficam
  no chaveiro do sistema do computador, a menos que uma senha mestra as mantenha na pasta também: em
  outro computador, defina a senha mestra antes ou digite as senhas de novo.
- Ela não se atualiza sozinha: substitua o executável pelo novo. A tela de boas-vindas mostra
  `portable` ao lado da versão.

## Onde ficam os dados e os logs

O app guarda conexões e configurações (`connections.toml`, `settings.toml` e `secrets.vault` com uma
senha mestra) na sua pasta de configuração, e histórico, scripts, drivers baixados e logs na sua
pasta de dados, ambas chamadas `dev.platypusdb.desktop`:

| Sistema | Pasta de configuração | Pasta de dados |
|---|---|---|
| Windows | `%APPDATA%\dev.platypusdb.desktop` | `%APPDATA%\dev.platypusdb.desktop` |
| macOS | `~/Library/Application Support/dev.platypusdb.desktop` | a mesma |
| Linux | `~/.config/dev.platypusdb.desktop` | `~/.local/share/dev.platypusdb.desktop` |

Uma cópia portátil guarda tudo na sua pasta `data`, e a variável de ambiente `PLATYPUS_DATA_DIR`
coloca tudo na pasta que ela indica.

![Configurações, a página Diagnóstico](../../screenshots/guide/diagnostics.png)

O log é `logs/platypus.log` na pasta de dados: uma linha por inicialização, erro e falha, rotacionado
em 1 MB com dois arquivos mais antigos (`platypus.1.log`, `platypus.2.log`). Configurações ›
**Diagnóstico** mostra a pasta (**Copiar**) e **Salvar pacote de diagnóstico…**, um zip com a versão,
o sistema, as configurações sem senhas e os logs, para anexar a um relatório de bug. Nada é enviado a
lugar nenhum; leia os logs antes de compartilhar, pois um erro pode citar um servidor ou uma tabela.
Depois de uma falha, a próxima inicialização oferece o mesmo pacote.

## Atualizações

- Na inicialização, o app procura uma release mais nova e mostra "PlatypusDB `<version>` está
  disponível" com **Mais tarde** e **Instalar e reiniciar**.
- Uma cópia portátil só avisa: substitua o executável. Uma instalação por um gerenciador de pacotes
  (Flatpak, o AUR) avisa e deixa a atualização para ele.
- Os drivers baixados se atualizam à parte: Configurações › **Drivers** › **Procurar atualizações**,
  ou as configurações **Procurar atualizações de drivers ao iniciar** (ativada, no máximo uma vez
  por dia) e **Instalar as atualizações de drivers automaticamente** (desativada). Os drivers
  embutidos se atualizam com o app.

## FAQ

**Posso usar o PlatypusDB offline?** Sim, com um banco de dados local. Os downloads (drivers,
atualizações) precisam de rede.

**Onde relato um bug?** No [issue tracker](https://github.com/platypus-labs/platypus-db/issues),
com o pacote de diagnóstico se puder.

**Onde estão os atalhos?** [SHORTCUTS.md](../SHORTCUTS.md), e Configurações › Atalhos de teclado
para alterá-los.
