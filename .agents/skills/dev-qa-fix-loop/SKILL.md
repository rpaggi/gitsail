---
name: dev-qa-fix-loop
description: Executa o ciclo completo de desenvolvimento de uma tarefa até fechar sem defeito — pega a próxima tarefa pendente, delega a implementação aos especialistas certos, aciona QA de ponta a ponta depois da conclusão e, se houver bug, corrige imediatamente e repete o QA até o ciclo encerrar verde. Invocado via /dev-qa-fix-loop [id-ou-descrição-opcional].
---

# Ciclo dev → QA → fix até verde

Orquestra a equipe (ou os subagentes disponíveis) para levar uma unidade de trabalho de "pendente" até "concluída" **com QA aprovado**, num loop fechado: constrói → testa de ponta a ponta → corrige bugs → retesta, sem parar enquanto houver defeito.

O argumento (se houver) identifica a tarefa. Se vier vazio, pegue a única/próxima pendente; se houver mais de uma candidata, liste-as e pergunte qual.

## Regra zero

Antes de qualquer análise ou código, consulte a base de conhecimento do projeto (se houver) com a query mais específica sobre a tarefa. Repita com termos alternativos se o retorno for pobre — ela é a fonte de verdade.

## Fluxo

### 1. Selecionar e analisar
1. Liste as tarefas pendentes e identifique a certa (ou use o argumento).
2. Leia o contexto completo da tarefa — descrição, critérios de aceite, referências.
3. Busque decisões relacionadas na base de conhecimento.
4. **Reconcilie o enunciado com a realidade do código e do git.** A tarefa pode descrever um estado desatualizado — confira branch, working tree, arquivos citados. Divergência entre o que a tarefa assume e o que o repositório mostra é achado a reportar, não detalhe a ignorar.
5. Marque a tarefa como em andamento.

### 2. Planejar
Decomponha em entregáveis, mapeie qual especialista cada um exige, decida a ordem de dependências. Se o escopo real divergir materialmente do que foi pedido (ex.: mudança maior do que prevista, ação difícil de reverter), **pare e confirme com o usuário** antes de executar essa parte.

### 3. Executar
Delegue cada entregável ao especialista certo, respeitando dependências (schema compartilhado → serviços → rotas → interface). Entregáveis independentes em paralelo. Rode verificação estática/testes das áreas afetadas — sem regressão. Use sempre o ambiente de desenvolvimento documentado do projeto (nunca um processo solto fora dele).

### 4. QA de ponta a ponta — sempre após a conclusão
Delegue a quem faz QA o teste manual real pela interface (ou pela API, se não houver interface), cobrindo os critérios de aceite da tarefa, os fluxos afetados por papel/permissão se houver, e isolamento entre tenants/escopos se aplicável. Quem faz QA evidencia (screenshots/logs) e reporta bugs — não corrige.

### 5. Loop de correção até verde
- **Se o QA encontrar qualquer bug:** corrija imediatamente, delegando ao especialista do domínio do defeito. Não adie, não abra "tarefa para depois".
- Após cada correção, **volte ao passo 4**: revalide o cenário que falhou e faça uma regressão rápida do fluxo tocado.
- Repita até fechar **sem nenhum bug aberto**.

### 6. Fechamento
- Revisão final: convenções do projeto, validação de entrada externa, ausência de segredo em log, sem código de depuração esquecido.
- Marque a tarefa como concluída, preencha a conclusão com o que foi entregue e como foi verificado.
- Se a feature introduziu regra de negócio nova, registre como decisão/documentação (fluxo de revisão do projeto, se houver).
- Resuma ao usuário: o que foi entregue, arquivos tocados, resultado do QA, próximos passos. Commit/push só quando o usuário pedir.

## Princípios

- **Não encerre com bug aberto.** O critério de "pronto" é QA verde, não código escrito.
- **Corrija na hora.** Bug achado no QA volta imediatamente ao especialista; não vira backlog.
- **Reconcilie antes de agir.** O código e o histórico de versionamento mandam mais que o texto da tarefa.
- **Confirme escopo que extrapola o pedido original.**
- **Rastreie no board/tracker.** Status sempre refletindo o estado real.
- **Ambiente de desenvolvimento documentado, sempre** — nunca processo solto para "só conferir".

