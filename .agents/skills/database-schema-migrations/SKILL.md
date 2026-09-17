---
name: database-schema-migrations
description: Modela e migra schema de banco de dados com disciplina de medição antes de otimizar, migrations sempre geradas por ferramenta (nunca edição manual do banco), e declaração explícita do impacto de cada mudança — o que quebra, o que exige reprocessamento, o que é irreversível.
---

# Modelagem e migração de banco de dados

Performance é **medida, não presumida.** A resposta certa para "isso precisa de índice/cache/partição?" quase sempre começa em "quantas linhas/quantas queries por segundo, de verdade?" — não em intuição.

## Antes de mexer no schema

Busque a base de conhecimento do projeto pelo modelo de dados existente e por decisões de modelagem já tomadas (ex.: por que uma coluna é nullable, por que um índice existe ou deliberadamente não existe).

## Regras gerais que quase sempre valem

- **Migration sempre gerada pela ferramenta do projeto** (drizzle-kit, Prisma Migrate, Alembic, o que for) a partir da definição de schema — nunca editar o banco de produção à mão, mesmo para "só um índice rápido". SQL que a ferramenta não gera sozinha (extensões, índices especiais) entra como migration manual, mas ainda registrada no histórico de migrations da ferramenta — um `.sql` solto que a ferramenta não sabe que existe é pior que não ter rodado: passa a impressão de aplicado sem estar.
- **Soft-delete com retenção clara** para dado que o usuário pode querer recuperar (registro de trabalho); **hard delete sem lixeira** para dado operacional efêmero (sessão, token, cache) — misturar os dois modelos no mesmo campo confunde quem lê o schema depois.
- **Enums no banco em inglês/técnico, tradução só na camada de apresentação** — evita que um rótulo de UI vaze para uma comparação de código.
- **Índice vetorial (se o projeto usa busca por similaridade) só se justifica pela contagem real de vetores** — busca por força bruta filtrada por uma chave seletiva (ex. projeto/tenant) resolve alguns milhares de vetores em poucos milissegundos; índices aproximados (HNSW/IVFFlat) só pagam o custo de manutenção a partir de dezenas/centenas de milhares.
- **Dado derivado (contagem, peso de aresta em grafo, soma) nunca se materializa quando pode ser calculado on-demand** — materializar cria uma segunda fonte de verdade que diverge da primeira mais cedo ou mais tarde. Só materialize com medição real de que o cálculo on-demand é o gargalo.

## Como você entrega

Toda mudança de schema vem com a migration gerada **e** o impacto declarado por escrito: o que precisa reprocessar, o que quebra, o que é irreversível. Quem for aplicar a migration em produção lê essa declaração antes de rodar — não adivinha pelo diff do SQL.

**Mudança que altera uma convenção que a documentação resumida do projeto descreve (predicado de visibilidade, regra de retenção, enum) sai atualizada no mesmo commit.**

