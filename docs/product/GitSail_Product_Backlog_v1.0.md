# GitSail — Product Backlog v1.0

**Navigate your Git history.**

- Documento: v1.0, baseline de planejamento de 17/09/2026.
- Produto coberto: v0.1 → v1.0; versão do documento não significa produto implementado.
- Status: pronto para revisão de planejamento; nenhuma história está implementada ou aceita por este documento.
- Idioma: português para planejamento com o responsável pelo produto; código, APIs e documentação técnica primária em inglês, conforme PRD §13.5.

## 1. Fontes e precedência

Este backlog deriva dos documentos originais fornecidos pelo usuário, preservados sem alteração:

- [PRD v0.1–v1.0](GitSail_PRD_v0.1-v1.0.md): fonte de escopo, comportamento e milestones.
- [SAD e ADRs v0.1](../architecture/GitSail_SAD_and_ADRs_v0.1.md): fonte de estrutura, fronteiras e decisões arquiteturais.
- [Logo](../../assets/branding/logo_gitsail.png), [mockup Desktop](../../assets/mockups/gitsail_gui_mockup.png) e [mockup TUI](../../assets/mockups/gitsail_tui_mockup.png): referências visuais originais da conversa, sem redesenho nesta entrega.
- Conversa “Recriar GitKraken E GitLens”, identificador `6aab68a4-f364-83e9-a1ac-dea14207a96c`, e instruções atuais: identidade GitSail, mascote náutico, vela + Git graph e backlog antes da implementação.

O backlog detalha requisitos; não substitui PRD/SAD nem transforma exemplos conceituais em contratos finais. Critérios mais específicos, prioridades e decomposição são propostas de planejamento derivadas das fontes, não decisões retroativamente atribuídas a elas. Cada épico informa suas seções-fonte, herdadas pelas histórias. Alterações futuras de escopo devem atualizar a fonte pertinente e manter rastreabilidade.

### 1.1 Conciliações aplicadas

1. **Core e monorepo:** `gitsail-core` no PRD §14 é refinado pelo SAD §§6–7 em `gitsail-domain` e `gitsail-application`. Core neste backlog é o conceito compartilhado, não um terceiro crate adicional. Demais crates/apps seguem o SAD.
2. **CLI primeiro:** envelope do PRD §15 e do SAD §14 são exemplos. `schemaVersion` é obrigatório; formato final, cursor e correlação ainda exigem decisão. Não há daemon obrigatório (ADR-012).
3. **Amend:** entra no Desktop v0.3 porque o PRD §9.3 já o exige; a v0.5 consolida a experiência avançada e TUI.
4. **Tags/stash:** inspeção disponível na TUI v0.2; mutações são v0.5. Worktrees básicos são v0.5, gestor avançado de workspace continua pós-v1.0.
5. **Distribuição:** v0.1 precisa de binário instalável (PRD §7.4), v0.3 de Desktop utilizável e v0.4 de extensão instalável. v1.0 consolida canais, atualização e compatibilidade.
6. **Interatividade avançada:** rebase interativo básico é v0.5; editor visual avançado continua ideia pós-v1.0. Stage por linha e criação PR/MR permanecem condicionais.
7. **Temas:** dark/light são compromisso v0.3; sistema extensível de temas não é antecipado da lista pós-v1.0.
8. **Integração remota:** GitHub/GitLab complementam Git local. Forgejo/Gitea no contexto arquitetural não amplia o roadmap comprometido. Gestão completa de issues/CI não entra.
9. **Identidade:** nome/tagline e direção visual são preservados; não se declara concluída a verificação definitiva de disponibilidade de marca discutida na conversa.

## 2. Convenções de planejamento

- IDs `EPIC-01`–`EPIC-26` e `US-xxx` são estáveis a partir desta baseline. Inserções futuras recebem novos IDs; itens removidos mantêm um registro de descontinuação. Não renumerar nem reciclar IDs para ordenar trabalho.
- Versão alvo indica o milestone em que a história deve estar aceita. Quando uma capacidade evolui antes, o critério explicita a entrega inicial. Não são datas ou estimativas de esforço.
- **Must:** necessário para cumprir a versão indicada; **Should:** importante, adiado somente com justificativa de impacto; **Could:** opcional/condicional; **Won’t até v1.0:** fora desta baseline, registrado na seção de exclusões. Must na v0.5 não é pré-requisito implícito da v0.1.
- Dependências listadas são bloqueios diretos de aceitação; dependências transitivas não são repetidas. A ordem numérica de épicos/histórias não é ordem de execução.
- Histórias transversais e DoD aplicam-se em cada release conforme escopo, mesmo quando a consolidação recebe uma história em versão posterior. Gates de release adicionais estão na seção 5.
- Critérios são verificáveis; detalhes ainda não decididos têm uma atividade explícita de decisão. Não há SLA numérico, biblioteca auxiliar, licença ou protocolo definitivo inventado nesta baseline.
- Toda história começa com status **Planejada**. Aprovar o backlog não marca histórias como Done e não inicia implementação.

## 3. Definition of Done global — DOD-G

Uma história só está Done quando todos os pontos aplicáveis abaixo e sua DoD específica têm evidência vinculada à issue/PR. Não aplicável exige justificativa registrada.

1. Critérios de aceite demonstrados, incluindo estado vazio, erro e cancelamento quando pertinentes; revisão por mantenedor concluída.
2. Comportamento corresponde ao PRD e ao SAD; decisões novas registradas sem sobrescrever silenciosamente ADRs. Domain puro, Application depende de ports, parsing na infraestrutura e UI sem execução Git direta.
3. Testes adequados ao risco passam: unidade para regras puras, contrato/integração contra Git real para semântica, teste de interação para fluxo crítico. Não basta compilar nem adicionar testes que só repetem implementação.
4. Windows, Linux e macOS validados para componentes aplicáveis, incluindo paths Unicode e limitações declaradas. CI pertinente com format/lint/build/test passa; documentação/assets usam revisão de conteúdo e integridade em vez de testes de código artificiais.
5. Leitura não modifica working tree, index ou refs como efeito não solicitado. Mutação declara intenção, risco e alvo, revalida pré-condições, respeita serialização/locks e invalida estado. Confirmação reforçada para ações destrutivas; cancelamento não promete rollback inexistente.
6. Nenhum segredo ou conteúdo completo é exposto por padrão em logs. Conteúdo Git/forge é não confiável. Processos usam argumentos diretos; credenciais são delegadas ao Git/OS. Git local não exige conta central nem telemetria.
7. UI permanece responsiva em operações longas; recursos/processos são limpos. Histórico é paginado; cache não substitui Git como fonte de verdade e resultados obsoletos são descartados.
8. Contratos externos têm DTOs explícitos, versão, exemplos e erros estáveis. Mudança incompatível é versionada e verificada contra consumidores suportados.
9. Ações de UI têm foco/atalhos ou alternativa de teclado adequada, labels e estados além de cor. Strings/conteúdo não criam injeção terminal/markup.
10. Documentação de uso, limites, recuperação e compatibilidade atualizada quando afetada, com documentação técnica primária em inglês. Assets têm origem e uso registrados.
11. Evidências, riscos residuais e limitações constam da revisão. Nenhum defeito conhecido que viole um critério obrigatório fica oculto; dependências estão aceitas e não há placeholders tratados como funcionalidades concluídas.

Cada DoD específica abaixo significa **DOD-G + verificação adicional descrita**, nunca dispensa os itens globais.

## 4. Catálogo de épicos


| ID | Épico | Histórias | Versões | Fonte principal |
|---|---|---:|---|---|
| [EPIC-01](#epic-01) | Project Foundation | 5 | v0.1 | PRD §§4–6, 14, 16; SAD §§4–13, 19, 37–40; ADR-001–004 e ADR-009 |
| [EPIC-02](#epic-02) | Repository Discovery | 4 | v0.1, v0.2 | PRD §§7.2–7.3, 17; SAD §§8–12, 21, 30–31 |
| [EPIC-03](#epic-03) | Working Tree & Status | 5 | v0.1, v0.2, v0.3 | PRD §§7.2–7.3, 8.3, 9.3; SAD §§8–10, 20–23, 26 |
| [EPIC-04](#epic-04) | Commit History | 5 | v0.1, v0.2, v0.4 | PRD §§7.2, 8.3, 9.3, 10.2; SAD §§8–9, 23–25 |
| [EPIC-05](#epic-05) | Branches & References | 5 | v0.1, v0.2, v0.5 | PRD §§7.2, 8.3, 11.2; SAD §§8–10, 20, 26 |
| [EPIC-06](#epic-06) | Diff Engine | 6 | v0.1, v0.3, v0.5 | PRD §§7.2, 9.3, 11.2; SAD §§8–11, 26, 30–32 |
| [EPIC-07](#epic-07) | Blame Engine | 4 | v0.1, v0.4 | PRD §§7.2–7.3, 10.2–10.4; SAD §§8–11, 23, 26 |
| [EPIC-08](#epic-08) | CLI & Protocol | 5 | v0.1, v1.0 | PRD §§7.2–7.4, 15, 22; SAD §§14–15, 19, 36; ADR-008 e ADR-012 |
| [EPIC-09](#epic-09) | TUI Foundation | 5 | v0.2 | PRD §§8.1–8.5, 13.4; SAD §§18, 21–22, 26; ADR-005 |
| [EPIC-10](#epic-10) | TUI Git Workflow | 6 | v0.2 | PRD §§8.2–8.6; SAD §§9, 18, 20 |
| [EPIC-11](#epic-11) | Desktop Foundation | 5 | v0.3 | PRD §§9.1–9.5, 13.4; SAD §§17, 21–22, 26; ADR-006 |
| [EPIC-12](#epic-12) | Desktop Git Workflow | 8 | v0.3 | PRD §§9.2–9.5, 8.3; SAD §§9, 17, 20 |
| [EPIC-13](#epic-13) | Commit Graph | 4 | v0.2, v0.3 | PRD §§8.2–8.3, 9.2–9.3, 13.1; SAD §§24–25, 32; ADR-011 |
| [EPIC-14](#epic-14) | VS Code Foundation | 4 | v0.4 | PRD §§10.1–10.4, 22; SAD §§14–16, 26, 33, 36; ADR-007–008 e ADR-012 |
| [EPIC-15](#epic-15) | VS Code Blame & History | 6 | v0.4 | PRD §§10.2–10.5; SAD §§16, 23, 26 |
| [EPIC-16](#epic-16) | Merge & Conflicts | 5 | v0.5 | PRD §§11.1–11.4; SAD §§9, 19–22, 26 |
| [EPIC-17](#epic-17) | Rebase & History Editing | 8 | v0.5 | PRD §§9.3, 11.2–11.3, 23; SAD §§9, 20–22, 26 |
| [EPIC-18](#epic-18) | Stash, Tags & Worktrees | 5 | v0.2, v0.5 | PRD §§8.3, 11.2–11.3, 23; SAD §§8–9, 20–21, 26 |
| [EPIC-19](#epic-19) | Remote Operations | 5 | v0.2, v0.5, v1.0 | PRD §§8.3, 11.2–11.3, 13.2; SAD §§9, 19–20, 26–28; ADR-010 |
| [EPIC-20](#epic-20) | GitHub/GitLab Integration | 4 | v1.0 | PRD §§3, 12.3, 13.2–13.3, 22–23; SAD §§3, 27, 33–34; ADR-010 |
| [EPIC-21](#epic-21) | Settings & Personalization | 5 | v0.3, v0.4, v1.0 | PRD §§9.3, 10.4, 12.2, 13.4–13.5; SAD §29 |
| [EPIC-22](#epic-22) | Security & Safety | 5 | v0.1, v0.2, v1.0 | PRD §§7.3, 8.5, 11.3, 12.2, 13.2–13.3, 13.6; SAD §§19–20, 26–28, 30, 33; ADR-009–010 |
| [EPIC-23](#epic-23) | Performance & Large Repositories | 4 | v0.1, v0.2, v0.3, v1.0 | PRD §§13.1, 17–18; SAD §§21–26, 31–32 |
| [EPIC-24](#epic-24) | Testing & Quality | 5 | v0.1, v0.4, v1.0 | PRD §§7.3–7.4, 12.5–12.6, 17; SAD §§31, 35, 37, 40 |
| [EPIC-25](#epic-25) | Distribution & Updates | 5 | v0.1, v0.3, v0.4, v1.0 | PRD §§7.4, 12.2, 12.4–12.5, 22; SAD §§30, 35–36, 39 |
| [EPIC-26](#epic-26) | Documentation & Open Source | 5 | v0.1, v1.0 | PRD §§1–3, 5, 13.5, 14, 18, 21–24; SAD §§6, 34, 37–40; ADR-001–012; decisões de branding da conversa |

**Total: 26 épicos e 133 histórias.**

### Distribuição por milestone e prioridade

| Versão | Must | Should | Could | Total |
|---|---:|---:|---:|---:|
| v0.1 | 31 | 0 | 0 | 31 |
| v0.2 | 28 | 0 | 0 | 28 |
| v0.3 | 19 | 1 | 2 | 22 |
| v0.4 | 18 | 0 | 0 | 18 |
| v0.5 | 20 | 1 | 0 | 21 |
| v1.0 | 12 | 0 | 1 | 13 |

## 5. Milestones e gates de saída

| Milestone | Resultado verificável e bloqueios |
|---|---|
| v0.1 | [US-124](#us-124): instalar CLI e executar open/status/log/branches/diff/blame em saída humana e JSON. Todos os Must v0.1, segurança de leitura, CI nos três sistemas e DoD arquitetural SAD §40 concluídos. |
| v0.2 | Must v0.2 aceitos após gate v0.1; fluxo TUI inspeção → stage → commit → branch → fetch/pull/push, graph, blame e referências. [US-116](#us-116), [US-111](#us-111), [US-112](#us-112) prontos antes de mutações; binário da TUI e guia de uso instaláveis. |
| v0.3 | Must v0.3 aceitos; Desktop com paridade cotidiana, stage por hunk, amend, diff duplo, busca, dark/light, atalhos e acessibilidade. [US-125](#us-125) instalável; testes TUI/Desktop já ativos. Stage por linha e drag/drop só se estáveis. |
| v0.4 | Must v0.4 aceitos; extensão instala, identifica autoria, navega file/line history, diff e handoff. [US-126](#us-126) e compatibilidade CLI comprovadas; testes das três interfaces consolidados. |
| v0.5 | Must v0.5 aceitos em Core/TUI/Desktop; merge/rebase/cherry-pick/conflitos, reset/revert, stash/tags/worktrees e force push protegidos. Gates incluem regressões de recuperação e todos os guardrails, antes de liberar edição destrutiva. |
| v1.0 | Todos os Must até v1.0 aceitos, decisões Should/Could registradas e [US-128](#us-128) + [US-133](#us-133) completos; compatibilidade, performance medida, privacidade e paridade demonstradas. |

Histórias podem ser desenvolvidas em paralelo por fronteiras já definidas, mas este documento não autoriza começar implementação. Não há datas, estimativas ou alocação de pessoas presumidas.

## 6. Histórias por épico

<a id="epic-01"></a>
### EPIC-01 — Project Foundation

Estabelecer limites arquiteturais e contratos internos compartilhados.

**Rastreabilidade:** PRD §§4–6, 14, 16; SAD §§4–13, 19, 37–40; ADR-001–004 e ADR-009. As histórias deste épico detalham essas fontes; critérios adicionais são refinamentos propostos nesta baseline.

<a id="us-001"></a>
#### US-001 — Estruturar o monorepo e suas fronteiras

- **Épico:** EPIC-01
- **Versão alvo:** v0.1
- **MoSCoW:** Must
- **Componente:** Core / Infraestrutura
- **Dependências:** Nenhuma história prévia; fontes documentais disponíveis.
- **Status:** Planejada

**Como** contribuidor, **quero** uma estrutura única de workspace, **para** evoluir interfaces sem duplicar o domínio.

**Critérios de aceite:**

1. Workspace prevê gitsail-domain, gitsail-application, gitsail-git, gitsail-protocol, gitsail-cli e gitsail-tui, além de apps/desktop e apps/vscode.
2. Dependências apontam para dentro; Domain não importa infraestrutura ou apresentação.
3. Pastas de documentação e fixtures seguem o SAD, sem exigir implementação de UI na v0.1.

**Definition of Done:** DOD-G + Grafo de dependências revisado contra SAD §§5–7 e ADRs associados.

<a id="us-002"></a>
#### US-002 — Modelar entidades e erros de domínio

- **Épico:** EPIC-01
- **Versão alvo:** v0.1
- **MoSCoW:** Must
- **Componente:** Core
- **Dependências:** [US-001](#us-001)
- **Status:** Planejada

**Como** desenvolvedor de interfaces, **quero** tipos próprios para repositório, commits, referências, status, diff e blame, **para** consumir dados sem interpretar saídas Git.

**Critérios de aceite:**

1. Entidades preservam campos do SAD §8, inclusive autor/committer, parents e estados HEAD.
2. Erros têm código estável, mensagem segura, remediation opcional e ID de operação.
3. Tipos não dependem de runtime async, toolkit ou saída textual do Git.

**Definition of Done:** DOD-G + Testes de invariantes e catálogo de erros revisados; não há serialização externa acidental.

<a id="us-003"></a>
#### US-003 — Separar casos de uso e portas de leitura/escrita

- **Épico:** EPIC-01
- **Versão alvo:** v0.1
- **MoSCoW:** Must
- **Componente:** Core
- **Dependências:** [US-002](#us-002)
- **Status:** Planejada

**Como** mantenedor, **quero** portas de inspeção e mutação distintas, **para** reutilizar comportamento sem conceder capacidades desnecessárias.

**Critérios de aceite:**

1. Casos de uso de leitura do SAD §9 dependem de portas e podem usar doubles.
2. Contratos de mutação ficam separados quando aplicável sem antecipar operações v0.5.
3. Nenhuma UI escolhe comandos ou faz parsing Git.

**Definition of Done:** DOD-G + Teste com double demonstra substituição do adapter; revisão confirma ADR-002 e ADR-009.

<a id="us-004"></a>
#### US-004 — Controlar processos Git pelo runner

- **Épico:** EPIC-01
- **Versão alvo:** v0.1
- **MoSCoW:** Must
- **Componente:** Git Provider
- **Dependências:** [US-002](#us-002)
- **Status:** Planejada

**Como** usuário, **quero** execução controlada dos processos Git, **para** receber resultados e falhas previsíveis.

**Critérios de aceite:**

1. GitProcessRunner recebe executável, argumentos, ambiente e diretório explícitos sem shell.
2. Captura stdout, stderr, exit status, duração e cancelamento com limites de recursos documentados.
3. Git ausente, versão incompatível, timeout e cancelamento geram erros distintos sem vazar segredos.

**Definition of Done:** DOD-G + Fixtures de processo cobrem sucesso, falha, timeout e cancelamento nos sistemas suportados.

<a id="us-005"></a>
#### US-005 — Implementar o adapter inicial Git CLI

- **Épico:** EPIC-01
- **Versão alvo:** v0.1
- **MoSCoW:** Must
- **Componente:** Git Provider
- **Dependências:** [US-003](#us-003), [US-004](#us-004)
- **Status:** Planejada

**Como** mantenedor, **quero** um GitCliProvider substituível, **para** aproveitar o Git instalado sem acoplar as interfaces.

**Critérios de aceite:**

1. Adapter implementa as portas por meio do runner.
2. Parsing usa formatos estruturados e separadores inequívocos, independente de mensagens localizadas.
3. Resultados são mapeados para domínio e falhas de parsing são explícitas.

**Definition of Done:** DOD-G + Contrato do provider testável; revisão localiza todo parsing Git na infraestrutura.

<a id="epic-02"></a>
### EPIC-02 — Repository Discovery

Identificar corretamente o repositório e seu contexto local.

**Rastreabilidade:** PRD §§7.2–7.3, 17; SAD §§8–12, 21, 30–31. As histórias deste épico detalham essas fontes; critérios adicionais são refinamentos propostos nesta baseline.

<a id="us-006"></a>
#### US-006 — Abrir repositório por diretório ou caminho

- **Épico:** EPIC-02
- **Versão alvo:** v0.1
- **MoSCoW:** Must
- **Componente:** Core / Git Provider
- **Dependências:** [US-005](#us-005)
- **Status:** Planejada

**Como** desenvolvedor, **quero** abrir um repositório local, **para** consultar o projeto correto.

**Critérios de aceite:**

1. Diretório atual, raiz e subdiretório resolvem o mesmo repositório.
2. Resposta identifica root e worktree sem presumir que .git seja uma pasta.
3. Caminho inexistente ou fora de repositório retorna erro estruturado sem criar arquivos.

**Definition of Done:** DOD-G + Fixtures de descoberta e comparação de estado antes/depois comprovam leitura sem mutação.

<a id="us-007"></a>
#### US-007 — Representar estados de HEAD e repositório

- **Épico:** EPIC-02
- **Versão alvo:** v0.1
- **MoSCoW:** Must
- **Componente:** Core / Git Provider
- **Dependências:** [US-006](#us-006)
- **Status:** Planejada

**Como** desenvolvedor, **quero** distinguir branch, detached HEAD e repositório vazio, **para** interpretar corretamente meu contexto.

**Critérios de aceite:**

1. Branch atual e commit são retornados quando existem.
2. Unborn HEAD e detached HEAD têm estados distintos, sem branch inventada.
3. Bare repo é identificado e operações dependentes de worktree retornam limitação explícita.

**Definition of Done:** DOD-G + Fixtures empty, one-commit, detached e bare validam os estados.

<a id="us-008"></a>
#### US-008 — Preservar caminhos entre plataformas

- **Épico:** EPIC-02
- **Versão alvo:** v0.1
- **MoSCoW:** Must
- **Componente:** Core / Git Provider
- **Dependências:** [US-006](#us-006)
- **Status:** Planejada

**Como** usuário multiplataforma, **quero** abrir projetos com caminhos reais do meu sistema, **para** trabalhar sem corrupção de nomes.

**Critérios de aceite:**

1. Unicode, espaços, drives Windows e UNC têm tratamento documentado e testes aplicáveis.
2. Caminhos não são montados com separadores fixos nem presumem case sensitivity.
3. Limitações de caminhos não UTF-8 no protocolo são explícitas e não geram substituição silenciosa.

**Definition of Done:** DOD-G + Matriz de paths inclui Unicode e limites de plataforma; representação externa documentada.

<a id="us-009"></a>
#### US-009 — Manter sessão e atualização do repositório

- **Épico:** EPIC-02
- **Versão alvo:** v0.2
- **MoSCoW:** Must
- **Componente:** Core
- **Dependências:** [US-007](#us-007), [US-008](#us-008)
- **Status:** Planejada

**Como** usuário, **quero** uma sessão que acompanhe mudanças externas, **para** não executar ações sobre dados antigos.

**Critérios de aceite:**

1. Sessão mantém identidade, snapshots HEAD/status, seleção, geração de refresh e operações ativas.
2. Refresh manual, por foco e após mutação invalida dados relevantes.
3. Resultados de geração antiga não substituem estado novo; trocar repositório cancela ou descarta respostas antigas.

**Definition of Done:** DOD-G + Teste de corrida e mudança externa comprova isolamento entre sessões.

<a id="epic-03"></a>
### EPIC-03 — Working Tree & Status

Inspecionar mudanças e preparar commits com controle explícito.

**Rastreabilidade:** PRD §§7.2–7.3, 8.3, 9.3; SAD §§8–10, 20–23, 26. As histórias deste épico detalham essas fontes; critérios adicionais são refinamentos propostos nesta baseline.

<a id="us-010"></a>
#### US-010 — Consultar status tipado da working tree e index

- **Épico:** EPIC-03
- **Versão alvo:** v0.1
- **MoSCoW:** Must
- **Componente:** Core / Git Provider
- **Dependências:** [US-007](#us-007)
- **Status:** Planejada

**Como** desenvolvedor, **quero** listar mudanças locais, **para** entender o que será incluído em um commit.

**Critérios de aceite:**

1. Distingue modificado, adicionado, removido, renomeado detectável e untracked.
2. Index e worktree são campos separados; rename preserva caminho anterior.
3. Clean/dirty e estados sem commits são coerentes; leitura não altera index ou arquivos.

**Definition of Done:** DOD-G + Fixtures combinam staged e unstaged no mesmo arquivo e nomes com separadores incomuns.

<a id="us-011"></a>
#### US-011 — Stage e unstage por arquivo

- **Épico:** EPIC-03
- **Versão alvo:** v0.2
- **MoSCoW:** Must
- **Componente:** Core / Git Provider
- **Dependências:** [US-010](#us-010), [US-111](#us-111), [US-116](#us-116)
- **Status:** Planejada

**Como** desenvolvedor, **quero** escolher arquivos do index, **para** compor commits intencionais.

**Critérios de aceite:**

1. Stage inclui somente caminhos selecionados, inclusive remoções.
2. Unstage preserva conteúdo da working tree e funciona antes do primeiro commit.
3. Falha ou alteração concorrente atualiza status e informa o resultado sem sucesso falso.

**Definition of Done:** DOD-G + Integração verifica index e working tree antes/depois, incluindo repositório vazio.

<a id="us-012"></a>
#### US-012 — Criar commit a partir do index

- **Épico:** EPIC-03
- **Versão alvo:** v0.2
- **MoSCoW:** Must
- **Componente:** Core / Git Provider
- **Dependências:** [US-011](#us-011)
- **Status:** Planejada

**Como** desenvolvedor, **quero** registrar mudanças preparadas, **para** salvar uma unidade de trabalho.

**Critérios de aceite:**

1. Commit contém somente o index e mensagem fornecida; vazio não é criado implicitamente.
2. Falta de identidade e falha de hook geram diagnóstico e preservam trabalho.
3. Sucesso retorna hash e atualiza HEAD/status; falha não é tratada como commit concluído.

**Definition of Done:** DOD-G + Fixtures verificam árvore do commit, hooks com erro e arquivos unstaged preservados.

<a id="us-013"></a>
#### US-013 — Stage e unstage por hunk

- **Épico:** EPIC-03
- **Versão alvo:** v0.3
- **MoSCoW:** Must
- **Componente:** Core / Git Provider
- **Dependências:** [US-011](#us-011), [US-025](#us-025)
- **Status:** Planejada

**Como** desenvolvedor, **quero** selecionar hunks de um arquivo, **para** separar mudanças em commits.

**Critérios de aceite:**

1. Seleção opera sobre uma versão conhecida do diff.
2. Somente hunks escolhidos alteram o index, preservando working tree.
3. Diff obsoleto ou hunk inaplicável impede aplicação cega e pede atualização.

**Definition of Done:** DOD-G + Teste integra múltiplos hunks, remoções e alteração concorrente do arquivo.

<a id="us-014"></a>
#### US-014 — Stage por linha quando estável

- **Épico:** EPIC-03
- **Versão alvo:** v0.3
- **MoSCoW:** Could
- **Componente:** Core / Desktop
- **Dependências:** [US-013](#us-013)
- **Status:** Planejada

**Como** desenvolvedor, **quero** selecionar linhas de uma mudança, **para** produzir commits menores.

**Critérios de aceite:**

1. Somente linhas selecionadas entram no index com contexto válido.
2. Linhas adjacentes, CRLF e ausência de newline final têm comportamento verificado.
3. Casos não suportados desabilitam a ação com explicação, mantendo stage por hunk.

**Definition of Done:** DOD-G + Habilitação depende de regressões de patch aprovadas; caso contrário item é adiado explicitamente.

<a id="epic-04"></a>
### EPIC-04 — Commit History

Consultar histórico e contexto sem carregar todo o repositório.

**Rastreabilidade:** PRD §§7.2, 8.3, 9.3, 10.2; SAD §§8–9, 23–25. As histórias deste épico detalham essas fontes; critérios adicionais são refinamentos propostos nesta baseline.

<a id="us-015"></a>
#### US-015 — Listar commits com paginação

- **Épico:** EPIC-04
- **Versão alvo:** v0.1
- **MoSCoW:** Must
- **Componente:** Core / Git Provider
- **Dependências:** [US-007](#us-007)
- **Status:** Planejada

**Como** desenvolvedor, **quero** consultar páginas de commits, **para** explorar a evolução do projeto.

**Critérios de aceite:**

1. Cada commit inclui hashes, autor/e-mail disponível, committer, datas, mensagem, parents e referências disponíveis.
2. Limite e continuação evitam carregar todo o histórico.
3. Repositório vazio retorna página vazia; detached HEAD é suportado.

**Definition of Done:** DOD-G + Fixtures com merge, vazio e múltiplas páginas validam ordem, campos e continuação.

<a id="us-016"></a>
#### US-016 — Consultar detalhes de um commit

- **Épico:** EPIC-04
- **Versão alvo:** v0.1
- **MoSCoW:** Must
- **Componente:** Core / Git Provider
- **Dependências:** [US-015](#us-015)
- **Status:** Planejada

**Como** desenvolvedor, **quero** abrir um commit pelo identificador, **para** entender sua intenção e relações.

**Critérios de aceite:**

1. Retorna subject/body, autor/committer, datas, parents e decorations.
2. Commit raiz não exige parent; merges preservam todos os parents.
3. Hash inexistente ou abreviação ambígua retorna erro reconhecível.

**Definition of Done:** DOD-G + Integração cobre commit raiz, merge e resolução inequívoca de identificador.

<a id="us-017"></a>
#### US-017 — Filtrar e buscar histórico

- **Épico:** EPIC-04
- **Versão alvo:** v0.2
- **MoSCoW:** Must
- **Componente:** Core / Git Provider
- **Dependências:** [US-015](#us-015)
- **Status:** Planejada

**Como** desenvolvedor, **quero** buscar por texto, autor e referência, **para** localizar mudanças relevantes.

**Critérios de aceite:**

1. Consulta aceita ref/range, autor e texto onde suportado.
2. Filtros persistem nas páginas seguintes e resetam cursor quando alterados.
3. Limites de busca são documentados; zero resultados não é erro.

**Definition of Done:** DOD-G + Combinações de filtros e paginação validadas com dataset conhecido.

<a id="us-018"></a>
#### US-018 — Consultar histórico de arquivo

- **Épico:** EPIC-04
- **Versão alvo:** v0.4
- **MoSCoW:** Must
- **Componente:** Core / Git Provider
- **Dependências:** [US-015](#us-015), [US-008](#us-008)
- **Status:** Planejada

**Como** desenvolvedor, **quero** acompanhar mudanças de um arquivo, **para** entender sua evolução no editor.

**Critérios de aceite:**

1. Consulta retorna commits paginados relacionados ao caminho e revisão.
2. Política de seguir renames é explícita; caminhos antigos retornam contexto correto quando suportado.
3. Arquivo removido ou sem histórico gera estado claro sem histórico inventado.

**Definition of Done:** DOD-G + Fixture de rename e remoção valida resultados e limites documentados.

<a id="us-019"></a>
#### US-019 — Consultar histórico de linha ou intervalo

- **Épico:** EPIC-04
- **Versão alvo:** v0.4
- **MoSCoW:** Must
- **Componente:** Core / Git Provider
- **Dependências:** [US-018](#us-018), [US-031](#us-031)
- **Status:** Planejada

**Como** desenvolvedor, **quero** rastrear a evolução de um trecho, **para** entender a origem de uma alteração.

**Critérios de aceite:**

1. Entrada inclui revisão, caminho e intervalo válido.
2. Resultado relaciona commits e alterações do trecho com limites de rastreamento explícitos.
3. Trecho não commitado, removido ou indisponível não recebe atribuição enganosa.

**Definition of Done:** DOD-G + Fixtures de inserção, remoção e deslocamento de linhas sustentam a política de rastreamento.

<a id="epic-05"></a>
### EPIC-05 — Branches & References

Navegar e administrar referências sem perder trabalho local.

**Rastreabilidade:** PRD §§7.2, 8.3, 11.2; SAD §§8–10, 20, 26. As histórias deste épico detalham essas fontes; critérios adicionais são refinamentos propostos nesta baseline.

<a id="us-020"></a>
#### US-020 — Listar branches locais e remotas

- **Épico:** EPIC-05
- **Versão alvo:** v0.1
- **MoSCoW:** Must
- **Componente:** Core / Git Provider
- **Dependências:** [US-007](#us-007)
- **Status:** Planejada

**Como** desenvolvedor, **quero** consultar branches e upstreams, **para** entender a organização do repositório.

**Critérios de aceite:**

1. Lista tipo, nome, alvo e branch atual.
2. Upstream e ahead/behind são retornados quando calculáveis; ausência difere de zero.
3. Detached HEAD e repositório vazio não produzem branch fictícia.

**Definition of Done:** DOD-G + Fixtures com upstream ausente e divergente comprovam campos e contagens.

<a id="us-021"></a>
#### US-021 — Trocar de branch com proteção

- **Épico:** EPIC-05
- **Versão alvo:** v0.2
- **MoSCoW:** Must
- **Componente:** Core / Git Provider
- **Dependências:** [US-020](#us-020), [US-010](#us-010), [US-111](#us-111), [US-116](#us-116)
- **Status:** Planejada

**Como** desenvolvedor, **quero** trocar de branch, **para** trabalhar na linha correta.

**Critérios de aceite:**

1. Destino e mudanças locais aparecem antes da ação quando houver risco.
2. Mudanças incompatíveis impedem troca sem descarte implícito.
3. Sucesso atualiza HEAD e invalida leituras da sessão.

**Definition of Done:** DOD-G + Teste preserva alterações locais em checkout recusado e valida branch final.

<a id="us-022"></a>
#### US-022 — Criar branch local

- **Épico:** EPIC-05
- **Versão alvo:** v0.2
- **MoSCoW:** Must
- **Componente:** Core / Git Provider
- **Dependências:** [US-020](#us-020), [US-111](#us-111), [US-116](#us-116)
- **Status:** Planejada

**Como** desenvolvedor, **quero** criar branch de uma referência escolhida, **para** isolar meu trabalho.

**Critérios de aceite:**

1. Nome e ponto de partida são explícitos e validados.
2. Nome existente não é sobrescrito.
3. Criação não troca branch sem ação explícita; referências são atualizadas.

**Definition of Done:** DOD-G + Fixtures verificam ref de origem, duplicidade e ausência de checkout implícito.

<a id="us-023"></a>
#### US-023 — Excluir branch local com confirmação

- **Épico:** EPIC-05
- **Versão alvo:** v0.2
- **MoSCoW:** Must
- **Componente:** Core / Git Provider
- **Dependências:** [US-020](#us-020), [US-111](#us-111), [US-116](#us-116)
- **Status:** Planejada

**Como** desenvolvedor, **quero** remover branches encerradas, **para** manter referências organizadas.

**Critérios de aceite:**

1. Confirmação identifica branch e alcance local.
2. Branch atual e branch em uso por worktree são protegidas.
3. Branch não integrada não é excluída à força implicitamente.

**Definition of Done:** DOD-G + Teste de exclusão permitida e recusada verifica que commits/referências indevidos não são removidos.

<a id="us-024"></a>
#### US-024 — Renomear branch local

- **Épico:** EPIC-05
- **Versão alvo:** v0.5
- **MoSCoW:** Must
- **Componente:** Core / Git Provider / TUI / Desktop
- **Dependências:** [US-022](#us-022), [US-040](#us-040), [US-051](#us-051)
- **Status:** Planejada

**Como** desenvolvedor, **quero** renomear uma branch, **para** corrigir sua identificação.

**Critérios de aceite:**

1. TUI e Desktop oferecem nome anterior e novo com validação.
2. Colisão não sobrescreve referência; upstream e contexto resultante ficam visíveis.
3. Operação usa o Core e refresca listas e seleção.

**Definition of Done:** DOD-G + Integração cobre branch atual, outra branch e colisão; fluxos das duas interfaces verificados.

<a id="epic-06"></a>
### EPIC-06 — Diff Engine

Expor diffs tipados e comparações utilizáveis nas interfaces.

**Rastreabilidade:** PRD §§7.2, 9.3, 11.2; SAD §§8–11, 26, 30–32. As histórias deste épico detalham essas fontes; critérios adicionais são refinamentos propostos nesta baseline.

<a id="us-025"></a>
#### US-025 — Consultar diff unstaged e staged

- **Épico:** EPIC-06
- **Versão alvo:** v0.1
- **MoSCoW:** Must
- **Componente:** Core / Git Provider
- **Dependências:** [US-010](#us-010)
- **Status:** Planejada

**Como** desenvolvedor, **quero** comparar working tree e index, **para** inspecionar mudanças antes de commitar.

**Critérios de aceite:**

1. Modos staged e unstaged são explícitos.
2. Resultado contém arquivos, status, ranges de hunks, contexto e adições/remoções.
3. Arquivo com mudanças staged e unstaged apresenta comparações distintas.

**Definition of Done:** DOD-G + Fixtures validam conteúdo e ranges; nenhuma consulta altera arquivos ou index.

<a id="us-026"></a>
#### US-026 — Consultar diff de commit

- **Épico:** EPIC-06
- **Versão alvo:** v0.1
- **MoSCoW:** Must
- **Componente:** Core / Git Provider
- **Dependências:** [US-016](#us-016)
- **Status:** Planejada

**Como** desenvolvedor, **quero** visualizar alterações de um commit, **para** revisar seu efeito.

**Critérios de aceite:**

1. Commit raiz é comparado com árvore vazia.
2. Política de parent para merge é explícita e permite identificar base escolhida.
3. Arquivos adicionados, removidos e renomeados têm paths e conteúdo coerentes.

**Definition of Done:** DOD-G + Fixtures raiz e merge verificam base da comparação e linhas resultantes.

<a id="us-027"></a>
#### US-027 — Tratar diffs binários e limites de conteúdo

- **Épico:** EPIC-06
- **Versão alvo:** v0.1
- **MoSCoW:** Must
- **Componente:** Core / Git Provider
- **Dependências:** [US-025](#us-025), [US-026](#us-026)
- **Status:** Planejada

**Como** desenvolvedor, **quero** distinguir texto, binário e conteúdo indisponível, **para** não interpretar um diff incompleto como vazio.

**Critérios de aceite:**

1. Binários indicam mudança sem inventar hunks textuais.
2. CRLF, ausência de newline final e rename preservam informação relevante.
3. Truncamento ou limite de tamanho é informado; operações longas podem ser canceladas.

**Definition of Done:** DOD-G + Fixtures binárias e de newline comprovam representação e cancelamento.

<a id="us-028"></a>
#### US-028 — Comparar duas revisões

- **Épico:** EPIC-06
- **Versão alvo:** v0.3
- **MoSCoW:** Must
- **Componente:** Core / Git Provider
- **Dependências:** [US-026](#us-026)
- **Status:** Planejada

**Como** desenvolvedor, **quero** escolher base e destino de uma comparação, **para** analisar diferenças entre pontos do histórico.

**Critérios de aceite:**

1. Base e destino são exibidos e resolvidos sem ambiguidade.
2. Troca dos lados altera corretamente sinais e caminhos.
3. Referência inválida falha sem reaproveitar diff antigo como atual.

**Definition of Done:** DOD-G + Teste de comparação reversa e referências inválidas documenta a semântica.

<a id="us-029"></a>
#### US-029 — Copiar ou exportar patch

- **Épico:** EPIC-06
- **Versão alvo:** v0.5
- **MoSCoW:** Must
- **Componente:** Core / TUI / Desktop
- **Dependências:** [US-025](#us-025), [US-040](#us-040), [US-051](#us-051)
- **Status:** Planejada

**Como** desenvolvedor, **quero** obter um patch das mudanças escolhidas, **para** compartilhar uma alteração por ação explícita.

**Critérios de aceite:**

1. Origem e escopo do patch são informados.
2. Exportação preserva formato aplicável e não modifica repositório.
3. Clipboard indisponível oferece arquivo ou alternativa documentada; conteúdo não é enviado a serviços.

**Definition of Done:** DOD-G + Round-trip de patch textual em fixture comprova fidelidade e fluxo nas interfaces.

<a id="us-030"></a>
#### US-030 — Aplicar patch quando suportado

- **Épico:** EPIC-06
- **Versão alvo:** v0.5
- **MoSCoW:** Should
- **Componente:** Core / TUI / Desktop
- **Dependências:** [US-029](#us-029), [US-111](#us-111), [US-116](#us-116)
- **Status:** Planejada

**Como** desenvolvedor, **quero** aplicar um patch selecionado, **para** reutilizar mudanças com controle.

**Critérios de aceite:**

1. Prévia mostra arquivos e destino suportado antes da aplicação.
2. Patch inválido, path fora do repositório ou contexto incompatível é rejeitado.
3. Resultado informa falhas e eventual aplicação parcial, sem alegar rollback inexistente.

**Definition of Done:** DOD-G + Fixtures de patch válido, inválido e malicioso verificam resultado e preservação de dados.

<a id="epic-07"></a>
### EPIC-07 — Blame Engine

Atribuir linhas a commits com contexto e limitações explícitas.

**Rastreabilidade:** PRD §§7.2–7.3, 10.2–10.4; SAD §§8–11, 23, 26. As histórias deste épico detalham essas fontes; critérios adicionais são refinamentos propostos nesta baseline.

<a id="us-031"></a>
#### US-031 — Consultar blame tipado por arquivo

- **Épico:** EPIC-07
- **Versão alvo:** v0.1
- **MoSCoW:** Must
- **Componente:** Core / Git Provider
- **Dependências:** [US-007](#us-007), [US-008](#us-008)
- **Status:** Planejada

**Como** desenvolvedor, **quero** conhecer autoria por linha, **para** entender a origem do código.

**Critérios de aceite:**

1. Cada linha contém número final/original, commit, autor, timestamp e conteúdo.
2. Consulta respeita repositório e caminho explícitos.
3. Arquivo inexistente, vazio, binário ou não versionado tem resultado/erro definido.

**Definition of Done:** DOD-G + Integração com múltiplos autores e arquivo vazio valida mapeamento de linhas.

<a id="us-032"></a>
#### US-032 — Consultar blame em revisão e intervalo

- **Épico:** EPIC-07
- **Versão alvo:** v0.4
- **MoSCoW:** Must
- **Componente:** Core / Git Provider
- **Dependências:** [US-031](#us-031)
- **Status:** Planejada

**Como** desenvolvedor, **quero** limitar blame à revisão e trecho desejados, **para** consultar contexto relevante no editor.

**Critérios de aceite:**

1. Revisão e intervalo têm validação e semântica documentada.
2. Números de linha continuam referindo-se à versão consultada.
3. Intervalo inválido e revisão inexistente não retornam dados de outra versão.

**Definition of Done:** DOD-G + Fixture de linhas deslocadas entre commits valida referências históricas.

<a id="us-033"></a>
#### US-033 — Distinguir linhas locais de linhas commitadas

- **Épico:** EPIC-07
- **Versão alvo:** v0.4
- **MoSCoW:** Must
- **Componente:** Core / Git Provider
- **Dependências:** [US-031](#us-031), [US-010](#us-010)
- **Status:** Planejada

**Como** desenvolvedor, **quero** identificar conteúdo ainda não commitado, **para** evitar atribuir minhas edições a outro autor.

**Critérios de aceite:**

1. Linhas alteradas localmente têm estado próprio.
2. Conteúdo consultado e revisão são identificáveis pela interface.
3. Buffer não salvo não é tratado como cópia idêntica do arquivo em disco.

**Definition of Done:** DOD-G + Fixture de edição local e contrato para buffer divergente evitam atribuições falsas.

<a id="us-034"></a>
#### US-034 — Atualizar e cancelar consultas de blame

- **Épico:** EPIC-07
- **Versão alvo:** v0.4
- **MoSCoW:** Must
- **Componente:** Core
- **Dependências:** [US-032](#us-032), [US-033](#us-033), [US-009](#us-009)
- **Status:** Planejada

**Como** desenvolvedor, **quero** blame responsivo e atualizado, **para** navegar arquivos sem resultados atrasados.

**Critérios de aceite:**

1. Cache considera repositório, revisão, caminho e versão do conteúdo.
2. Mudanças relevantes invalidam cache; resposta atrasada não decora outro arquivo.
3. Cancelamento interrompe trabalho desnecessário e não retorna resultado completo fictício.

**Definition of Done:** DOD-G + Teste de troca rápida de arquivo e edição verifica invalidação e isolamento.

<a id="epic-08"></a>
### EPIC-08 — CLI & Protocol

Entregar consultas humanas e um contrato JSON versionado.

**Rastreabilidade:** PRD §§7.2–7.4, 15, 22; SAD §§14–15, 19, 36; ADR-008 e ADR-012. As histórias deste épico detalham essas fontes; critérios adicionais são refinamentos propostos nesta baseline.

<a id="us-035"></a>
#### US-035 — Definir DTOs e envelope versionado

- **Épico:** EPIC-08
- **Versão alvo:** v0.1
- **MoSCoW:** Must
- **Componente:** Protocol
- **Dependências:** [US-002](#us-002)
- **Status:** Planejada

**Como** integrador, **quero** um contrato JSON explícito, **para** consumir o Core sem depender de tipos internos.

**Critérios de aceite:**

1. schemaVersion é obrigatório e DTOs mapeiam domínio explicitamente.
2. Envelope de sucesso, erro, paginação e correlação é documentado antes de estabilizar consumidores.
3. JSON de exemplo no PRD/SAD é tratado como conceitual; decisão final registra compatibilidade.

**Definition of Done:** DOD-G + Schemas/exemplos válidos e testes de mapeamento cobrem dados e erros sem expor structs internos.

<a id="us-036"></a>
#### US-036 — Expor os seis comandos de consulta

- **Épico:** EPIC-08
- **Versão alvo:** v0.1
- **MoSCoW:** Must
- **Componente:** CLI
- **Dependências:** [US-006](#us-006), [US-010](#us-010), [US-015](#us-015), [US-020](#us-020), [US-027](#us-027), [US-031](#us-031)
- **Status:** Planejada

**Como** desenvolvedor, **quero** usar open, status, log, branches, diff e blame, **para** consultar GitSail pelo terminal.

**Critérios de aceite:**

1. Comandos aceitam contexto de repositório consistente e exibem saída legível.
2. Help documenta opções, limites e exemplos de cada consulta.
3. Argumento inválido e falha operacional têm exit code e mensagem apropriados.

**Definition of Done:** DOD-G + Smoke tests executam todos os comandos nas fixtures sem alterar o repositório.

<a id="us-037"></a>
#### US-037 — Oferecer modo JSON consumível por processos

- **Épico:** EPIC-08
- **Versão alvo:** v0.1
- **MoSCoW:** Must
- **Componente:** CLI / Protocol
- **Dependências:** [US-036](#us-036), [US-035](#us-035)
- **Status:** Planejada

**Como** integrador, **quero** executar consultas com --json, **para** automatizar consumo confiável.

**Critérios de aceite:**

1. Consultas relevantes retornam envelope JSON válido e versionado.
2. stdout não mistura progresso, ANSI ou logs; diagnósticos seguem canal definido.
3. Exit status e envelope de erro são consistentes inclusive em repo inválido.

**Definition of Done:** DOD-G + Teste de processo analisa stdout de cada comando e valida códigos de saída.

<a id="us-038"></a>
#### US-038 — Controlar encerramento e diagnósticos da CLI

- **Épico:** EPIC-08
- **Versão alvo:** v0.1
- **MoSCoW:** Must
- **Componente:** CLI
- **Dependências:** [US-037](#us-037), [US-004](#us-004)
- **Status:** Planejada

**Como** usuário, **quero** interromper consultas longas e receber diagnóstico seguro, **para** recuperar controle do terminal.

**Critérios de aceite:**

1. Interrupção e timeout finalizam processos filhos sem deixar leitura pendurada.
2. Falhas distinguem Git ausente, permissão, parsing e operação cancelada.
3. Modo debug mantém logs separados e oculta credenciais/conteúdo sensível.

**Definition of Done:** DOD-G + Teste de cancelamento e captura de saídas comprova término e redação de segredos.

<a id="us-039"></a>
#### US-039 — Estabilizar compatibilidade do protocolo

- **Épico:** EPIC-08
- **Versão alvo:** v1.0
- **MoSCoW:** Must
- **Componente:** Protocol / CLI / VSCode
- **Dependências:** [US-037](#us-037), [US-069](#us-069)
- **Status:** Planejada

**Como** integrador, **quero** uma política explícita de compatibilidade, **para** atualizar componentes com previsibilidade.

**Critérios de aceite:**

1. Mudanças incompatíveis incrementam schemaVersion.
2. Cliente detecta versão incompatível e informa correção sem interpretar dados errados.
3. Matriz de versões suportadas e fixtures de compatibilidade acompanham releases.

**Definition of Done:** DOD-G + Suíte produtor/consumidor cobre versões suportadas e rejeição de schema desconhecido.

<a id="epic-09"></a>
### EPIC-09 — TUI Foundation

Criar uma experiência Ratatui responsiva e centrada no teclado.

**Rastreabilidade:** PRD §§8.1–8.5, 13.4; SAD §§18, 21–22, 26; ADR-005. As histórias deste épico detalham essas fontes; critérios adicionais são refinamentos propostos nesta baseline.

<a id="us-040"></a>
#### US-040 — Abrir sessão e renderizar estrutura da TUI

- **Épico:** EPIC-09
- **Versão alvo:** v0.2
- **MoSCoW:** Must
- **Componente:** TUI
- **Dependências:** [US-009](#us-009), [US-036](#us-036)
- **Status:** Planejada

**Como** usuário de terminal, **quero** abrir o GitSail em modo interativo, **para** explorar um repositório pelo teclado.

**Critérios de aceite:**

1. Layout contém sidebar, graph, detalhes, diff e barra de atalhos.
2. Repositório/branch e estados de carregamento, vazio e erro ficam visíveis.
3. Aplicação usa Ratatui e application diretamente sem parsing Git próprio.

**Definition of Done:** DOD-G + Smoke test de abertura e estado inicial usa fixture conhecida; referência visual TUI registrada.

<a id="us-041"></a>
#### US-041 — Separar eventos, atualização e renderização

- **Épico:** EPIC-09
- **Versão alvo:** v0.2
- **MoSCoW:** Must
- **Componente:** TUI
- **Dependências:** [US-040](#us-040)
- **Status:** Planejada

**Como** usuário de terminal, **quero** interagir durante operações demoradas, **para** manter a interface responsiva.

**Critérios de aceite:**

1. Eventos geram ações e updates tipados.
2. Processos Git não executam no render loop.
3. Conclusões e cancelamentos atualizam apenas a sessão/generation correspondente.

**Definition of Done:** DOD-G + Teste com operação deliberadamente lenta comprova navegação e descarte de resultado antigo.

<a id="us-042"></a>
#### US-042 — Navegar por teclado e ajuda contextual

- **Épico:** EPIC-09
- **Versão alvo:** v0.2
- **MoSCoW:** Must
- **Componente:** TUI
- **Dependências:** [US-041](#us-041)
- **Status:** Planejada

**Como** usuário de terminal, **quero** atalhos consistentes e ajuda visível, **para** aprender e executar tarefas rapidamente.

**Critérios de aceite:**

1. Setas/j/k, Enter, /, q e ? seguem contexto documentado.
2. Foco entre painéis é visível e pode ser movimentado sem mouse.
3. Menu de ações ou command palette mostra apenas ações disponíveis e seus atalhos.

**Definition of Done:** DOD-G + Teste de sequência de teclas verifica foco, retorno e ajuda sem disparar ações ocultas.

<a id="us-043"></a>
#### US-043 — Restaurar terminal e adaptar renderização

- **Épico:** EPIC-09
- **Versão alvo:** v0.2
- **MoSCoW:** Must
- **Componente:** TUI
- **Dependências:** [US-040](#us-040)
- **Status:** Planejada

**Como** usuário de terminal, **quero** encerrar e redimensionar com segurança, **para** continuar usando meu terminal normalmente.

**Critérios de aceite:**

1. Saída normal e falhas tratadas restauram modo e cursor.
2. Resize adapta painéis e comunica largura mínima quando necessário.
3. Modo com poucas cores/ASCII mantém estados distinguíveis sem depender só de cor.

**Definition of Done:** DOD-G + Verificação em tamanhos e capacidades diferentes registra restauração após falha simulada.

<a id="us-044"></a>
#### US-044 — Mostrar progresso e confirmação de operações

- **Épico:** EPIC-09
- **Versão alvo:** v0.2
- **MoSCoW:** Must
- **Componente:** TUI
- **Dependências:** [US-041](#us-041), [US-111](#us-111)
- **Status:** Planejada

**Como** usuário de terminal, **quero** ver estado, risco e resultado de cada ação, **para** decidir e recuperar-me de falhas.

**Critérios de aceite:**

1. Operação mostra alvo e estado em andamento.
2. Confirmação usa metadados do Core e permite cancelar antes de mutar.
3. Erro oferece mensagem segura/remediação e sucesso provoca refresh.

**Definition of Done:** DOD-G + Testes de estado cobrem confirmar, cancelar, falhar e concluir sem confirmação genérica ambígua.

<a id="epic-10"></a>
### EPIC-10 — TUI Git Workflow

Completar o fluxo cotidiano no terminal usando capacidades compartilhadas.

**Rastreabilidade:** PRD §§8.2–8.6; SAD §§9, 18, 20. As histórias deste épico detalham essas fontes; critérios adicionais são refinamentos propostos nesta baseline.

<a id="us-045"></a>
#### US-045 — Explorar histórico e detalhes na TUI

- **Épico:** EPIC-10
- **Versão alvo:** v0.2
- **MoSCoW:** Must
- **Componente:** TUI
- **Dependências:** [US-042](#us-042), [US-017](#us-017), [US-016](#us-016), [US-066](#us-066)
- **Status:** Planejada

**Como** desenvolvedor, **quero** navegar e buscar commits, **para** entender o histórico pelo terminal.

**Critérios de aceite:**

1. Lista e graph mantêm seleção do mesmo commit.
2. Busca altera consulta paginada sem travar navegação.
3. Enter abre detalhes com hash, autor, datas e mensagem.

**Definition of Done:** DOD-G + Fluxo de busca, paginação e detalhes validado em fixture com branches e merge.

<a id="us-046"></a>
#### US-046 — Inspecionar status, diff e blame na TUI

- **Épico:** EPIC-10
- **Versão alvo:** v0.2
- **MoSCoW:** Must
- **Componente:** TUI
- **Dependências:** [US-042](#us-042), [US-010](#us-010), [US-027](#us-027), [US-031](#us-031)
- **Status:** Planejada

**Como** desenvolvedor, **quero** inspecionar arquivos e autoria, **para** revisar mudanças antes de agir.

**Critérios de aceite:**

1. Status separa index/worktree e abre o diff correto.
2. Diff permite percorrer arquivos/hunks e informa binários/limites.
3. Blame de arquivo apresenta linha, autor e commit sem expor controles de terminal.

**Definition of Done:** DOD-G + Fluxo completo de seleção e leitura verificado com arquivo alterado, binário e nome malicioso.

<a id="us-047"></a>
#### US-047 — Preparar e criar commits na TUI

- **Épico:** EPIC-10
- **Versão alvo:** v0.2
- **MoSCoW:** Must
- **Componente:** TUI
- **Dependências:** [US-046](#us-046), [US-044](#us-044), [US-012](#us-012)
- **Status:** Planejada

**Como** desenvolvedor, **quero** fazer stage, unstage e commit, **para** concluir trabalho sem sair da TUI.

**Critérios de aceite:**

1. Seleção de arquivos invoca casos de uso compartilhados.
2. Composer aceita mensagem e mostra escopo staged.
3. Falha preserva mensagem e trabalho; sucesso atualiza status/histórico.

**Definition of Done:** DOD-G + Cenário cotidiano e hook recusando commit são exercitados na TUI.

<a id="us-048"></a>
#### US-048 — Administrar branches na TUI

- **Épico:** EPIC-10
- **Versão alvo:** v0.2
- **MoSCoW:** Must
- **Componente:** TUI
- **Dependências:** [US-044](#us-044), [US-021](#us-021), [US-022](#us-022), [US-023](#us-023)
- **Status:** Planejada

**Como** desenvolvedor, **quero** criar, trocar e excluir branches, **para** organizar meu fluxo no terminal.

**Critérios de aceite:**

1. Lista distingue locais/remotas e branch atual.
2. Ações exibem nome/ref de origem e confirmação adequada.
3. Branch protegida ou mudanças incompatíveis mostram erro sem descarte.

**Definition of Done:** DOD-G + Fluxo com criação, checkout e exclusão verifica estado Git após cada ação.

<a id="us-049"></a>
#### US-049 — Sincronizar com remote pela TUI

- **Épico:** EPIC-10
- **Versão alvo:** v0.2
- **MoSCoW:** Must
- **Componente:** TUI
- **Dependências:** [US-044](#us-044), [US-096](#us-096), [US-097](#us-097), [US-098](#us-098)
- **Status:** Planejada

**Como** desenvolvedor, **quero** executar fetch, pull e push, **para** sincronizar sem deixar o terminal interativo.

**Critérios de aceite:**

1. Remote e branch/upstream ficam explícitos.
2. Progresso e erros de autenticação/rede não bloqueiam navegação.
3. Divergência ou conflito é mostrado sem rebase/force push automático.

**Definition of Done:** DOD-G + E2E com remote fixture cobre sincronização e push rejeitado.

<a id="us-050"></a>
#### US-050 — Consultar tags, remotes e stash na TUI

- **Épico:** EPIC-10
- **Versão alvo:** v0.2
- **MoSCoW:** Must
- **Componente:** TUI
- **Dependências:** [US-042](#us-042), [US-091](#us-091)
- **Status:** Planejada

**Como** desenvolvedor, **quero** inspecionar referências auxiliares, **para** conhecer o estado do repositório.

**Critérios de aceite:**

1. Painéis listam tags, remotes e entradas de stash.
2. Seleção mostra alvo/mensagem e metadados disponíveis.
3. Ausência de itens é estado vazio; leitura não oferece mutações v0.5 prematuramente.

**Definition of Done:** DOD-G + Fixture com e sem referências valida listagem e navegação.

<a id="epic-11"></a>
### EPIC-11 — Desktop Foundation

Estabelecer uma GUI própria em Tauri e Vue 3.

**Rastreabilidade:** PRD §§9.1–9.5, 13.4; SAD §§17, 21–22, 26; ADR-006. As histórias deste épico detalham essas fontes; critérios adicionais são refinamentos propostos nesta baseline.

<a id="us-051"></a>
#### US-051 — Criar shell Desktop e bridge tipada

- **Épico:** EPIC-11
- **Versão alvo:** v0.3
- **MoSCoW:** Must
- **Componente:** Desktop
- **Dependências:** [US-003](#us-003), [US-009](#us-009), [US-035](#us-035)
- **Status:** Planejada

**Como** usuário Desktop, **quero** abrir uma aplicação gráfica integrada ao Core, **para** usar GitSail visualmente.

**Critérios de aceite:**

1. Vue 3 usa serviço tipado e comandos Tauri finos.
2. Estado separa view, sessão, dados e preferências.
3. Nenhum comando Tauri duplica parsing ou regras Git.

**Definition of Done:** DOD-G + Smoke multiplataforma e revisão de bridge demonstram limites do SAD §17.

<a id="us-052"></a>
#### US-052 — Abrir e retomar repositórios recentes

- **Épico:** EPIC-11
- **Versão alvo:** v0.3
- **MoSCoW:** Must
- **Componente:** Desktop
- **Dependências:** [US-051](#us-051), [US-008](#us-008)
- **Status:** Planejada

**Como** desenvolvedor, **quero** selecionar projetos locais e recentes, **para** retomar meu trabalho rapidamente.

**Critérios de aceite:**

1. Seletor aceita pasta e mantém lista de recentes.
2. Repositório movido/inacessível mostra recuperação sem apagar conteúdo.
3. Troca de projeto isola seleção, operações e resultados atrasados.

**Definition of Done:** DOD-G + Teste de projeto recente válido/inválido e troca durante leitura aprovado.

<a id="us-053"></a>
#### US-053 — Compor layout e identidade GitSail

- **Épico:** EPIC-11
- **Versão alvo:** v0.3
- **MoSCoW:** Must
- **Componente:** Desktop
- **Dependências:** [US-051](#us-051), [US-129](#us-129)
- **Status:** Planejada

**Como** desenvolvedor, **quero** uma interface reconhecível e organizada, **para** explorar graph e mudanças com clareza.

**Critérios de aceite:**

1. Layout contém graph central, sidebar de branches/remotes/tags/stashes e painéis de mudanças/detalhes.
2. Usa GitSail e tagline Navigate your Git history., com conceito náutico e vela + Git graph.
3. Mockup Desktop orienta identidade própria; adaptação funcional não copia assets ou UI proprietária.

**Definition of Done:** DOD-G + Revisão contra os assets originais e PRD documenta decisões visuais e estados vazio/erro/loading.

<a id="us-054"></a>
#### US-054 — Manter GUI responsiva e atualizada

- **Épico:** EPIC-11
- **Versão alvo:** v0.3
- **MoSCoW:** Must
- **Componente:** Desktop
- **Dependências:** [US-051](#us-051), [US-009](#us-009)
- **Status:** Planejada

**Como** desenvolvedor, **quero** interagir durante leituras e sincronizações, **para** trabalhar sem congelamentos.

**Critérios de aceite:**

1. Operações não bloqueiam UI e exibem andamento.
2. Refresh manual, por foco e após mutação usa sessão compartilhada.
3. Resultados de consultas canceladas ou sessões antigas não substituem dados atuais.

**Definition of Done:** DOD-G + Cenário com atraso e troca de repositório valida responsividade e isolamento.

<a id="us-055"></a>
#### US-055 — Oferecer navegação acessível

- **Épico:** EPIC-11
- **Versão alvo:** v0.3
- **MoSCoW:** Must
- **Componente:** Desktop
- **Dependências:** [US-053](#us-053), [US-054](#us-054)
- **Status:** Planejada

**Como** usuário, **quero** operar painéis por teclado e mouse, **para** usar o cliente de forma confortável.

**Critérios de aceite:**

1. Foco visível, labels/tooltips e navegação de teclado cobrem ações principais.
2. Zoom e resize preservam acesso às ações.
3. Estados usam texto/ícone além de cor e contraste é verificado nos temas suportados.

**Definition of Done:** DOD-G + Checklist de acessibilidade e fluxo sem mouse registrados com problemas bloqueantes resolvidos.

<a id="epic-12"></a>
### EPIC-12 — Desktop Git Workflow

Entregar operações cotidianas completas com revisão visual.

**Rastreabilidade:** PRD §§9.2–9.5, 8.3; SAD §§9, 17, 20. As histórias deste épico detalham essas fontes; critérios adicionais são refinamentos propostos nesta baseline.

<a id="us-056"></a>
#### US-056 — Navegar histórico e busca global

- **Épico:** EPIC-12
- **Versão alvo:** v0.3
- **MoSCoW:** Must
- **Componente:** Desktop
- **Dependências:** [US-053](#us-053), [US-017](#us-017), [US-067](#us-067)
- **Status:** Planejada

**Como** desenvolvedor, **quero** buscar commits, branches e tags, **para** localizar pontos do histórico visualmente.

**Critérios de aceite:**

1. Busca aceita hash, mensagem, autor, branch e tag.
2. Graph/lista/detalhes preservam identidade da seleção.
3. Context menus e command palette invocam ações válidas para o item.

**Definition of Done:** DOD-G + E2E de busca e seleção de referência valida detalhes e contexto.

<a id="us-057"></a>
#### US-057 — Exibir diff unified e side-by-side

- **Épico:** EPIC-12
- **Versão alvo:** v0.3
- **MoSCoW:** Must
- **Componente:** Desktop
- **Dependências:** [US-053](#us-053), [US-027](#us-027), [US-028](#us-028)
- **Status:** Planejada

**Como** desenvolvedor, **quero** comparar mudanças em dois modos, **para** revisar alterações com precisão.

**Critérios de aceite:**

1. Usuário alterna unified/side-by-side preservando arquivo e base/destino.
2. Linhas e hunks mapeiam corretamente adições/remoções.
3. Binário e diff limitado têm indicação explícita; listas extensas não exigem render total.

**Definition of Done:** DOD-G + Fixture de múltiplos hunks verifica equivalência dos modos e estado de conteúdo indisponível.

<a id="us-058"></a>
#### US-058 — Selecionar mudanças e compor commit

- **Épico:** EPIC-12
- **Versão alvo:** v0.3
- **MoSCoW:** Must
- **Componente:** Desktop
- **Dependências:** [US-057](#us-057), [US-012](#us-012), [US-013](#us-013), [US-061](#us-061)
- **Status:** Planejada

**Como** desenvolvedor, **quero** fazer stage por arquivo/hunk e escrever commit, **para** registrar mudanças revisadas.

**Critérios de aceite:**

1. Painéis separam staged e unstaged e atualizam após cada seleção.
2. Composer mostra mensagem e escopo do index.
3. Erro preserva mensagem e seleção; sucesso mostra hash e estado atualizado.

**Definition of Done:** DOD-G + E2E verifica conteúdo efetivo do commit e preservação de mudanças não selecionadas.

<a id="us-059"></a>
#### US-059 — Alterar último commit com confirmação

- **Épico:** EPIC-12
- **Versão alvo:** v0.3
- **MoSCoW:** Must
- **Componente:** Core / Desktop
- **Dependências:** [US-012](#us-012), [US-061](#us-061)
- **Status:** Planejada

**Como** desenvolvedor, **quero** usar amend conscientemente, **para** corrigir o último commit.

**Critérios de aceite:**

1. Prévia identifica HEAD, mensagem e mudanças staged a incluir.
2. Ação explicita substituição do commit e risco de histórico já compartilhado.
3. HEAD é revalidado antes de executar; ausência de commit ou mudança concorrente bloqueia ação.

**Definition of Done:** DOD-G + Integração demonstra novo hash, preservação de trabalho e recusa com HEAD obsoleto.

<a id="us-060"></a>
#### US-060 — Operar branches e sincronização no Desktop

- **Épico:** EPIC-12
- **Versão alvo:** v0.3
- **MoSCoW:** Must
- **Componente:** Desktop
- **Dependências:** [US-061](#us-061), [US-021](#us-021), [US-022](#us-022), [US-023](#us-023), [US-096](#us-096), [US-097](#us-097), [US-098](#us-098)
- **Status:** Planejada

**Como** desenvolvedor, **quero** administrar branches e remotes na GUI, **para** concluir meu fluxo cotidiano.

**Critérios de aceite:**

1. Criar/trocar/excluir branch usa os mesmos casos de uso da TUI.
2. Fetch/pull/push exibem remote, branch, upstream e resultado.
3. Erros, divergência e bloqueios mantêm os mesmos códigos/semântica do Core.

**Definition of Done:** DOD-G + E2E compara estados Git resultantes com os fluxos da TUI.

<a id="us-061"></a>
#### US-061 — Apresentar intenção, risco e resultado

- **Épico:** EPIC-12
- **Versão alvo:** v0.3
- **MoSCoW:** Must
- **Componente:** Desktop
- **Dependências:** [US-054](#us-054), [US-111](#us-111)
- **Status:** Planejada

**Como** desenvolvedor, **quero** entender a ação antes de confirmá-la, **para** manter controle sobre alterações.

**Critérios de aceite:**

1. Diálogos mostram operação Git, alvo e impacto conhecido.
2. Cancelar antes de confirmar não muda o repositório.
3. Concluir/falhar atualiza estado e fornece mensagem segura e remediação.

**Definition of Done:** DOD-G + Testes cobrem confirmação, cancelamento e resultado; ações destrutivas não usam texto genérico.

<a id="us-062"></a>
#### US-062 — Consultar blame, tags, stash e remotes

- **Épico:** EPIC-12
- **Versão alvo:** v0.3
- **MoSCoW:** Must
- **Componente:** Desktop
- **Dependências:** [US-053](#us-053), [US-031](#us-031), [US-091](#us-091)
- **Status:** Planejada

**Como** desenvolvedor, **quero** consultar os recursos já disponíveis na TUI, **para** usar Desktop como cliente cotidiano completo.

**Critérios de aceite:**

1. Blame de arquivo permite relacionar linha e commit.
2. Sidebar mostra tags, remotes e stash com detalhes disponíveis.
3. Empty/error/loading são explícitos e leitura não altera repositório.

**Definition of Done:** DOD-G + E2E de paridade de consulta com TUI e fixtures de referências aprovado.

<a id="us-063"></a>
#### US-063 — Oferecer drag/drop apenas para ações seguras

- **Épico:** EPIC-12
- **Versão alvo:** v0.3
- **MoSCoW:** Could
- **Componente:** Desktop
- **Dependências:** [US-058](#us-058)
- **Status:** Planejada

**Como** desenvolvedor, **quero** usar arrastar e soltar em ações inequívocas, **para** agilizar tarefas frequentes.

**Critérios de aceite:**

1. Cada destino comunica ação e objeto afetado antes da soltura.
2. Drag/drop não dispara mutação destrutiva sem a mesma confirmação de outras entradas.
3. Todas as ações possuem alternativa por teclado/menu; gestos ambíguos não são habilitados.

**Definition of Done:** DOD-G + Revisão de UX e teste de cancelamento demonstram ausência de operações acidentais.

<a id="epic-13"></a>
### EPIC-13 — Commit Graph

Compartilhar semântica do DAG mantendo renderização própria por interface.

**Rastreabilidade:** PRD §§8.2–8.3, 9.2–9.3, 13.1; SAD §§24–25, 32; ADR-011. As histórias deste épico detalham essas fontes; critérios adicionais são refinamentos propostos nesta baseline.

<a id="us-064"></a>
#### US-064 — Calcular linhas e lanes compartilhadas

- **Épico:** EPIC-13
- **Versão alvo:** v0.2
- **MoSCoW:** Must
- **Componente:** Core / Graph
- **Dependências:** [US-015](#us-015)
- **Status:** Planejada

**Como** desenvolvedor, **quero** um graph que represente parents e referências, **para** entender relações entre commits.

**Critérios de aceite:**

1. Layout recebe hashes, parents, decorations e limites de página.
2. Saída independe de Ratatui, Vue e Tauri.
3. Merge, root e múltiplas branches preservam arestas e identidade dos commits.

**Definition of Done:** DOD-G + Testes puros com DAGs conhecidos verificam conectividade e determinismo.

<a id="us-065"></a>
#### US-065 — Preservar continuidade entre páginas do graph

- **Épico:** EPIC-13
- **Versão alvo:** v0.2
- **MoSCoW:** Must
- **Componente:** Core / Graph
- **Dependências:** [US-064](#us-064)
- **Status:** Planejada

**Como** desenvolvedor, **quero** carregar histórico progressivamente, **para** explorar grandes repositórios sem grafos enganosos.

**Critérios de aceite:**

1. Arestas para parents fora da página têm continuidade identificável.
2. Shallow history e filtros não inventam conexões.
3. Anexar página preserva seleção por hash e política de estabilidade de lanes documentada.

**Definition of Done:** DOD-G + Fixtures paginadas e shallow validam fronteiras, continuação e ausência de arestas falsas.

<a id="us-066"></a>
#### US-066 — Renderizar graph na TUI

- **Épico:** EPIC-13
- **Versão alvo:** v0.2
- **MoSCoW:** Must
- **Componente:** TUI
- **Dependências:** [US-065](#us-065), [US-040](#us-040)
- **Status:** Planejada

**Como** usuário de terminal, **quero** um graph legível no terminal, **para** entender branches pelo teclado.

**Critérios de aceite:**

1. Renderer usa layout compartilhado e associa linha ao commit.
2. Cores são acompanhadas por formas/conectores legíveis.
3. Scroll e seleção funcionam com paginação e resize.

**Definition of Done:** DOD-G + Snapshots seletivos e navegação em DAG com merge aprovados.

<a id="us-067"></a>
#### US-067 — Renderizar graph interativo no Desktop

- **Épico:** EPIC-13
- **Versão alvo:** v0.3
- **MoSCoW:** Must
- **Componente:** Desktop
- **Dependências:** [US-065](#us-065), [US-051](#us-051)
- **Status:** Planejada

**Como** usuário Desktop, **quero** explorar commits em um graph interativo, **para** navegar relações visualmente.

**Critérios de aceite:**

1. Seleção, hover e context menu referem-se ao hash correto.
2. Renderização é incremental/virtualizada quando necessária.
3. Filtro e carregamento adicional preservam semântica do layout compartilhado.

**Definition of Done:** DOD-G + Teste de seleção após scroll/paginação e comparação de arestas com Core aprovado.

<a id="epic-14"></a>
### EPIC-14 — VS Code Foundation

Integrar o editor ao Core por CLI JSON sem reimplementar Git.

**Rastreabilidade:** PRD §§10.1–10.4, 22; SAD §§14–16, 26, 33, 36; ADR-007–008 e ADR-012. As histórias deste épico detalham essas fontes; critérios adicionais são refinamentos propostos nesta baseline.

<a id="us-068"></a>
#### US-068 — Ativar extensão e identificar contexto

- **Épico:** EPIC-14
- **Versão alvo:** v0.4
- **MoSCoW:** Must
- **Componente:** VSCode
- **Dependências:** [US-037](#us-037)
- **Status:** Planejada

**Como** usuário do VS Code, **quero** ativar GitSail ao trabalhar em um projeto, **para** receber contexto Git no editor.

**Critérios de aceite:**

1. Extensão TypeScript usa descoberta pelo Core.
2. Arquivo ativo é associado ao repositório correto, inclusive em workspace com múltiplas pastas.
3. Arquivo sem repositório não gera erro repetitivo ou operação indevida.

**Definition of Done:** DOD-G + Extension tests cobrem ativação, troca de pasta e arquivo externo.

<a id="us-069"></a>
#### US-069 — Consumir CLI JSON por cliente isolado

- **Épico:** EPIC-14
- **Versão alvo:** v0.4
- **MoSCoW:** Must
- **Componente:** VSCode / Protocol
- **Dependências:** [US-068](#us-068), [US-038](#us-038)
- **Status:** Planejada

**Como** usuário do VS Code, **quero** consultas locais confiáveis ao GitSail, **para** usar blame/history sem duplicação de lógica.

**Critérios de aceite:**

1. Cliente executa processo por argumentos e valida envelope/schema.
2. Binário ausente/incompatível gera orientação e não tenta parsing Git alternativo.
3. Timeout, cancelamento e encerramento limpam processos/recursos.

**Definition of Done:** DOD-G + Contract tests com respostas válidas, inválidas e incompatíveis aprovados.

<a id="us-070"></a>
#### US-070 — Localizar e configurar binário da extensão

- **Épico:** EPIC-14
- **Versão alvo:** v0.4
- **MoSCoW:** Must
- **Componente:** VSCode / Distribuição
- **Dependências:** [US-069](#us-069)
- **Status:** Planejada

**Como** usuário do VS Code, **quero** configurar um binário compatível, **para** instalar e usar a extensão com previsibilidade.

**Critérios de aceite:**

1. Caminho explícito e descoberta documentada são suportados.
2. Estratégia de entrega do binário é decidida e registrada antes do empacotamento da v0.4.
3. Origem/versão são verificáveis; não há download/execução silenciosa de arquivo não confiável.

**Definition of Done:** DOD-G + Teste de instalação limpa, caminho inválido e versão incompatível; decisão de distribuição registrada.

<a id="us-071"></a>
#### US-071 — Respeitar ciclo de vida e confiança do workspace

- **Épico:** EPIC-14
- **Versão alvo:** v0.4
- **MoSCoW:** Must
- **Componente:** VSCode
- **Dependências:** [US-069](#us-069), [US-110](#us-110)
- **Status:** Planejada

**Como** usuário do VS Code, **quero** uma extensão que respeite o contexto do editor, **para** evitar resultados incorretos e execução indevida.

**Critérios de aceite:**

1. Desativação e troca de editor removem subscriptions, decorações e consultas pendentes.
2. Política de workspace trust controla execução de processos e configurações locais sensíveis.
3. Buffers não salvos têm tratamento explícito sem atribuir blame do disco como atual.

**Definition of Done:** DOD-G + Extension tests incluem workspace não confiável, buffer divergente e desativação durante consulta.

<a id="epic-15"></a>
### EPIC-15 — VS Code Blame & History

Responder quem, quando e por quê diretamente no código.

**Rastreabilidade:** PRD §§10.2–10.5; SAD §§16, 23, 26. As histórias deste épico detalham essas fontes; critérios adicionais são refinamentos propostos nesta baseline.

<a id="us-072"></a>
#### US-072 — Exibir blame inline opcional

- **Épico:** EPIC-15
- **Versão alvo:** v0.4
- **MoSCoW:** Must
- **Componente:** VSCode
- **Dependências:** [US-071](#us-071), [US-034](#us-034), [US-108](#us-108)
- **Status:** Planejada

**Como** desenvolvedor, **quero** ver autoria da linha no editor, **para** entender o contexto sem trocar de ferramenta.

**Critérios de aceite:**

1. Exibe autor, data, hash abreviado e mensagem conforme formato.
2. Pode ser desligado e respeita delay/modo linha atual ou múltiplas linhas.
3. Buffer divergente e linhas locais têm indicação correta sem autor fictício.

**Definition of Done:** DOD-G + Teste de edição, navegação rápida e desativação verifica decorações e invalidação.

<a id="us-073"></a>
#### US-073 — Exibir hover e detalhes do commit

- **Épico:** EPIC-15
- **Versão alvo:** v0.4
- **MoSCoW:** Must
- **Componente:** VSCode
- **Dependências:** [US-072](#us-072), [US-016](#us-016)
- **Status:** Planejada

**Como** desenvolvedor, **quero** abrir detalhes a partir da autoria, **para** entender a motivação da mudança.

**Critérios de aceite:**

1. Hover apresenta commit, autor/data e mensagem de forma segura.
2. Ação abre detalhes sem depender de parsing no editor.
3. Texto do repositório não injeta comandos ou markup ativo não autorizado.

**Definition of Done:** DOD-G + Extension test com mensagem maliciosa e commit normal valida conteúdo e ações.

<a id="us-074"></a>
#### US-074 — Navegar histórico de arquivo

- **Épico:** EPIC-15
- **Versão alvo:** v0.4
- **MoSCoW:** Must
- **Componente:** VSCode
- **Dependências:** [US-069](#us-069), [US-018](#us-018)
- **Status:** Planejada

**Como** desenvolvedor, **quero** consultar commits de um arquivo, **para** revisar sua evolução.

**Critérios de aceite:**

1. Lista paginada preserva contexto do arquivo e revisão.
2. Selecionar commit abre detalhes e permite navegar para diff.
3. Rename, remoção e histórico vazio têm estados explícitos.

**Definition of Done:** DOD-G + Teste de arquivo renomeado e paginação comprova integração com Core.

<a id="us-075"></a>
#### US-075 — Navegar histórico de linha

- **Épico:** EPIC-15
- **Versão alvo:** v0.4
- **MoSCoW:** Must
- **Componente:** VSCode
- **Dependências:** [US-069](#us-069), [US-019](#us-019)
- **Status:** Planejada

**Como** desenvolvedor, **quero** consultar mudanças de um trecho, **para** investigar uma decisão no código.

**Critérios de aceite:**

1. Comando usa seleção/linha e revisão identificadas.
2. Resultados mostram mudanças e commits associados.
3. Limitações e conteúdo não salvo são apresentados sem atribuição inventada.

**Definition of Done:** DOD-G + Fixture com deslocamento de linhas valida referência de origem e navegação.

<a id="us-076"></a>
#### US-076 — Abrir diff e copiar hash do commit

- **Épico:** EPIC-15
- **Versão alvo:** v0.4
- **MoSCoW:** Must
- **Componente:** VSCode
- **Dependências:** [US-073](#us-073), [US-026](#us-026)
- **Status:** Planejada

**Como** desenvolvedor, **quero** abrir a comparação e copiar o hash, **para** compartilhar ou revisar uma alteração.

**Critérios de aceite:**

1. Diff usa conteúdo/base do commit escolhido e informa política para merge.
2. Copiar hash entrega hash completo da seleção.
3. Conteúdo histórico abre como leitura sem sobrescrever arquivo de trabalho.

**Definition of Done:** DOD-G + Teste valida lados do diff, clipboard e preservação da working tree.

<a id="us-077"></a>
#### US-077 — Abrir commit no GitSail Desktop

- **Épico:** EPIC-15
- **Versão alvo:** v0.4
- **MoSCoW:** Must
- **Componente:** VSCode / Desktop
- **Dependências:** [US-073](#us-073), [US-056](#us-056)
- **Status:** Planejada

**Como** desenvolvedor, **quero** continuar a investigação no Desktop, **para** alternar interfaces mantendo contexto.

**Critérios de aceite:**

1. Quando Desktop está disponível, abre repositório e seleciona o mesmo hash.
2. Mecanismo de handoff valida argumentos e é documentado sem exigir daemon.
3. Desktop ausente apresenta alternativa útil sem falhar o restante da extensão.

**Definition of Done:** DOD-G + Teste de integração verifica hash/repo e caso de aplicativo ausente.

<a id="epic-16"></a>
### EPIC-16 — Merge & Conflicts

Executar integrações e resolver conflitos com estado explícito.

**Rastreabilidade:** PRD §§11.1–11.4; SAD §§9, 19–22, 26. As histórias deste épico detalham essas fontes; critérios adicionais são refinamentos propostos nesta baseline.

<a id="us-078"></a>
#### US-078 — Detectar operações e conflitos em andamento

- **Épico:** EPIC-16
- **Versão alvo:** v0.5
- **MoSCoW:** Must
- **Componente:** Core / Git Provider
- **Dependências:** [US-010](#us-010), [US-009](#us-009)
- **Status:** Planejada

**Como** desenvolvedor, **quero** identificar merge, rebase e cherry-pick pendentes, **para** escolher ações válidas de recuperação.

**Critérios de aceite:**

1. Estado distingue operação, arquivos em conflito e capacidades disponíveis.
2. Operação iniciada externamente é reconhecida no refresh.
3. Operações incompatíveis ficam bloqueadas sem apagar metadados Git.

**Definition of Done:** DOD-G + Fixtures de merge/rebase/cherry-pick em andamento validam estados e transições.

<a id="us-079"></a>
#### US-079 — Executar merge com prévia de intenção

- **Épico:** EPIC-16
- **Versão alvo:** v0.5
- **MoSCoW:** Must
- **Componente:** Core / TUI / Desktop
- **Dependências:** [US-078](#us-078), [US-111](#us-111), [US-116](#us-116), [US-044](#us-044), [US-061](#us-061)
- **Status:** Planejada

**Como** desenvolvedor, **quero** integrar uma referência na branch atual, **para** combinar linhas de trabalho.

**Critérios de aceite:**

1. Origem, destino e política de merge aparecem antes da execução.
2. Fast-forward, merge commit e conflito são resultados distintos.
3. TUI e Desktop exibem estado atualizado e não relatam conflito como sucesso completo.

**Definition of Done:** DOD-G + Integração com três resultados e fluxos nas duas interfaces aprovada.

<a id="us-080"></a>
#### US-080 — Guiar resolução de arquivos em conflito

- **Épico:** EPIC-16
- **Versão alvo:** v0.5
- **MoSCoW:** Must
- **Componente:** Core / TUI / Desktop
- **Dependências:** [US-079](#us-079), [US-027](#us-027)
- **Status:** Planejada

**Como** desenvolvedor, **quero** revisar arquivos conflitantes e marcar resoluções, **para** concluir uma integração com controle.

**Critérios de aceite:**

1. Lista identifica arquivos/stages em conflito.
2. Usuário pode inspecionar versões e editar por fluxo suportado, inclusive solução externa.
3. Marcar resolvido atualiza index apenas por ação explícita; binários têm fluxo documentado.

**Definition of Done:** DOD-G + Fixture textual e binária valida marcação de resolução sem escolha automática de lado.

<a id="us-081"></a>
#### US-081 — Continuar ou abortar operações suportadas

- **Épico:** EPIC-16
- **Versão alvo:** v0.5
- **MoSCoW:** Must
- **Componente:** Core / TUI / Desktop
- **Dependências:** [US-080](#us-080)
- **Status:** Planejada

**Como** desenvolvedor, **quero** continuar ou abortar uma operação pendente, **para** recuperar um estado utilizável.

**Critérios de aceite:**

1. Ações disponíveis refletem merge/rebase/cherry-pick detectado.
2. Continue verifica conflitos restantes e pré-condições.
3. Abort informa alcance e possíveis limitações; resultado real é reinspecionado.

**Definition of Done:** DOD-G + Testes cobrem sucesso, falha e abort sem prometer recuperação de alterações que o Git não garante.

<a id="us-082"></a>
#### US-082 — Consolidar UX de conflito entre TUI e Desktop

- **Épico:** EPIC-16
- **Versão alvo:** v0.5
- **MoSCoW:** Must
- **Componente:** TUI / Desktop
- **Dependências:** [US-081](#us-081), [US-083](#us-083), [US-086](#us-086)
- **Status:** Planejada

**Como** desenvolvedor, **quero** um fluxo consistente de conflito, **para** alternar interfaces sem perder entendimento.

**Critérios de aceite:**

1. Ambas mostram operação, progresso conhecido e arquivos pendentes.
2. Restart ou mudança externa reconstrói estado a partir de Git.
3. Ações inválidas são indisponíveis e erros preservam opções de recuperação.

**Definition of Done:** DOD-G + E2E inicia operação numa interface e verifica recuperação na outra via estado Git.

<a id="epic-17"></a>
### EPIC-17 — Rebase & History Editing

Editar histórico com intenção explícita e recuperação quando disponível.

**Rastreabilidade:** PRD §§9.3, 11.2–11.3, 23; SAD §§9, 20–22, 26. As histórias deste épico detalham essas fontes; critérios adicionais são refinamentos propostos nesta baseline.

<a id="us-083"></a>
#### US-083 — Executar rebase sobre uma base escolhida

- **Épico:** EPIC-17
- **Versão alvo:** v0.5
- **MoSCoW:** Must
- **Componente:** Core / TUI / Desktop
- **Dependências:** [US-078](#us-078), [US-111](#us-111), [US-116](#us-116), [US-044](#us-044), [US-061](#us-061)
- **Status:** Planejada

**Como** desenvolvedor, **quero** reaplicar commits sobre outra base, **para** atualizar minha linha de trabalho.

**Critérios de aceite:**

1. Prévia identifica branch, base e reescrita esperada.
2. Estado sujo/incompatível é bloqueado ou tratado explicitamente sem stash automático oculto.
3. Conflito permanece visível e usa capacidades de recuperação do Git.

**Definition of Done:** DOD-G + Fixtures de sucesso e conflito verificam histórico e preservação de trabalho.

<a id="us-084"></a>
#### US-084 — Planejar rebase interativo básico

- **Épico:** EPIC-17
- **Versão alvo:** v0.5
- **MoSCoW:** Must
- **Componente:** Core / TUI / Desktop
- **Dependências:** [US-083](#us-083)
- **Status:** Planejada

**Como** desenvolvedor, **quero** reordenar e escolher ações de commits, **para** organizar meu histórico antes de compartilhar.

**Critérios de aceite:**

1. Plano mostra faixa, ordem e ações suportadas antes de confirmar.
2. Referências e HEAD são revalidados; plano inválido não executa.
3. Editor do plano usa fronteira controlada sem injetar shell; UI visual avançada fica pós-v1.0.

**Definition of Done:** DOD-G + Teste de plano válido/inválido e alteração concorrente comprova segurança da execução.

<a id="us-085"></a>
#### US-085 — Aplicar squash e fixup

- **Épico:** EPIC-17
- **Versão alvo:** v0.5
- **MoSCoW:** Must
- **Componente:** Core / TUI / Desktop
- **Dependências:** [US-084](#us-084)
- **Status:** Planejada

**Como** desenvolvedor, **quero** combinar commits relacionados, **para** produzir histórico legível.

**Critérios de aceite:**

1. Plano distingue squash de fixup e seus efeitos na mensagem.
2. Primeira posição ou combinação inválida é recusada.
3. Conflitos mantêm estado e permitem recuperação pelo workflow compartilhado.

**Definition of Done:** DOD-G + Fixture verifica árvore resultante, quantidade de commits e política de mensagens.

<a id="us-086"></a>
#### US-086 — Executar cherry-pick

- **Épico:** EPIC-17
- **Versão alvo:** v0.5
- **MoSCoW:** Must
- **Componente:** Core / TUI / Desktop
- **Dependências:** [US-078](#us-078), [US-111](#us-111), [US-116](#us-116), [US-044](#us-044), [US-061](#us-061)
- **Status:** Planejada

**Como** desenvolvedor, **quero** aplicar um commit selecionado, **para** reutilizar uma mudança em outra branch.

**Critérios de aceite:**

1. Commit e destino ficam explícitos.
2. Commit merge exige política de parent suportada ou recusa clara.
3. Conflito e operação vazia são diferenciados de sucesso e permitem recuperação aplicável.

**Definition of Done:** DOD-G + Fixtures de sucesso, conflito e merge validam resultado e limitações.

<a id="us-087"></a>
#### US-087 — Reverter efeito de commit

- **Épico:** EPIC-17
- **Versão alvo:** v0.5
- **MoSCoW:** Must
- **Componente:** Core / TUI / Desktop
- **Dependências:** [US-078](#us-078), [US-111](#us-111), [US-116](#us-116), [US-044](#us-044), [US-061](#us-061)
- **Status:** Planejada

**Como** desenvolvedor, **quero** desfazer uma mudança por novo commit, **para** preservar a história compartilhada.

**Critérios de aceite:**

1. Prévia identifica commit e destino.
2. Sucesso cria reversão sem mover silenciosamente referências antigas.
3. Conflito e merge sem parent definido são tratados explicitamente.

**Definition of Done:** DOD-G + Teste compara árvore antes/depois e confirma preservação do histórico anterior.

<a id="us-088"></a>
#### US-088 — Executar reset soft, mixed e hard

- **Épico:** EPIC-17
- **Versão alvo:** v0.5
- **MoSCoW:** Must
- **Componente:** Core / TUI / Desktop
- **Dependências:** [US-078](#us-078), [US-111](#us-111), [US-116](#us-116), [US-044](#us-044), [US-061](#us-061)
- **Status:** Planejada

**Como** desenvolvedor, **quero** escolher a modalidade de reset, **para** ajustar HEAD, index e working tree conscientemente.

**Critérios de aceite:**

1. Prévia diferencia efeitos de cada modo sobre HEAD/index/working tree.
2. Hard exige confirmação reforçada com alvo e perdas previstas.
3. HEAD/estado são revalidados e mudança concorrente exige nova avaliação.

**Definition of Done:** DOD-G + Três fixtures verificam separadamente efeitos de soft/mixed/hard e cancelamento sem mutação.

<a id="us-089"></a>
#### US-089 — Inspecionar reflog

- **Épico:** EPIC-17
- **Versão alvo:** v0.5
- **MoSCoW:** Must
- **Componente:** Core / TUI / Desktop
- **Dependências:** [US-015](#us-015), [US-045](#us-045), [US-056](#us-056)
- **Status:** Planejada

**Como** desenvolvedor, **quero** consultar movimentos de referências, **para** localizar estados anteriores do trabalho.

**Critérios de aceite:**

1. Lista entradas com referência, hash, data e mensagem disponíveis.
2. Entrada selecionada abre commit quando objeto existe.
3. Expiração/objeto ausente é informada; inspeção não executa reset.

**Definition of Done:** DOD-G + Fixture com amend/reset verifica entradas e comportamento de objeto indisponível.

<a id="us-090"></a>
#### US-090 — Disponibilizar amend na TUI

- **Épico:** EPIC-17
- **Versão alvo:** v0.5
- **MoSCoW:** Must
- **Componente:** TUI
- **Dependências:** [US-059](#us-059), [US-047](#us-047)
- **Status:** Planejada

**Como** desenvolvedor, **quero** alterar último commit pelo terminal, **para** ter a mesma semântica de amend do Desktop.

**Critérios de aceite:**

1. TUI usa o caso de uso existente sem implementação Git paralela.
2. Confirmação identifica commit substituído e risco de publicação.
3. Falha preserva mensagem e alterações; sucesso refresca graph e status.

**Definition of Done:** DOD-G + E2E verifica equivalência do amend TUI/Desktop e proteção de HEAD obsoleto.

<a id="epic-18"></a>
### EPIC-18 — Stash, Tags & Worktrees

Administrar estados auxiliares sem antecipar gestores avançados de workspace.

**Rastreabilidade:** PRD §§8.3, 11.2–11.3, 23; SAD §§8–9, 20–21, 26. As histórias deste épico detalham essas fontes; critérios adicionais são refinamentos propostos nesta baseline.

<a id="us-091"></a>
#### US-091 — Consultar tags, remotes e stash

- **Épico:** EPIC-18
- **Versão alvo:** v0.2
- **MoSCoW:** Must
- **Componente:** Core / Git Provider
- **Dependências:** [US-005](#us-005), [US-007](#us-007)
- **Status:** Planejada

**Como** desenvolvedor, **quero** listar referências e trabalho guardado, **para** inspecionar meu repositório.

**Critérios de aceite:**

1. Tags incluem alvo e metadados disponíveis; remotes distinguem fetch/push URL.
2. Stash inclui índice, commit, mensagem e data disponíveis.
3. URLs com credenciais são redigidas em diagnósticos; listas vazias são válidas.

**Definition of Done:** DOD-G + Fixtures annotated/lightweight tags, stash e remote sem credenciais expostas aprovadas.

<a id="us-092"></a>
#### US-092 — Criar stash com escopo explícito

- **Épico:** EPIC-18
- **Versão alvo:** v0.5
- **MoSCoW:** Must
- **Componente:** Core / TUI / Desktop
- **Dependências:** [US-091](#us-091), [US-010](#us-010), [US-111](#us-111), [US-116](#us-116), [US-044](#us-044), [US-061](#us-061)
- **Status:** Planejada

**Como** desenvolvedor, **quero** guardar mudanças temporariamente, **para** trocar de contexto sem perder trabalho.

**Critérios de aceite:**

1. Prévia distingue tracked/untracked e comportamento do index.
2. Mensagem e escopo são explícitos; não inclui ignored silenciosamente.
3. Após execução mostra stash criado e estado local resultante.

**Definition of Done:** DOD-G + Fixture verifica conteúdo guardado e arquivos excluídos do escopo preservados.

<a id="us-093"></a>
#### US-093 — Aplicar, pop e excluir stash

- **Épico:** EPIC-18
- **Versão alvo:** v0.5
- **MoSCoW:** Must
- **Componente:** Core / TUI / Desktop
- **Dependências:** [US-092](#us-092), [US-078](#us-078)
- **Status:** Planejada

**Como** desenvolvedor, **quero** recuperar ou remover trabalho guardado, **para** administrar mudanças temporárias.

**Critérios de aceite:**

1. Apply e pop têm consequências distintas e usam identidade revalidada do stash.
2. Conflito não é apresentado como restauração concluída e preservação do stash é verificada.
3. Drop exige confirmação reforçada com entrada exata.

**Definition of Done:** DOD-G + Integração cobre apply/pop com conflito, drop cancelado e índice alterado externamente.

<a id="us-094"></a>
#### US-094 — Criar e excluir tags locais

- **Épico:** EPIC-18
- **Versão alvo:** v0.5
- **MoSCoW:** Must
- **Componente:** Core / TUI / Desktop
- **Dependências:** [US-091](#us-091), [US-111](#us-111), [US-116](#us-116), [US-044](#us-044), [US-061](#us-061)
- **Status:** Planejada

**Como** desenvolvedor, **quero** gerenciar tags locais, **para** marcar versões do projeto.

**Critérios de aceite:**

1. Nome, alvo e tipo suportado são explícitos; colisão não sobrescreve.
2. Exclusão confirma referência local exata.
3. Ação local não publica nem exclui tag remota implicitamente.

**Definition of Done:** DOD-G + Fixtures validam criação, exclusão e preservação de remotes; tipos suportados documentados.

<a id="us-095"></a>
#### US-095 — Listar, criar e remover worktrees básicos

- **Épico:** EPIC-18
- **Versão alvo:** v0.5
- **MoSCoW:** Must
- **Componente:** Core / TUI / Desktop
- **Dependências:** [US-020](#us-020), [US-008](#us-008), [US-111](#us-111), [US-116](#us-116), [US-044](#us-044), [US-061](#us-061)
- **Status:** Planejada

**Como** desenvolvedor, **quero** usar worktrees Git, **para** trabalhar em branches simultâneas.

**Critérios de aceite:**

1. Lista associa path, branch/HEAD e estado disponível.
2. Criação valida caminho e restrições de branch em uso.
3. Remoção recusa perda de trabalho sem ação explicitamente suportada; não inclui gestor avançado de workspace.

**Definition of Done:** DOD-G + Fixtures múltiplas e dirty worktree validam bloqueios, caminhos e refresh entre sessões.

<a id="epic-19"></a>
### EPIC-19 — Remote Operations

Sincronizar com Git remoto usando autenticação existente.

**Rastreabilidade:** PRD §§8.3, 11.2–11.3, 13.2; SAD §§9, 19–20, 26–28; ADR-010. As histórias deste épico detalham essas fontes; critérios adicionais são refinamentos propostos nesta baseline.

<a id="us-096"></a>
#### US-096 — Buscar atualizações do remote

- **Épico:** EPIC-19
- **Versão alvo:** v0.2
- **MoSCoW:** Must
- **Componente:** Core / Git Provider
- **Dependências:** [US-091](#us-091), [US-112](#us-112), [US-116](#us-116)
- **Status:** Planejada

**Como** desenvolvedor, **quero** executar fetch no remote escolhido, **para** atualizar referências remotas.

**Critérios de aceite:**

1. Remote é explícito e processo não bloqueia UI.
2. Progresso, falha de rede/autenticação e cancelamento têm resultados distintos.
3. Conclusão atualiza refs/ahead-behind sem alterar working tree.

**Definition of Done:** DOD-G + Remote local de teste comprova refs atualizadas e árvore preservada.

<a id="us-097"></a>
#### US-097 — Integrar mudanças do upstream

- **Épico:** EPIC-19
- **Versão alvo:** v0.2
- **MoSCoW:** Must
- **Componente:** Core / Git Provider
- **Dependências:** [US-096](#us-096), [US-021](#us-021)
- **Status:** Planejada

**Como** desenvolvedor, **quero** executar pull com política clara, **para** sincronizar minha branch.

**Critérios de aceite:**

1. Remote/upstream e política adotada ficam visíveis.
2. Política inicial segura, incluindo recusa de divergência não suportada, é documentada.
3. Conflito é informado e não provoca reset/rebase ou descarte oculto.

**Definition of Done:** DOD-G + Fixtures fast-forward e divergência validam resultado; recuperação avançada permanece na v0.5.

<a id="us-098"></a>
#### US-098 — Publicar branch por push comum

- **Épico:** EPIC-19
- **Versão alvo:** v0.2
- **MoSCoW:** Must
- **Componente:** Core / Git Provider
- **Dependências:** [US-096](#us-096), [US-111](#us-111)
- **Status:** Planejada

**Como** desenvolvedor, **quero** enviar commits ao remote escolhido, **para** compartilhar meu trabalho.

**Critérios de aceite:**

1. Remote e branch destino/upstream são explícitos.
2. Rejeição non-fast-forward não ativa force automaticamente.
3. Erro de autenticação/rede não informa sucesso e estado remoto pode ser reinspecionado.

**Definition of Done:** DOD-G + Remote fixture cobre push aceito/rejeitado e ausência de força implícita.

<a id="us-099"></a>
#### US-099 — Oferecer force push protegido

- **Épico:** EPIC-19
- **Versão alvo:** v0.5
- **MoSCoW:** Must
- **Componente:** Core / Git Provider / TUI / Desktop
- **Dependências:** [US-098](#us-098), [US-111](#us-111), [US-044](#us-044), [US-061](#us-061)
- **Status:** Planejada

**Como** desenvolvedor, **quero** publicar histórico reescrito conscientemente, **para** atualizar uma branch sem sobrescrever trabalho inesperado.

**Critérios de aceite:**

1. Ação tem nome distinto de push e confirmação reforçada.
2. Destino e estado remoto esperado são apresentados; proteção equivalente a lease recusa avanço inesperado.
3. Falha de proteção não faz fallback automático para força irrestrita.

**Definition of Done:** DOD-G + Teste com avanço concorrente do remote comprova rejeição e preservação do trabalho remoto.

<a id="us-100"></a>
#### US-100 — Consolidar recuperação e diagnóstico de transporte

- **Épico:** EPIC-19
- **Versão alvo:** v1.0
- **MoSCoW:** Must
- **Componente:** Core / TUI / Desktop
- **Dependências:** [US-097](#us-097), [US-098](#us-098), [US-099](#us-099), [US-113](#us-113)
- **Status:** Planejada

**Como** desenvolvedor, **quero** entender operações remotas interrompidas, **para** retomar sincronização com segurança.

**Critérios de aceite:**

1. Timeout/cancelamento informa estado conhecido sem garantir rollback remoto.
2. Nova tentativa reconsulta estado quando necessário e não repete escrita cegamente.
3. Mensagens orientam credenciais/SSH/rede sem expor segredos.

**Definition of Done:** DOD-G + Teste de interrupção e reconciliação após resultado incerto acompanha troubleshooting.

<a id="epic-20"></a>
### EPIC-20 — GitHub/GitLab Integration

Adicionar contexto de forges sem tornar Git local dependente de conta.

**Rastreabilidade:** PRD §§3, 12.3, 13.2–13.3, 22–23; SAD §§3, 27, 33–34; ADR-010. As histórias deste épico detalham essas fontes; critérios adicionais são refinamentos propostos nesta baseline.

<a id="us-101"></a>
#### US-101 — Detectar forge e abrir links contextualizados

- **Épico:** EPIC-20
- **Versão alvo:** v1.0
- **MoSCoW:** Must
- **Componente:** Integrações / TUI / Desktop
- **Dependências:** [US-091](#us-091), [US-110](#us-110)
- **Status:** Planejada

**Como** desenvolvedor, **quero** abrir repository, branch e commit no navegador, **para** consultar contexto remoto.

**Critérios de aceite:**

1. URLs GitHub/GitLab suportadas são reconhecidas e normalizadas.
2. Links codificam paths/refs e aceitam somente destinos/esquemas permitidos.
3. Remote desconhecido mantém Git local funcional sem link inventado.

**Definition of Done:** DOD-G + Fixtures de URLs SSH/HTTPS e maliciosas validam detecção e navegação por ação explícita.

<a id="us-102"></a>
#### US-102 — Autorizar consultas às APIs de forge

- **Épico:** EPIC-20
- **Versão alvo:** v1.0
- **MoSCoW:** Must
- **Componente:** Integrações
- **Dependências:** [US-101](#us-101), [US-112](#us-112)
- **Status:** Planejada

**Como** desenvolvedor, **quero** conectar minha conta de forge opcionalmente, **para** consultar PRs/MRs privados com controle.

**Critérios de aceite:**

1. Tokens usam armazenamento seguro do OS e escopo mínimo necessário.
2. Conectar/desconectar é explícito e remove acesso local quando solicitado.
3. Falha, expiração ou falta de conta não bloqueia consultas Git locais.

**Definition of Done:** DOD-G + Testes com storage/API doubles validam revogação, expiração e ausência de token em logs.

<a id="us-103"></a>
#### US-103 — Consultar Pull/Merge Requests em escopo limitado

- **Épico:** EPIC-20
- **Versão alvo:** v1.0
- **MoSCoW:** Must
- **Componente:** Integrações / Desktop
- **Dependências:** [US-102](#us-102)
- **Status:** Planejada

**Como** desenvolvedor, **quero** listar e abrir PRs/MRs relacionados ao repositório, **para** acompanhar revisão sem um cliente completo de projetos.

**Critérios de aceite:**

1. Escopo inicial lista título, estado, autor e branches quando disponíveis.
2. Paginação, rate limit, offline e permissão insuficiente têm estados explícitos.
3. Conteúdo externo é tratado como não confiável; abrir no navegador é ação do usuário.

**Definition of Done:** DOD-G + Contrato dos adapters GitHub/GitLab e falhas de API verificados; escopo limitado documentado.

<a id="us-104"></a>
#### US-104 — Criar PR/MR se autenticação e UX estiverem maduras

- **Épico:** EPIC-20
- **Versão alvo:** v1.0
- **MoSCoW:** Could
- **Componente:** Integrações / Desktop
- **Dependências:** [US-103](#us-103)
- **Status:** Planejada

**Como** desenvolvedor, **quero** criar uma solicitação de revisão, **para** compartilhar uma branch sem retrabalho.

**Critérios de aceite:**

1. Usuário revisa provider, projeto, base, head, título e descrição antes do envio.
2. Não há publicação silenciosa; repetição após resposta incerta verifica possível criação anterior.
3. Entrega é condicionada a autenticação/UX estáveis e pode ser adiada sem bloquear Git local.

**Definition of Done:** DOD-G + Teste com API simulada cobre erro e resposta incerta; decisão de inclusão registrada.

<a id="epic-21"></a>
### EPIC-21 — Settings & Personalization

Oferecer preferências coerentes sem alterar configuração Git indevidamente.

**Rastreabilidade:** PRD §§9.3, 10.4, 12.2, 13.4–13.5; SAD §29. As histórias deste épico detalham essas fontes; critérios adicionais são refinamentos propostos nesta baseline.

<a id="us-105"></a>
#### US-105 — Persistir preferências com escopos claros

- **Épico:** EPIC-21
- **Versão alvo:** v0.3
- **MoSCoW:** Must
- **Componente:** Core / Desktop
- **Dependências:** [US-051](#us-051)
- **Status:** Planejada

**Como** usuário, **quero** preferências locais previsíveis, **para** personalizar GitSail sem alterar o comportamento do Git.

**Critérios de aceite:**

1. Precedência é defaults, usuário e repositório quando justificado.
2. Preferências de UI não escrevem .git/config.
3. Formato de persistência é decidido/documentado; arquivo inválido gera fallback seguro e diagnóstico.

**Definition of Done:** DOD-G + Testes de precedência, persistência e configuração inválida comprovam isolamento da configuração Git.

<a id="us-106"></a>
#### US-106 — Oferecer temas dark e light

- **Épico:** EPIC-21
- **Versão alvo:** v0.3
- **MoSCoW:** Must
- **Componente:** Desktop
- **Dependências:** [US-105](#us-105), [US-053](#us-053)
- **Status:** Planejada

**Como** usuário, **quero** alternar entre temas claros e escuros, **para** adaptar leitura ao meu ambiente.

**Critérios de aceite:**

1. Dark é tema inicial; dark e light estão disponíveis no fechamento da v0.3.
2. Escolha persiste entre sessões.
3. Graph, diff, foco e estados mantêm contraste e informação além da cor.

**Definition of Done:** DOD-G + Revisão visual e acessibilidade aprovadas em ambos os temas; sistema de temas arbitrários fica fora do escopo.

<a id="us-107"></a>
#### US-107 — Configurar atalhos do Desktop

- **Épico:** EPIC-21
- **Versão alvo:** v0.3
- **MoSCoW:** Must
- **Componente:** Desktop
- **Dependências:** [US-105](#us-105), [US-055](#us-055)
- **Status:** Planejada

**Como** usuário, **quero** ajustar atalhos de ações, **para** adaptar o fluxo ao meu teclado.

**Critérios de aceite:**

1. Ações configuráveis têm bindings visíveis e restauração de padrão.
2. Conflitos de atalhos são detectados.
3. Command palette/ajuda refletem bindings vigentes.

**Definition of Done:** DOD-G + Teste de remapeamento, conflito e restauração aprovado sem perder acesso por teclado.

<a id="us-108"></a>
#### US-108 — Configurar apresentação de blame no editor

- **Épico:** EPIC-21
- **Versão alvo:** v0.4
- **MoSCoW:** Must
- **Componente:** VSCode
- **Dependências:** [US-069](#us-069)
- **Status:** Planejada

**Como** usuário do VS Code, **quero** configurar formato, data, delay e alcance de blame, **para** equilibrar contexto e legibilidade.

**Critérios de aceite:**

1. Inline blame pode ser habilitado/desabilitado.
2. Formato e data relativa/absoluta, delay e linha atual/múltiplas linhas são configuráveis.
3. Valor inválido usa padrão documentado sem iniciar consultas excessivas.

**Definition of Done:** DOD-G + Extension tests de mudança em runtime e validação de configurações aprovados.

<a id="us-109"></a>
#### US-109 — Consolidar preferências entre interfaces

- **Épico:** EPIC-21
- **Versão alvo:** v1.0
- **MoSCoW:** Must
- **Componente:** Core / TUI / Desktop / VSCode
- **Dependências:** [US-105](#us-105), [US-107](#us-107), [US-108](#us-108), [US-042](#us-042)
- **Status:** Planejada

**Como** usuário, **quero** coerência de preferências compartilháveis, **para** alternar interfaces com menos surpresa.

**Critérios de aceite:**

1. Mapa distingue preferências compartilhadas de específicas; formatos de data/diff/graph são alinhados onde aplicável.
2. TUI oferece remapeamento documentado sem exigir perfis Vim/Emacs.
3. Política de confirmação não permite desabilitar proteções obrigatórias; arquitetura permite futura tradução.

**Definition of Done:** DOD-G + Matriz de escopos e migração revisada; testes de defaults e atalhos TUI aprovados.

<a id="epic-22"></a>
### EPIC-22 — Security & Safety

Proteger execução, dados e privacidade em todas as versões.

**Rastreabilidade:** PRD §§7.3, 8.5, 11.3, 12.2, 13.2–13.3, 13.6; SAD §§19–20, 26–28, 30, 33; ADR-009–010. As histórias deste épico detalham essas fontes; critérios adicionais são refinamentos propostos nesta baseline.

<a id="us-110"></a>
#### US-110 — Tratar paths, textos e argumentos como não confiáveis

- **Épico:** EPIC-22
- **Versão alvo:** v0.1
- **MoSCoW:** Must
- **Componente:** Core / Git Provider / CLI
- **Dependências:** [US-004](#us-004), [US-002](#us-002)
- **Status:** Planejada

**Como** usuário, **quero** processar repositórios sem executar conteúdo como comando, **para** inspecionar projetos com segurança.

**Critérios de aceite:**

1. Argumentos/pathspecs não são interpolados em shell nem confundidos com opções.
2. Mensagens e nomes têm controles/escapes de terminal sanitizados para apresentação.
3. Regras de escape/validação são reutilizáveis por futuros renderers GUI e conteúdo de forge.

**Definition of Done:** DOD-G + Fixtures maliciosas de nomes/mensagens verificam ausência de execução/injeção e preservação do dado original interno.

<a id="us-111"></a>
#### US-111 — Descrever risco e revalidar mutações

- **Épico:** EPIC-22
- **Versão alvo:** v0.2
- **MoSCoW:** Must
- **Componente:** Core
- **Dependências:** [US-003](#us-003), [US-007](#us-007)
- **Status:** Planejada

**Como** desenvolvedor, **quero** conhecer alvo, intenção e risco antes de alterar Git, **para** evitar perdas por ações ambíguas.

**Critérios de aceite:**

1. Core fornece metadados Safe/Moderate/Destructive coerentes com SAD.
2. Operações destrutivas exigem confirmação reforçada nas interfaces.
3. Pré-condições/HEAD são revalidados antes de executar; confirmação antiga não autoriza estado novo.

**Definition of Done:** DOD-G + Testes de intenção tipada e corrida entre prévia/execução comprovam bloqueio de ação obsoleta.

<a id="us-112"></a>
#### US-112 — Delegar credenciais Git aos mecanismos existentes

- **Épico:** EPIC-22
- **Versão alvo:** v0.2
- **MoSCoW:** Must
- **Componente:** Git Provider
- **Dependências:** [US-004](#us-004), [US-110](#us-110)
- **Status:** Planejada

**Como** desenvolvedor, **quero** usar credential helpers e SSH já configurados, **para** autenticar sem novo cofre inseguro.

**Critérios de aceite:**

1. Git usa mecanismos Git/SSH/OS compatíveis com ambiente.
2. GitSail não grava senha/token em texto puro.
3. Falha ou interação de autenticação necessária retorna diagnóstico útil sem hang indefinido.

**Definition of Done:** DOD-G + Testes com helpers simulados validam sucesso/falha e ausência de credenciais em saídas/logs.

<a id="us-113"></a>
#### US-113 — Produzir logs locais e diagnóstico redigido

- **Épico:** EPIC-22
- **Versão alvo:** v0.1
- **MoSCoW:** Must
- **Componente:** Core / CLI
- **Dependências:** [US-004](#us-004), [US-002](#us-002)
- **Status:** Planejada

**Como** usuário, **quero** obter diagnóstico local seguro, **para** reportar problemas sem expor meu código.

**Critérios de aceite:**

1. Logs incluem nível, componente, duração, operação e código de erro.
2. Tokens, senhas, URLs com credenciais e conteúdo completo de arquivos não são registrados por padrão.
3. Modo debug e exportação explícita mantêm redação de segredos.

**Definition of Done:** DOD-G + Teste insere segredos sentinela e comprova ausência em logs e pacote de diagnóstico.

<a id="us-114"></a>
#### US-114 — Consolidar privacidade e crash handling

- **Épico:** EPIC-22
- **Versão alvo:** v1.0
- **MoSCoW:** Must
- **Componente:** Core / TUI / Desktop / VSCode
- **Dependências:** [US-113](#us-113), [US-039](#us-039)
- **Status:** Planejada

**Como** usuário, **quero** usar GitSail localmente sem conta central, **para** manter controle dos meus dados.

**Critérios de aceite:**

1. Todos os fluxos Git locais funcionam sem conta GitSail e sem telemetria obrigatória.
2. Telemetria permanece desabilitada por padrão; qualquer implementação exige opt-in documentado.
3. Crash report só é enviado com consentimento explícito; logs locais permitem diagnóstico sem upload.

**Definition of Done:** DOD-G + Teste de configuração limpa verifica ausência de envio espontâneo e documentação descreve dados e consentimento.

<a id="epic-23"></a>
### EPIC-23 — Performance & Large Repositories

Preservar responsividade e consistência com dados extensos.

**Rastreabilidade:** PRD §§13.1, 17–18; SAD §§21–26, 31–32. As histórias deste épico detalham essas fontes; critérios adicionais são refinamentos propostos nesta baseline.

<a id="us-115"></a>
#### US-115 — Limitar trabalho e memória das consultas

- **Épico:** EPIC-23
- **Versão alvo:** v0.1
- **MoSCoW:** Must
- **Componente:** Core / Git Provider
- **Dependências:** [US-015](#us-015), [US-027](#us-027), [US-031](#us-031), [US-004](#us-004)
- **Status:** Planejada

**Como** desenvolvedor, **quero** consultas incrementais e canceláveis, **para** usar repositórios grandes sem carregar tudo.

**Critérios de aceite:**

1. Primeira página de log não materializa histórico completo na aplicação.
2. Diff/blame têm limites e cancelamento explícitos.
3. Resultados parciais/limitados são distintos de conclusão completa e nunca tratados como ausência de mudanças.

**Definition of Done:** DOD-G + Benchmark inicial registra volume/memória e teste comprova cancelamento; nenhum SLA numérico é inventado.

<a id="us-116"></a>
#### US-116 — Serializar mutações e invalidar leituras

- **Épico:** EPIC-23
- **Versão alvo:** v0.2
- **MoSCoW:** Must
- **Componente:** Core
- **Dependências:** [US-009](#us-009)
- **Status:** Planejada

**Como** desenvolvedor, **quero** coordenação de operações no mesmo repositório, **para** evitar conflitos causados pelo próprio aplicativo.

**Critérios de aceite:**

1. Mutações da mesma sessão/repositório são serializadas e lock Git externo é respeitado.
2. Leituras podem coexistir sem sobrescrever snapshots novos.
3. Mutação invalida caches relevantes; worktrees compartilhando metadata têm regra documentada.

**Definition of Done:** DOD-G + Teste concorrente demonstra ordem, erro de lock externo e descarte de leitura obsoleta.

<a id="us-117"></a>
#### US-117 — Aplicar cache com invalidação verificável

- **Épico:** EPIC-23
- **Versão alvo:** v0.3
- **MoSCoW:** Should
- **Componente:** Core
- **Dependências:** [US-115](#us-115), [US-116](#us-116), [US-065](#us-065)
- **Status:** Planejada

**Como** usuário, **quero** reutilizar consultas custosas sem dados incorretos, **para** navegar com fluidez.

**Critérios de aceite:**

1. Cache de páginas/layout usa chaves de contexto e geração.
2. Refresh, mudança de refs e mutações invalidam entradas relevantes.
3. Elegibilidade destrutiva não é confiada ao cache e há limites/evicção.

**Definition of Done:** DOD-G + Teste de cache hit seguido de mudança externa confirma atualização e limites de memória.

<a id="us-118"></a>
#### US-118 — Medir log, graph, diff e blame em diferentes escalas

- **Épico:** EPIC-23
- **Versão alvo:** v1.0
- **MoSCoW:** Must
- **Componente:** Qualidade / Core / TUI / Desktop / VSCode
- **Dependências:** [US-115](#us-115), [US-067](#us-067), [US-034](#us-034)
- **Status:** Planejada

**Como** mantenedor, **quero** benchmarks reproduzíveis, **para** priorizar regressões com evidência.

**Critérios de aceite:**

1. Cenários pequeno/médio/grande registram características e ambiente.
2. Mede primeira página, layout/render, cancelamento e consumo de recursos relevantes.
3. Orçamentos são definidos a partir das medições e registrados antes de virar gate de regressão.

**Definition of Done:** DOD-G + Relatório reproduzível e critérios de regressão aprovados; números do SAD permanecem sem SLA pré-fixado.

<a id="epic-24"></a>
### EPIC-24 — Testing & Quality

Validar comportamento, arquitetura e consistência em sistemas suportados.

**Rastreabilidade:** PRD §§7.3–7.4, 12.5–12.6, 17; SAD §§31, 35, 37, 40. As histórias deste épico detalham essas fontes; critérios adicionais são refinamentos propostos nesta baseline.

<a id="us-119"></a>
#### US-119 — Manter fixtures Git temporárias representativas

- **Épico:** EPIC-24
- **Versão alvo:** v0.1
- **MoSCoW:** Must
- **Componente:** Qualidade
- **Dependências:** [US-001](#us-001)
- **Status:** Planejada

**Como** contribuidor, **quero** cenários Git reproduzíveis, **para** testar sem usar repositórios pessoais.

**Critérios de aceite:**

1. Suite gera empty, one commit, branches, merge commit, detached, dirty, rename e binário.
2. Inclui shallow/bare; conflito/rebase em andamento são fixtures para leitura e futuras operações.
3. Cada cenário declara estado esperado e limpeza isolada.

**Definition of Done:** DOD-G + Execuções repetidas produzem resultados equivalentes e não escrevem fora de temporários autorizados.

<a id="us-120"></a>
#### US-120 — Validar domínio e contrato do provider

- **Épico:** EPIC-24
- **Versão alvo:** v0.1
- **MoSCoW:** Must
- **Componente:** Qualidade / Core
- **Dependências:** [US-005](#us-005), [US-119](#us-119)
- **Status:** Planejada

**Como** mantenedor, **quero** testes de unidade, contrato e integração, **para** confiar nas consultas e futuras substituições de provider.

**Critérios de aceite:**

1. Domínio testa invariantes sem Git/processo real.
2. Contrato comum verifica semântica do provider contra Git real temporário.
3. Golden parsing seletivo cobre edge cases sem acoplamento a saída localizada.

**Definition of Done:** DOD-G + Suite aprovada para consultas v0.1; discrepâncias são resolvidas ou explicitamente limitadas pelo contrato.

<a id="us-121"></a>
#### US-121 — Executar CI multiplataforma e fitness arquitetural

- **Épico:** EPIC-24
- **Versão alvo:** v0.1
- **MoSCoW:** Must
- **Componente:** Qualidade / Infraestrutura
- **Dependências:** [US-120](#us-120), [US-037](#us-037)
- **Status:** Planejada

**Como** contribuidor, **quero** feedback automático de qualidade, **para** evitar regressões antes de integrar mudanças.

**Critérios de aceite:**

1. Linux/Windows/macOS executam format, lint, unit/integration e build v0.1.
2. Checks verificam dependências de domínio, parsing restrito e protocolo explícito.
3. Pipeline evolui com TUI, Desktop, extensão e auditoria de dependências conforme chegam.

**Definition of Done:** DOD-G + Execução verde nos três sistemas e política de checks obrigatórios documentadas.

<a id="us-122"></a>
#### US-122 — Cobrir fluxos críticos das interfaces

- **Épico:** EPIC-24
- **Versão alvo:** v0.4
- **MoSCoW:** Must
- **Componente:** Qualidade / TUI / Desktop / VSCode
- **Dependências:** [US-121](#us-121), [US-047](#us-047), [US-058](#us-058), [US-072](#us-072), [US-076](#us-076)
- **Status:** Planejada

**Como** mantenedor, **quero** regressões de interface verificáveis, **para** preservar os fluxos principais.

**Critérios de aceite:**

1. TUI testa update/state e snapshots seletivos desde v0.2.
2. Desktop testa E2E crítico desde v0.3; extensão testa lifecycle/blame/history na v0.4.
3. Falha, cancelamento, repo vazio e conteúdo malicioso fazem parte dos cenários.

**Definition of Done:** DOD-G + Suite consolidada v0.4 é reproduzível; cada release anterior já aplica sua parte pela DoD global.

<a id="us-123"></a>
#### US-123 — Validar plataforma integrada e regressões avançadas

- **Épico:** EPIC-24
- **Versão alvo:** v1.0
- **MoSCoW:** Must
- **Componente:** Qualidade / Todas
- **Dependências:** [US-122](#us-122), [US-082](#us-082), [US-088](#us-088), [US-095](#us-095), [US-039](#us-039), [US-100](#us-100)
- **Status:** Planejada

**Como** usuário, **quero** comportamento consistente entre interfaces, **para** confiar no mesmo motor Git.

**Critérios de aceite:**

1. Mesmo cenário produz estado Git equivalente em TUI/Desktop.
2. VS Code apresenta hashes, autoria e diffs coerentes com Core.
3. Matriz inclui operações avançadas, interrupções, instalação e ausência de conta/rede para fluxos locais.

**Definition of Done:** DOD-G + Relatório de regressão por plataforma registra evidências e nenhum defeito bloqueante aberto.

<a id="epic-25"></a>
### EPIC-25 — Distribution & Updates

Entregar instalação e atualização progressivas até a plataforma estável.

**Rastreabilidade:** PRD §§7.4, 12.2, 12.4–12.5, 22; SAD §§30, 35–36, 39. As histórias deste épico detalham essas fontes; critérios adicionais são refinamentos propostos nesta baseline.

<a id="us-124"></a>
#### US-124 — Distribuir CLI e evoluir para binário CLI/TUI

- **Épico:** EPIC-25
- **Versão alvo:** v0.1
- **MoSCoW:** Must
- **Componente:** Distribuição
- **Dependências:** [US-121](#us-121), [US-131](#us-131), [US-130](#us-130)
- **Status:** Planejada

**Como** usuário, **quero** instalar GitSail em Windows, Linux e macOS, **para** executar consultas sem compilar manualmente.

**Critérios de aceite:**

1. Release v0.1 fornece CLI e instruções de Git pré-requisito por sistema.
2. Artefatos têm versão, checksum e origem verificável.
3. Pipeline admite TUI a partir da v0.2, sem exigir TUI para fechar v0.1.

**Definition of Done:** DOD-G + Instalação limpa e seis consultas validadas nas plataformas; decisão de Git mínimo/MSRV registrada.

<a id="us-125"></a>
#### US-125 — Empacotar aplicação Desktop

- **Épico:** EPIC-25
- **Versão alvo:** v0.3
- **MoSCoW:** Must
- **Componente:** Distribuição / Desktop
- **Dependências:** [US-124](#us-124), [US-058](#us-058), [US-060](#us-060), [US-055](#us-055)
- **Status:** Planejada

**Como** usuário Desktop, **quero** instalar a GUI, **para** usar GitSail como aplicativo local.

**Critérios de aceite:**

1. Pacotes/instaladores atendem sistemas suportados com dependências documentadas.
2. Estratégia de assinatura/notarização é decidida sem alegar assinatura ausente.
3. Instalação, abertura e remoção preservam repositórios e têm comportamento de preferências documentado.

**Definition of Done:** DOD-G + Smoke de pacote em ambiente limpo e inventário de limitações aprovados.

<a id="us-126"></a>
#### US-126 — Publicar pacote de extensão compatível

- **Épico:** EPIC-25
- **Versão alvo:** v0.4
- **MoSCoW:** Must
- **Componente:** Distribuição / VSCode
- **Dependências:** [US-070](#us-070), [US-122](#us-122), [US-130](#us-130)
- **Status:** Planejada

**Como** usuário do VS Code, **quero** instalar GitSail para blame/history, **para** usar contexto Git no editor.

**Critérios de aceite:**

1. VSIX e instruções de dependência do binário são verificáveis.
2. Preparação/entrega para Marketplace e Open VSX preserva a mesma compatibilidade declarada.
3. Permissões, licença, identidade e configuração constam dos metadados.

**Definition of Done:** DOD-G + Instalação a partir do pacote valida blame/history; disponibilidade dos canais é registrada como evidência.

<a id="us-127"></a>
#### US-127 — Atualizar Desktop com integridade e compatibilidade

- **Épico:** EPIC-25
- **Versão alvo:** v1.0
- **MoSCoW:** Must
- **Componente:** Distribuição / Desktop
- **Dependências:** [US-125](#us-125), [US-039](#us-039)
- **Status:** Planejada

**Como** usuário, **quero** atualizar o aplicativo com controle, **para** receber correções sem quebrar minhas integrações.

**Critérios de aceite:**

1. Mecanismo apresenta versão/origem e verifica integridade/autenticidade segundo estratégia registrada.
2. Falha ou interrupção não deixa instalação inutilizável e oferece recuperação documentada.
3. Compatibilidade do protocolo e migração de preferências são verificadas antes da troca.

**Definition of Done:** DOD-G + Teste de atualização válida, pacote inválido e interrupção comprova recuperação; CLI/extensão têm instruções próprias.

<a id="us-128"></a>
#### US-128 — Publicar matriz e checklist da v1.0

- **Épico:** EPIC-25
- **Versão alvo:** v1.0
- **MoSCoW:** Must
- **Componente:** Distribuição / Todas
- **Dependências:** [US-127](#us-127), [US-126](#us-126), [US-123](#us-123), [US-118](#us-118), [US-114](#us-114), [US-103](#us-103), [US-109](#us-109)
- **Status:** Planejada

**Como** mantenedor, **quero** um release integrado verificável, **para** declarar estabilidade com evidência.

**Critérios de aceite:**

1. Matriz relaciona versões de Core/CLI/TUI/Desktop/VSCode e schema.
2. Artefatos, checksums, notas, compatibilidade e troubleshooting acompanham a entrega.
3. Must de cada milestone estão concluídos e exceções de Should/Could são registradas.

**Definition of Done:** DOD-G + Checklist de release com links de evidência revisado; pendências não são convertidas silenciosamente em Done.

<a id="epic-26"></a>
### EPIC-26 — Documentation & Open Source

Preservar decisões, identidade e caminhos de contribuição.

**Rastreabilidade:** PRD §§1–3, 5, 13.5, 14, 18, 21–24; SAD §§6, 34, 37–40; ADR-001–012; decisões de branding da conversa. As histórias deste épico detalham essas fontes; critérios adicionais são refinamentos propostos nesta baseline.

<a id="us-129"></a>
#### US-129 — Preservar logo, mockups e identidade

- **Épico:** EPIC-26
- **Versão alvo:** v0.1
- **MoSCoW:** Must
- **Componente:** Documentação / Design
- **Dependências:** Nenhuma história prévia; fontes documentais disponíveis.
- **Status:** Planejada

**Como** contribuidor, **quero** referências visuais originais do GitSail, **para** evoluir uma identidade consistente.

**Critérios de aceite:**

1. Logo e mockups Desktop/TUI são mantidos como referências com nomes e origem registrados.
2. Nome GitSail, tagline Navigate your Git history., mascote náutico e conceito vela + Git graph são preservados.
3. Disponibilidade definitiva de marca e permissões dos assets não são declaradas verificadas sem evidência.

**Definition of Done:** DOD-G + Arquivos originais preservados; inventário diferencia referências visuais de requisitos funcionais do PRD.

<a id="us-130"></a>
#### US-130 — Resolver licença e política inicial de versões

- **Épico:** EPIC-26
- **Versão alvo:** v0.1
- **MoSCoW:** Must
- **Componente:** Documentação / Governança
- **Dependências:** Nenhuma história prévia; fontes documentais disponíveis.
- **Status:** Planejada

**Como** mantenedor, **quero** registrar licença e decisões de compatibilidade inicial, **para** publicar o projeto com regras claras.

**Critérios de aceite:**

1. Escolha MIT vs Apache-2.0 fica registrada antes do primeiro release público.
2. Git mínimo, Rust MSRV, nome/organização do repositório e versão pré-1.0 recebem decisões rastreáveis.
3. Backlog não presume respostas; decisões aprovadas atualizam documentação e critérios afetados.

**Definition of Done:** DOD-G + Registro decisório e arquivos de licença/compatibilidade revisados antes da distribuição.

<a id="us-131"></a>
#### US-131 — Documentar instalação, consultas e arquitetura

- **Épico:** EPIC-26
- **Versão alvo:** v0.1
- **MoSCoW:** Must
- **Componente:** Documentação
- **Dependências:** [US-037](#us-037), [US-120](#us-120)
- **Status:** Planejada

**Como** usuário e contribuidor, **quero** um guia inicial e decisões arquiteturais acessíveis, **para** usar e entender GitSail.

**Critérios de aceite:**

1. README cobre propósito, requisitos, instalação e seis consultas humanas/JSON.
2. PRD/SAD preservados e 12 ADRs ficam acessíveis com status; eventual separação mantém texto original.
3. Documentação técnica primária é em inglês e exemplos correspondem ao comportamento suportado.

**Definition of Done:** DOD-G + Exemplos são executados em fixture e links/ADRs são conferidos; este backlog de planejamento permanece em português.

<a id="us-132"></a>
#### US-132 — Documentar contribuição e manutenção

- **Épico:** EPIC-26
- **Versão alvo:** v0.1
- **MoSCoW:** Must
- **Componente:** Documentação
- **Dependências:** [US-001](#us-001), [US-119](#us-119), [US-130](#us-130)
- **Status:** Planejada

**Como** contribuidor, **quero** um caminho claro para colaborar, **para** enviar mudanças alinhadas ao projeto.

**Critérios de aceite:**

1. CONTRIBUTING explica setup, testes, boundaries e revisão.
2. Templates de issue/PR relacionam US/EPIC e reprodução sem segredos.
3. Processo para atualizar PRD/SAD/ADRs preserva IDs e histórico de decisões.

**Definition of Done:** DOD-G + Contribuidor consegue seguir setup e encontrar decisões/checks; documentos técnicos em inglês revisados.

<a id="us-133"></a>
#### US-133 — Consolidar manuais, troubleshooting e limites

- **Épico:** EPIC-26
- **Versão alvo:** v1.0
- **MoSCoW:** Must
- **Componente:** Documentação
- **Dependências:** [US-131](#us-131), [US-132](#us-132), [US-123](#us-123), [US-100](#us-100), [US-109](#us-109)
- **Status:** Planejada

**Como** usuário, **quero** documentação das três experiências e suas limitações, **para** usar e recuperar GitSail com autonomia.

**Critérios de aceite:**

1. Manuais cobrem TUI/Desktop/VSCode, operações avançadas, conflitos e recuperação.
2. Guia explica credenciais externas, configuração, updates, privacidade e compatibilidade.
3. Pós-v1.0 e decisões abertas permanecem separados de compromissos entregues.

**Definition of Done:** DOD-G + Links e exemplos validados contra release candidata; cobertura de requisitos e itens adiados revisada.

## 7. Cobertura das decisões arquiteturais

| ADR aceito | Histórias representativas |
|---|---|
| ADR-001 — Rust no Core | [US-001](#us-001), [US-002](#us-002) |
| ADR-002 — Ports & Adapters | [US-003](#us-003), [US-005](#us-005), [US-121](#us-121) |
| ADR-003 — Git CLI Provider | [US-004](#us-004), [US-005](#us-005) |
| ADR-004 — Monorepo | [US-001](#us-001), [US-121](#us-121) |
| ADR-005 — Ratatui | [US-040](#us-040), [US-041](#us-041) |
| ADR-006 — Tauri + Vue 3 | [US-051](#us-051), [US-054](#us-054) |
| ADR-007 — TypeScript no VS Code | [US-068](#us-068), [US-069](#us-069) |
| ADR-008 — Protocolo versionado | [US-035](#us-035), [US-037](#us-037), [US-039](#us-039) |
| ADR-009 — Leitura/escrita separadas | [US-003](#us-003), [US-111](#us-111) |
| ADR-010 — Credenciais delegadas | [US-112](#us-112), [US-102](#us-102) |
| ADR-011 — Layout separado de render | [US-064](#us-064), [US-066](#us-066), [US-067](#us-067) |
| ADR-012 — CLI JSON antes de daemon | [US-037](#us-037), [US-069](#us-069) |

## 8. Registro de decisões abertas e limites

Os itens abaixo permanecem abertos nas fontes. A coluna de encaminhamento indica onde resolver, não uma decisão já tomada. Questões que afetam uma história bloqueiam sua implementação/aceitação no ponto indicado; não exigem inventar respostas para aprovar a estrutura do backlog.

| Decisão | Encaminhamento e momento |
|---|---|
| MIT vs Apache-2.0; nome/organização do repositório; versão pré-1.0 | **Resolvido** em [US-130](#us-130)/[ADR-021](../architecture/GitSail_SAD_and_ADRs_v0.1.md#adr-021--license-minimum-git-version-rust-msrv-repository-identity-and-pre-10-versioning): Apache-2.0 (arquivo `LICENSE` na raiz); repositório permanece `rpaggi/gitsail` (GitHub) até decisão futura de organização dedicada; versões de crate ficam em `0.0.0` até o primeiro release público, com milestones (`v0.1`…`v1.0`) rastreados por tag/release notes. |
| Git mínimo e Rust MSRV | **Resolvido** em [US-130](#us-130)/[ADR-021](../architecture/GitSail_SAD_and_ADRs_v0.1.md#adr-021--license-minimum-git-version-rust-msrv-repository-identity-and-pre-10-versioning): Git mínimo 2.31 (derivado do uso real de `--path-format=absolute` em `gitsail-git`); Rust MSRV 1.97.0, fixado como piso igual ao toolchain verificado (não testado contra versão mais antiga) via `rust-version` no `Cargo.toml` do workspace. |
| Runtime/estratégia async e parser CLI | Refinar em [US-004](#us-004) e [US-036](#us-036); não expor runtime no Domain. |
| Envelope definitivo, cursor e correlação | [US-035](#us-035) e [US-015](#us-015); definir na v0.1 e manter testes de compatibilidade. |
| State management Desktop e persistência de preferências | [US-051](#us-051) e [US-105](#us-105), antes de estabilizar v0.3. |
| Filesystem watcher | Opcional em [US-009](#us-009)/[US-117](#us-117); refresh manual, por foco e após mutação já obrigatório. |
| Distribuição do binário da extensão | [US-070](#us-070), antes do pacote v0.4; não presumir bundling/download. |
| Assinatura/notarização e atualização | [US-125](#us-125) / [US-127](#us-127); documentar mecanismo e limitações reais. |
| Escopo exato e hosts suportados de GitHub/GitLab | [US-101](#us-101)–[US-103](#us-103); delimitar consulta inicial. Criar PR/MR é Could. |
| Submodules v1.0 ou pós-v1.0 | Decisão explícita de escopo antes do gate v1.0. Não existe história prometendo suporte completo; ampliar requer revisão do PRD e novos IDs. |
| Marca/pacotes/domínios e uso dos assets | [US-129](#us-129) mantém identidade escolhida sem alegar disponibilidade verificada — ver `docs/product/brand-identity.md` e `assets/README.md`. Avaliação de marca/direitos permanece **não realizada**; registrar antes de publicação pública da marca. |
| IPC/daemon e política de plugins | Permanecem evolução futura; nenhuma dependência obrigatória antes da v1.0. |

### Won’t até v1.0 / ideias não comprometidas

Conforme PRD §§3 e 23 e SAD §34: implementar Git/protocolo de transporte do zero; hospedagem de repositórios; cliente completo de CI/CD ou projetos/issues; editor de código completo; conta/sincronização proprietária obrigatória; IA como requisito; plugin API; Forgejo/Gitea; integrações de forge ampliadas; editor visual avançado de rebase; UX dedicada de assinatura de commits; gestor avançado de worktrees; bisect visual; analytics; JetBrains/Neovim; daemon/API local; temas arbitrários e perfis Vim/Emacs. Um provider alternativo só deve ser avaliado com benefício mensurável e mudança rastreada. Sem histórias executáveis ou IDs reservados para essas ideias nesta baseline.

## 9. Sequência sugerida após a revisão documental

1. Resolver decisões v0.1 e preservar identidade/fontes: [US-130](#us-130), [US-129](#us-129).
2. Workspace → domínio/erros → ports → runner → provider: [US-001](#us-001), [US-002](#us-002), [US-003](#us-003), [US-004](#us-004), [US-005](#us-005).
3. Descoberta → status → log paginado → branches → diff → blame, com [US-119](#us-119), [US-110](#us-110) e [US-113](#us-113) desde o início.
4. DTOs e CLI humana/JSON, contratos e CI: [US-035](#us-035), [US-036](#us-036), [US-037](#us-037), [US-120](#us-120), [US-121](#us-121).
5. Revisar SAD §40, documentação e binário v0.1; só então avançar aos gates TUI/Desktop/VSCode. O primeiro milestone funcional é consultar log tipado no Core/CLI.

## 10. Manutenção e evidências do backlog

Ao converter histórias em issues, usar o ID no título, relacionar o épico, conservar os campos e links de dependência e anexar evidências da DoD. Divisão futura preserva a história original como agregadora/descontinuada e atribui novos IDs às partes. Mudanças de versão/prioridade recebem justificativa e fonte afetada. Aceitação de história ocorre por comportamento demonstrado, não por quantidade de commits ou cobertura isolada.

A validação documental desta baseline verifica contagem, unicidade de IDs, campos obrigatórios, dependências existentes, ausência de ciclos, compatibilidade temporal entre dependências e links locais. Ela não substitui testes futuros do produto nem afirma que os gates de release foram atingidos.
