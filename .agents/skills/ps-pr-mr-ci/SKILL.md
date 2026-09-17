---
name: ps-pr-mr-ci
description: Push da branch atual, abre PR pro branch de produção, dá merge e acompanha a pipeline de CI/deploy até terminar. Use quando o usuário pedir para subir/publicar/dar deploy do trabalho ("sobe isso", "abre o PR e mergeia", "manda pra produção").
---

# /ps-pr-mr-ci — push → PR → merge → acompanhar CI

Fluxo de ponta a ponta pra levar o trabalho commitado até produção (ou até o branch de
destino que fizer sentido no repo), sem o usuário ter que pedir cada passo separado.
**Assume que o trabalho já está commitado** — esta skill não commita nada; se houver
mudanças não commitadas, pare e avise, não decida sozinho o que entra no commit.

## Antes de tudo: descubra as regras do repo, não assuma

Cada repo pode ter convenção própria de branch (`dev`→`main`, `develop`→`master`, trunk-based
direto na `main`, etc.) e de teste. **Leia o `CLAUDE.md`/`AGENTS.md`/`README.md` do repo se
existir** antes de agir — eles são a fonte de verdade de qual branch é "produção", qual comando
roda a suíte de teste, e se merge no branch de produção dispara deploy automático (se disparar,
trate o merge como uma ação de produção de verdade, não como "só mergear um PR").

Se não houver documentação, infira do estado real do repo (`git branch -a`, `git log`,
histórico de PRs recentes com `gh pr list --state merged --limit 5`) em vez de assumir
`main`/`master` por padrão.

## Passo a passo

1. **Confirme o estado do repo.** `git status` — se houver mudanças não commitadas, **pare** e
   pergunte ao usuário o que fazer (não assuma que deve commitar por conta própria; isso é
   decisão de mensagem de commit, não desta skill). `git log <branch-atual>` vs
   `git log origin/<branch-atual>` pra saber quantos commits vão subir.

2. **Rode a suíte de teste do repo, se houver uma documentada**, antes de subir qualquer coisa
   pra um branch que dispara deploy. Não pule esse passo silenciosamente só porque "provavelmente
   está tudo bem" — se algum agente/sessão anterior já confirmou os testes verdes recentemente
   *e nada mudou desde então*, pode citar essa confirmação em vez de rodar de novo, mas diga
   isso explicitamente ao usuário em vez de simplesmente não rodar.

3. **Verifique a conta do `gh` antes de qualquer chamada de API/PR — e nunca troque a ativa.**
   `gh auth status` pode ter múltiplas contas logadas com uma "ativa" que não é a dona do repo —
   isso quebra silenciosamente com `GraphQL: Could not resolve to a Repository...` ou 404, uma
   mensagem de erro que não aponta pra causa real. Descubra o dono certo pelo `git remote -v` e por
   qualquer nota de memória/CLAUDE.md sobre qual conta é dona de qual repo, e confirme com
   `gh api repos/<owner>/<repo> --jq .full_name`.
   Se a conta certa não for a ativa, **não rode `gh auth switch`** — a conta ativa é estado global
   da máquina (`~/.config/gh/hosts.yml`), compartilhado por qualquer sessão que rodar `gh` ao mesmo
   tempo; uma sessão troca, outra sessão paralela desfaz no meio do trabalho (medido: um
   `gh run view` de acompanhamento de deploy passou a dar 404 porque outra sessão trocou a conta de
   volta). Em vez disso, passe a conta em **todo** comando `gh` desta skill:
   `GH_TOKEN=$(gh auth token --user <dono-do-repo>) gh <comando> ...` (funciona com `gh api`,
   `gh pr`, `gh run` — qualquer subcomando). Se o repo já tiver essa convenção documentada
   (CLAUDE.md/memória), siga o formato exato registrado lá.

4. **Push**: `git push origin <branch-atual>`.

5. **PR**: confira se já existe um aberto pro branch de destino
   (`gh pr list --state open --base <destino> --head <branch-atual>`) antes de criar outro.
   Se não existir, `gh pr create --base <destino> --head <branch-atual>`, com título curto e um
   corpo em formato **Summary + Test plan** (o mesmo padrão que a instrução geral de "Creating
   pull requests" já usa) — liste o que mudou de fato (não copie mensagens de commit cruas) e o
   que foi verificado. Termine o corpo com a atribuição padrão desta sessão, se houver uma
   configurada (linhas de `Co-Authored-By`/`Generated with Claude Code` do lembrete do sistema).

6. **Merge**: `gh pr merge <número> --merge` (ou `--squash`/`--rebase` se o repo tiver essa
   convenção — confira PRs recentes ou configuração de branch protection do GitHub antes de
   escolher; não troque o método sem motivo). Confirme o merge de verdade depois
   (`gh pr view <número> --json state,mergedAt`), não confie em "comando não deu erro" — merge
   sem confirmação explícita de estado é o tipo de coisa que falha em silêncio.

7. **Acompanhe a pipeline, se existir uma** (`.github/workflows/**`, ou o que o `CLAUDE.md`
   documentar como deploy). `gh run list --branch <destino> --limit 3` pra achar o run disparado
   pelo push do merge (o mais recente, `in_progress`), depois `gh run watch <run-id>
   --exit-status` até terminar. Reporte o resultado real (sucesso/falha), não só "a pipeline
   rodou" — se falhar, pare e mostre o log relevante em vez de seguir em frente.

## O que NÃO fazer

- Não force-push, não pule hooks, não use `--admin` pra passar por cima de proteção de branch
  sem o usuário ter pedido isso explicitamente.
- Não decida sozinho fazer squash de commits ou reescrever histórico pra "limpar" o PR — o
  histórico em camadas (por área/território, se o repo tiver esse costume) é informação, não bagunça.
- Não invente branch de destino — se houver ambiguidade real (mais de um branch de produção
  plausível, ou repo sem convenção clara), pergunte em vez de chutar.
- Se a suíte de teste ou a pipeline de CI falhar, não tente "consertar rápido" por conta própria
  fora do escopo desta skill — reporte e pare; correção de bug é outro tipo de tarefa.

