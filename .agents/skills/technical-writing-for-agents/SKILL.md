---
name: technical-writing-for-agents
description: Redige e revisa documentação que outros agentes de IA vão ler e seguir como instrução executável — precisão editorial, nenhuma afirmação sem fonte conferida, sem contagem escrita à mão que apodrece, sem segredo ou detalhe de infraestrutura interna em documento publicado.
---

# Documentação técnica para consumo por agentes

O texto que você escreve não é acabamento — se ele for servido a agentes de IA de outros contextos/repositórios como instrução, instrução errada é obedecida com a mesma fidelidade que a certa.

## Antes de escrever

Confirme cada afirmação de comportamento contra a fonte real (decisão registrada, código, teste) — documento que descreve comportamento sem conferir a decisão é opinião com formatação.

## Regras que não se negociam

- **Documento publicado para consumo externo (servido a outro repositório/agente, ou visível publicamente) nunca contém credencial, host interno, IP de rede interna ou nome de máquina.** O critério prático: "colaria isto num README público sem hesitar?"
- **Não escreva contagem à mão** — número de ferramentas, artigos, agentes, o que for. Contagens escritas em prosa apodrecem no primeiro item novo e são lidas como verdade por quem só tem esse documento. Se o número for inevitável, escreva de onde ele sai (aponte para a fonte que se verifica sozinha — idealmente algo que quebra um teste quando diverge).
- **Não cite o status transitório de algo** ("está em revisão", "ainda não aprovado") — esse tipo de frase envelhece sozinha no primeiro clique de quem aprova. Cite o identificador estável (número, slug); o status atual se consulta na fonte, não se copia para o texto.
- **Evidência sobre confiança.** Toda afirmação sobre comportamento aponta para a fonte de onde saiu (decisão registrada, arquivo, teste). Se não deu para conferir, marque como hipótese — não arredonde para fato.
- **Divergência entre a documentação resumida e a fonte de verdade não é achado para o backlog — o conserto é parte da tarefa que encontrou.** Documentação que ninguém relê é onde a decisão morre em silêncio.

## O que normalmente não é seu território

- O código ao redor da documentação (o script que a publica, os testes que a validam) costuma ser mantido por quem é dono do sistema consumidor — se o seu texto precisa de um mecanismo novo (marcador, campo), peça a quem é dono em vez de inventar um por conta própria.
- Texto de interface dentro de componentes de UI costuma ser código com teste, mantido por quem mantém o componente.
- Comentário de código já existente que registra uma decisão histórica não se reescreve para "atualizar" — é registro de quem decidiu o quê na época; reescrevê-lo falsifica o histórico.

## Como você entrega

Diga o que mudou, contra o que você conferiu cada afirmação, e o que ficou sem conferir. Leia o texto como ele chega ao consumidor final (texto puro pela API/MCP, não a página renderizada com CSS) — se o leitor mais frequente é um agente, não uma pessoa, otimize para isso.

