# GitSail — Product Requirements Document
**Versões planejadas:** v0.1 a v1.0  
**Status:** Draft inicial  
**Produto:** GitSail  
**Tagline:** *Navigate your Git history.*

## 1. Visão do produto

GitSail é uma ferramenta open source para interação com repositórios Git por três experiências complementares: aplicação Desktop (GUI), interface de terminal (TUI) e extensão para Visual Studio Code. Todas as interfaces devem compartilhar o mesmo modelo de domínio e comportamento de Git, evitando implementações independentes e inconsistentes.

A proposta não é reproduzir GitKraken ou GitLens. O GitSail se inspira em boas experiências existentes no ecossistema Git, mas terá identidade visual, arquitetura, fluxos e decisões próprias.

### 1.1 Problema

Desenvolvedores frequentemente alternam entre terminal, clientes gráficos e editor para executar tarefas Git. Isso fragmenta o fluxo de trabalho, cria diferenças de comportamento entre ferramentas e obriga o usuário a aprender múltiplas interfaces.

### 1.2 Proposta de valor

**One Git engine. Three ways to work.**

O usuário poderá escolher a interface adequada ao momento sem abandonar o mesmo ecossistema:

- **GitSail Desktop:** exploração visual, graph, staging, diff e operações Git.
- **GitSail TUI:** fluxo rápido, keyboard-first e terminal-first.
- **GitSail for VS Code:** contexto Git dentro do código, com foco inicial em blame e histórico.
- **GitSail Core:** domínio e operações compartilhadas.

### 1.3 Princípios do produto

1. Open source por padrão.
2. Git continua sendo a fonte de verdade.
3. Core independente de interface.
4. Keyboard-first sempre que possível.
5. GUI e TUI são experiências próprias, não cópias uma da outra.
6. Operações destrutivas devem ser explícitas e seguras.
7. Recursos avançados não devem prejudicar a experiência básica.
8. Funcionar bem em repositórios locais antes de depender de provedores remotos.
9. Formatos estruturados e contratos estáveis para permitir integrações.
10. Performance e baixo consumo de recursos são requisitos de produto.

## 2. Público-alvo

### 2.1 Primário
Desenvolvedores que utilizam Git diariamente e alternam entre terminal, editor e ferramentas gráficas.

### 2.2 Secundário
- Desenvolvedores iniciantes que se beneficiam da visualização do histórico.
- Usuários terminal-first.
- Equipes que desejam uma alternativa open source a clientes Git proprietários.
- Contribuidores interessados em Rust, Git internals, TUI e extensões de editor.

## 3. Escopo geral

### Incluído até v1.0
- Descoberta e abertura de repositórios.
- Status da working tree.
- Histórico de commits.
- Graph de commits e branches.
- Diff.
- Blame.
- Branches locais e remotas.
- Stage/unstage.
- Commit.
- Fetch, pull e push.
- Tags.
- Stash.
- Merge.
- Rebase.
- Cherry-pick.
- Reset/revert com proteções.
- TUI.
- Desktop GUI.
- Extensão VS Code.
- Configurações compartilháveis quando tecnicamente adequado.
- Integração inicial com GitHub/GitLab em funcionalidades selecionadas.

### Fora do escopo inicial
- Implementar o protocolo Git do zero.
- Hospedar repositórios Git.
- Substituir GitHub/GitLab/Forgejo.
- Editor de código completo.
- Cliente de CI/CD completo.
- Gestão completa de issues/projetos.
- IA como requisito para v1.0.
- Sincronização em nuvem proprietária do GitSail.

## 4. Arquitetura conceitual

```text
                    GitSail Domain
                          |
                    Git Provider
                          |
              +-----------+-----------+
              |                       |
         Git CLI Provider       Provider futuro
          (primeiro)          (gix/libgit2/etc.)
              |
        GitSail Services
              |
    +---------+---------+-----------+
    |                   |           |
   CLI                  TUI       Desktop
    |                               |
    +--------------------------- VS Code
```

O domínio não deve depender diretamente de comandos shell. Uma porta de acesso Git abstrairá o provider. A primeira implementação poderá utilizar o executável `git` instalado na máquina.

## 5. Stack proposta

| Componente | Tecnologia inicial |
|---|---|
| Core/Domain | Rust |
| Git Provider inicial | Git CLI |
| CLI | Rust |
| TUI | Rust + Ratatui |
| Desktop | Tauri + Vue 3 |
| VS Code | TypeScript + VS Code Extension API |
| Serialização | JSON / serde |
| Testes Rust | cargo test |
| Monorepo | Cargo workspace + apps |
| CI | GitHub Actions |
| Licença | MIT ou Apache-2.0, decisão antes do primeiro release público |

## 6. Modelo de domínio inicial

Entidades e value objects previstos:

- Repository
- RepositoryStatus
- Commit
- CommitHash
- Author
- Branch
- RemoteBranch
- Tag
- Remote
- Diff
- DiffHunk
- FileChange
- Blame
- BlameLine
- Stash
- WorkingTree
- GitOperationResult

O domínio deve evitar expor diretamente o formato textual retornado pelo Git CLI.

---

# 7. GitSail v0.1 — Core + CLI

## 7.1 Objetivo
Criar a fundação técnica do produto. Ao final da v0.1, GitSail deve conseguir abrir um repositório local e consultar informações Git através de um modelo próprio, oferecendo saída humana e JSON.

## 7.2 Funcionalidades obrigatórias

### Repositório
- Abrir repositório pelo diretório atual.
- Abrir repositório por caminho.
- Detectar diretório que não é repositório.
- Identificar root do repositório.
- Identificar branch atual.
- Detectar detached HEAD.

### Status
- Arquivos modificados.
- Arquivos adicionados.
- Arquivos removidos.
- Arquivos renomeados quando detectável.
- Arquivos untracked.
- Estado clean/dirty.

### Histórico
- Listar commits.
- Hash completo e abreviado.
- Autor.
- E-mail quando disponível.
- Data.
- Mensagem.
- Parents.
- Referências associadas quando disponíveis.
- Paginação/limite.

### Branches
- Listar branches locais.
- Listar branches remotas.
- Identificar branch atual.
- Upstream.
- Ahead/behind quando disponível.

### Diff
- Working tree diff.
- Staged diff.
- Diff de commit.
- Arquivos afetados.
- Hunks.
- Linhas adicionadas/removidas.

### Blame
- Blame de arquivo.
- Commit por linha.
- Autor.
- Data.
- Número da linha.
- Conteúdo.

### CLI
Comandos iniciais:

```bash
gitsail open .
gitsail status
gitsail log
gitsail branches
gitsail diff
gitsail blame <arquivo>
```

Todos os comandos de consulta relevantes devem aceitar `--json`.

## 7.3 Requisitos não funcionais
- Não alterar o repositório em comandos de leitura.
- Erros com códigos e mensagens estruturadas.
- Suporte inicial a Windows, Linux e macOS.
- Paths Unicode.
- Testes com repositórios fixture.
- Tempo de resposta adequado para repositórios comuns.
- Cancelamento/timeout para operações longas quando aplicável.

## 7.4 Critérios de aceite
A v0.1 está concluída quando um usuário consegue instalar o binário, entrar em um repositório e obter status, log, branches, diff e blame em formato legível e JSON, com testes automatizados cobrindo os fluxos principais.

---

# 8. GitSail v0.2 — TUI

## 8.1 Objetivo
Entregar a primeira experiência interativa do GitSail, otimizada para teclado e terminal.

## 8.2 Layout principal
- Sidebar de repositório/branches.
- Painel central de commit graph.
- Painel de detalhes.
- Visualização de diff.
- Barra inferior de atalhos.
- Command palette ou menu de ações.

## 8.3 Funcionalidades
- Abrir repositório.
- Navegar pelo histórico.
- Commit graph colorido.
- Filtrar/search commits.
- Visualizar detalhes de commit.
- Visualizar diff.
- Navegar branches.
- Checkout de branch.
- Stage/unstage.
- Criar commit.
- Fetch.
- Pull.
- Push.
- Criar/deletar branch local com confirmação.
- Visualizar tags e remotes.
- Visualizar stash.
- Blame de arquivo.

## 8.4 Navegação
Padrão inicial:
- Setas ou `j/k`: navegação.
- `Enter`: abrir/confirmar.
- `/`: busca.
- `q`: voltar/sair conforme contexto.
- `?`: ajuda.
- Atalhos de operações devem ser configuráveis posteriormente.

## 8.5 Segurança
Operações destrutivas ou difíceis de reverter exigem confirmação. A interface deve mostrar claramente branch, commit ou arquivo afetado.

## 8.6 Critérios de aceite
Um desenvolvedor deve conseguir executar seu fluxo Git cotidiano básico sem abandonar a TUI: inspecionar mudanças, stage, commit, navegar histórico, trocar branch e sincronizar com remote.

---

# 9. GitSail v0.3 — Desktop GUI

## 9.1 Objetivo
Entregar uma aplicação gráfica moderna para exploração visual e operações Git, inspirada em boas práticas de clientes Git, mas com identidade própria.

## 9.2 Experiência principal
- Dark mode como tema inicial.
- Commit graph como elemento central.
- Sidebar de branches/remotes/tags/stashes.
- Painel de mudanças.
- Diff integrado.
- Detalhes de commit.
- Busca global.
- Command palette.
- Ações acessíveis por teclado e mouse.

## 9.3 Funcionalidades
Tudo que estiver estabilizado na TUI, acrescido de:
- Seletor de repositórios recentes.
- Drag/drop somente quando a ação for inequívoca e segura.
- Diff side-by-side e unified.
- Stage por arquivo.
- Stage por hunk.
- Stage por linha quando tecnicamente estável.
- Commit composer.
- Amend.
- Graph interativo.
- Context menu em commits e branches.
- Busca por hash, mensagem, autor, branch e tag.
- Preferências.
- Atalhos configuráveis.
- Tema dark e light até o fechamento da versão.

## 9.4 UX
A interface não deve esconder o que o Git está fazendo. Operações importantes devem apresentar nomes Git reconhecíveis, evitando abstrações que dificultem o aprendizado.

## 9.5 Critérios de aceite
O Desktop deve ser utilizável como cliente Git principal para operações cotidianas e manter consistência funcional com o Core/TUI.

---

# 10. GitSail v0.4 — VS Code Blame & History

## 10.1 Objetivo
Levar contexto de autoria e histórico para dentro do editor sem transformar a extensão em um segundo cliente Git completo.

## 10.2 MVP da extensão
- Inline blame opcional.
- Autor da linha.
- Data relativa/absoluta configurável.
- Hash abreviado.
- Mensagem do commit.
- Hover com detalhes.
- Abrir detalhes do commit.
- File history.
- Line history.
- Abrir diff do commit.
- Copiar hash.
- Abrir commit na interface GitSail quando Desktop estiver disponível.

## 10.3 Integração com Core
A extensão não deve reimplementar parsing Git em TypeScript. A comunicação inicial poderá ocorrer por processo local/CLI estruturado. Uma camada IPC dedicada poderá substituir esse mecanismo futuramente sem alterar o domínio.

## 10.4 Configurações
- Habilitar/desabilitar inline blame.
- Formato do blame.
- Delay antes de exibir.
- Exibir somente na linha atual ou em múltiplas linhas.
- Formato de data.
- Caminho do binário GitSail quando necessário.

## 10.5 Critérios de aceite
Ao abrir um arquivo versionado, o usuário consegue identificar rapidamente quem alterou uma linha, quando, por quê e navegar até o commit/diff relacionado.

---

# 11. GitSail v0.5 — Operações Git avançadas

## 11.1 Objetivo
Cobrir fluxos Git intermediários/avançados de forma segura e consistente entre TUI e Desktop.

## 11.2 Funcionalidades
- Merge.
- Rebase.
- Interactive rebase.
- Cherry-pick.
- Revert.
- Reset soft/mixed/hard.
- Stash create/apply/pop/drop.
- Tags create/delete.
- Branch rename/delete.
- Force push com proteção.
- Conflict detection.
- Conflict workflow.
- Abort/continue de merge/rebase/cherry-pick.
- Worktrees.
- Reflog viewer.
- Commit amend.
- Squash/fixup.
- Copiar patch.
- Aplicar patch quando suportado.

## 11.3 Guardrails
- `reset --hard`, force push e descartes exigem confirmação reforçada.
- Mostrar impacto previsto antes da confirmação quando possível.
- Nunca mascarar um force push como push comum.
- Operações em andamento devem ter estado visível.
- Oferecer abort/continue quando Git permitir.

## 11.4 Critérios de aceite
Os principais fluxos avançados podem ser executados sem necessidade de terminal externo, preservando transparência sobre os comandos/conceitos Git envolvidos.

---

# 12. GitSail v1.0 — Plataforma integrada

## 12.1 Objetivo
Consolidar GitSail como produto estável com Core, TUI, Desktop e extensão VS Code interoperáveis.

## 12.2 Escopo funcional
- Core estável e versionado.
- CLI estável para integrações.
- TUI pronta para uso diário.
- Desktop pronto para uso diário.
- VS Code blame/history estável.
- Operações básicas e avançadas consolidadas.
- Configuração consistente.
- Documentação pública.
- Instalação simplificada.
- Atualização de aplicativo.
- Telemetria desabilitada por padrão; qualquer eventual telemetria futura deverá ser opt-in e documentada.
- Crash reports somente com consentimento explícito.
- Contribuição open source documentada.

## 12.3 Integrações remotas iniciais
Integrações serão complementares ao Git local:
- Detectar GitHub/GitLab a partir de remotes.
- Abrir commit/repository/branch no navegador.
- Visualizar Pull/Merge Requests em escopo limitado.
- Criar PR/MR poderá ser incluído se a autenticação e UX estiverem maduras.
- Funcionalidades específicas de providers devem ficar isoladas do domínio Git.

## 12.4 Distribuição
Meta:
- Windows.
- Linux.
- macOS.
- VS Code Marketplace/Open VSX para extensão.
- Binários CLI/TUI via GitHub Releases.
- Pacotes adicionais poderão ser avaliados após estabilização.

## 12.5 Qualidade v1.0
- Suite automatizada para Core.
- Integration tests contra Git real.
- Fixtures para cenários complexos.
- Testes de regressão.
- Benchmarks para graph/log em repositórios grandes.
- Crash handling.
- Logs locais.
- Documentação de troubleshooting.
- Política de compatibilidade.

## 12.6 Critérios de aceite
A v1.0 será considerada pronta quando Desktop e TUI forem capazes de atender fluxos Git cotidianos e avançados suportados, a extensão VS Code fornecer blame/history confiável, e todas as interfaces utilizarem contratos compartilhados sem divergências relevantes de comportamento.

---

# 13. Requisitos transversais

## 13.1 Performance
- Inicialização percebida rápida.
- Carregamento progressivo do histórico.
- Não bloquear UI durante operações Git longas.
- Cache somente quando não comprometer consistência.
- Virtualização de listas/graphs quando necessário.
- Benchmarks com repositórios pequenos, médios e grandes.

## 13.2 Segurança
- Nunca armazenar credenciais Git em texto puro.
- Preferir credential helpers existentes do Git/OS.
- Sanitizar argumentos enviados a processos.
- Evitar execução por shell quando possível; usar argumentos de processo.
- Não enviar conteúdo de repositórios para serviços externos sem ação explícita.
- Extensões e integrações remotas devem operar com menor privilégio possível.

## 13.3 Privacidade
GitSail deve funcionar integralmente em repositórios locais sem conta GitSail. Nenhuma conta central é requisito para v1.0.

## 13.4 Acessibilidade
- Navegação por teclado no Desktop.
- Contraste adequado.
- Estados não dependentes apenas de cor.
- Labels e tooltips.
- Suporte a zoom.
- TUI legível em terminais com diferentes capacidades de cor.

## 13.5 Internacionalização
Código, APIs e documentação técnica primária em inglês. A arquitetura deve permitir tradução da interface no futuro, sem tornar i18n requisito para as primeiras versões.

## 13.6 Observabilidade local
- Logging por níveis.
- Modo debug.
- Possibilidade de copiar diagnóstico sem incluir conteúdo sensível por padrão.
- Identificador de operação para erros complexos.

# 14. Estrutura proposta do monorepo

```text
gitsail/
├── crates/
│   ├── gitsail-core/
│   ├── gitsail-git/
│   ├── gitsail-cli/
│   ├── gitsail-tui/
│   └── gitsail-protocol/
├── apps/
│   ├── desktop/
│   │   ├── src/
│   │   └── src-tauri/
│   └── vscode/
├── fixtures/
├── docs/
│   ├── architecture/
│   ├── adr/
│   ├── contributing/
│   └── product/
├── scripts/
├── Cargo.toml
├── LICENSE
├── CONTRIBUTING.md
└── README.md
```

# 15. Contrato de comunicação

Interfaces externas ao processo Rust devem consumir estruturas versionadas.

Exemplo conceitual:

```json
{
  "schemaVersion": 1,
  "repository": {
    "path": "/projects/gitsail",
    "branch": "main"
  },
  "commits": [
    {
      "hash": "a1b2c3d4",
      "shortHash": "a1b2c3d",
      "author": {
        "name": "Developer",
        "email": "developer@example.com"
      },
      "message": "feat: initial Git engine",
      "parents": ["9f8e7d6"]
    }
  ]
}
```

Mudanças incompatíveis devem resultar em nova versão de schema.

# 16. Estratégia de Git Provider

## Fase inicial
`GitCliProvider`: invoca o Git instalado, controla argumentos e converte resultados para o domínio.

## Evolução
Avaliar gix, libgit2 ou outra implementação somente quando houver benefício mensurável em performance, distribuição, controle ou portabilidade.

A interface do provider deve permitir substituição sem reescrever TUI, Desktop ou VS Code.

# 17. Estratégia de testes

- Unit tests no domínio.
- Contract tests do provider.
- Integration tests executando Git em fixtures temporárias.
- Golden tests para parsing quando apropriado.
- Testes de estados: clean, dirty, detached HEAD, merge conflict, rebase, shallow clone, bare repo, submodules quando suportados.
- Snapshot tests seletivos na TUI.
- E2E no Desktop para fluxos críticos.
- Extension tests no VS Code.

# 18. Métricas de sucesso

Como projeto open source, métricas de adoção são secundárias à qualidade técnica inicial.

Indicadores:
- Fluxos básicos concluídos sem erro.
- Tempo de inicialização.
- Tempo para renderizar histórico.
- Crash-free sessions, somente se medido localmente ou com opt-in.
- Issues de regressão.
- Cobertura de testes do Core.
- Número e qualidade de contribuições.
- Uso recorrente por mantenedores/contribuidores.
- Feedback qualitativo sobre TUI/Desktop/VS Code.

Não haverá meta de estrelas GitHub como critério de qualidade do produto.

# 19. Roadmap consolidado

| Versão | Tema | Entrega principal |
|---|---|---|
| v0.1 | Foundation | Core Rust + Git Provider + CLI/JSON |
| v0.2 | Terminal | TUI keyboard-first |
| v0.3 | Desktop | GUI Tauri + Vue |
| v0.4 | Editor | VS Code Blame + History |
| v0.5 | Power Git | Merge, rebase, cherry-pick, conflicts e operações avançadas |
| v1.0 | Stable Platform | Core + Desktop + TUI + VS Code integrados e documentados |

# 20. Dependências entre versões

```text
v0.1 Core
 |
 +--> v0.2 TUI --------+
 |                     |
 +--> v0.3 Desktop ----+--> v0.5 Advanced Git --> v1.0
 |                     |
 +--> v0.4 VS Code ----+
```

v0.1 é bloqueadora para todas as demais. v0.2 e v0.3 devem validar o domínio antes da estabilização da v1.0. v0.4 depende de um contrato estruturado estável o suficiente para comunicação com a extensão.

# 21. Riscos principais

| Risco | Mitigação |
|---|---|
| Escopo crescer até virar clone de clientes existentes | Roadmap rígido e critérios de aceite por versão |
| Parsing frágil do Git CLI | Usar formatos machine-readable, separators seguros e testes de integração |
| Graph lento em grandes repositórios | Paginação, lazy loading, virtualização e benchmarks |
| Divergência entre GUI/TUI/VS Code | Core e contratos compartilhados |
| Operações destrutivas causarem perda de trabalho | Guardrails, confirmações e transparência |
| Complexidade multiplataforma | CI em Windows/Linux/macOS desde cedo |
| VS Code duplicar lógica Git | Extensão consumindo GitSail Core/protocolo |
| Identidade confundida com GitKraken/GitLens | UX, branding, assets e fluxos próprios |

# 22. Decisões em aberto

Itens a decidir antes ou durante a v0.1:
- MIT vs Apache-2.0.
- Nome final do repositório/organização.
- Política de versionamento pré-1.0.
- Git mínimo suportado.
- Rust MSRV.
- Formato definitivo do protocolo CLI/IPC.
- Estratégia de distribuição do binário usado pela extensão VS Code.
- Política de plugins futura.
- Suporte a submodules na v1.0 ou pós-v1.
- Escopo de GitHub/GitLab na v1.0.

# 23. Pós-v1.0 — ideias, não compromissos

- Plugin API.
- Forgejo/Gitea.
- GitHub/GitLab richer integrations.
- Visual interactive rebase avançado.
- Commit signing UX.
- Worktree workspace manager.
- Bisect visual.
- History analytics.
- Extensões para JetBrains/Neovim.
- API local/daemon.
- Temas.
- Perfis de atalhos Vim/Emacs.
- Recursos assistidos por IA, sempre opcionais e separados do funcionamento básico.

# 24. Definição de pronto do produto v1.0

GitSail v1.0 deve permitir que um desenvolvedor use Desktop ou TUI como cliente Git diário, consulte autoria/histórico diretamente no VS Code, alterne entre essas experiências sem encontrar comportamentos contraditórios e mantenha controle explícito sobre cada operação executada no repositório.

A experiência final deve materializar a promessa:

**GitSail — Navigate your Git history.**
