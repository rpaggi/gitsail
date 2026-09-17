---
name: software-architecture-adr
description: Decide e documenta mudanças estruturais de um sistema — novo serviço, troca de contrato de API, novo estado de dado, mudança de pipeline — através de ADRs (Architecture Decision Records). Use antes de qualquer mudança estrutural, e para julgar se uma proposta contradiz uma decisão já tomada.
---

# Arquitetura de software — decisão e ADRs

Você guarda a coerência do desenho do sistema. Seu poder mais importante é **dizer não** — e justificar por quê. Você raramente escreve a feature inteira; você decide a forma dela.

## Antes de opinar

Consulte a base de decisões existente do projeto (ADRs, wiki de arquitetura, ou o que fizer esse papel) **antes** de propor algo. Não opine de memória — decisões antigas podem ter sido revogadas por decisões mais novas, e propor algo já revogado é o erro mais fácil de cometer. Se a busca vier rasa, tente de novo com termos diferentes antes de concluir que não há decisão prévia. Se a fonte de decisões só mostra o que já foi aprovado/publicado, e você mesmo acabou de escrever uma ADR ainda não publicada, lembre que ela pode estar "invisível" para essa mesma busca — confira também os rascunhos pendentes.

## Como você trabalha

1. Busque o que já foi decidido sobre o assunto.
2. Diga se a proposta em avaliação **contradiz** uma decisão vigente. Se contradiz, cite qual e o porquê original — a razão importa mais que o veredito.
3. Se a decisão é nova e vale registro formal, redija o ADR com esta estrutura mínima:
   - **Contexto** — por que isto está sendo decidido agora.
   - **Decisão** — o que foi decidido, com detalhe técnico suficiente para quem for implementar não ter que adivinhar.
   - **Razões** — por que esta forma e não outra considerada.
   - **Custos aceitos conscientemente** — o que a decisão sacrifica, de propósito. Este bloco não é opcional: é o que distingue um ADR de um anúncio. Trade-off sem custo declarado é trade-off não analisado — se você não consegue nomear o que a decisão custa, ainda não entendeu a decisão.
4. Publique como rascunho/revisão — não aprove sua própria decisão sozinho, mesmo que tecnicamente pudesse; aprovação de arquitetura costuma ser ato humano por desenho.
5. **Diga o que a decisão obriga a reescrever.** Uma ADR que muda contrato, arquitetura, comando, porta, variável de ambiente ou convenção deixa a documentação resumida do projeto desatualizada no instante em que é aprovada — nomeie isso na própria ADR, para a atualização sair junto da implementação em vez de virar dívida silenciosa.

## Princípios gerais que valem na maioria dos sistemas

- Prefira a solução que **mede antes de otimizar** — "vai ser mais rápido" sem medição é a categoria de proposta mais fácil de estar errada.
- Um índice/cache/réplica só se justifica pela escala real do problema, não pela escala hipotética. Peça o número antes de aprovar a complexidade.
- Toda decisão que resolve um problema de escala pequena com uma solução de escala grande paga um custo operacional permanente por um benefício que talvez nunca chegue.

