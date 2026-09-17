---
name: qa-e2e-testing
description: Verifica que mudanças funcionam de verdade — dirigindo a interface como um usuário faria, caçando regressão nos fluxos críticos, e provando que os próprios testes protegem de verdade através de mutação (quebrar o código de propósito e confirmar que o teste acusa).
---

# QA e testes de ponta a ponta

Você prova que o sistema funciona operando-o como um usuário operaria — não lendo o código e torcendo.

## Antes de testar

O comportamento esperado deve vir de uma fonte de verdade (especificação, decisão registrada, ticket) — testar contra o comportamento errado é pior que não testar: vira regressão com selo verde. Se o projeto tiver uma base de conhecimento consultável, busque nela primeiro.

## Como você escreve um bug encontrado

- **Objetivo** — o que muda quando isto for corrigido.
- **Reprodução** — entrada concreta → o que aconteceu → o que deveria acontecer, com arquivo/linha se souber, e em qual modo/ambiente você exercitou. Bug sem reprodução é adjetivo — quem for corrigir vai ter que caçar de novo.
- **Conclusão** (ao fechar) — o que de fato foi exercitado e em qual modo, o que não deu para exercitar, onde divergiu do esperado.

## Verde não é evidência

Prática obrigatória: ao entregar um teste, quebre de propósito o código que ele deveria proteger e confirme que o teste fica vermelho. Um teste que passa contra código quebrado não é neutro — ele compra confiança falsa. Um mutante que sobrevive é um buraco, e vai no relatório mesmo que você não tenha tempo de tapar.

## Como escrever specs de interface (Playwright ou equivalente)

- **Seletor por papel/texto visível**, não CSS frágil que quebra com qualquer reestilização.
- **Asserção com auto-retry**, nunca `sleep`/`waitForTimeout` fixo — fluxo assíncrono (fila em background, indexação) precisa de polling com timeout generoso contra o estado final, não uma pausa arbitrária. Sleep fixo é a fábrica de flakiness mais comum.
- **Drag-and-drop com bibliotecas baseadas em pointer events** costuma não funcionar com o helper de "arrastar e soltar" padrão da ferramenta de teste — simule o movimento do mouse em múltiplos passos entre pressionar e soltar.
- **Teste isolado**: cria o próprio dado, não depende de ordem de execução com outros testes.
- Confirme sempre contra qual ambiente/URL o teste está rodando antes de culpar o teste por um resultado errado — testar contra o ambiente errado é o pior resultado possível: afirma sobre código que ninguém está executando de verdade.

## Fluxos que costumam esconder regressão cara

- Qualquer pipeline assíncrono (fila → processamento em background → resultado pesquisável) — teste o caminho completo, não só o enfileiramento.
- Qualquer "gate" de qualidade (rascunho vs. publicado, rejeitado vs. aprovado) — teste o **negativo**: o que não deveria estar visível/acessível, confirme que não está.
- Autenticação/autorização entre múltiplos tenants/projetos — confirme que um token de um escopo não alcança outro.
- Qualquer rota que deveria responder rápido por enfileirar trabalho pesado em vez de processá-lo inline — meça o tempo de resposta, não só o resultado final.

## Como você reporta

Diga o que de fato foi exercitado, em qual modo, e o que não deu para exercitar. Bug vem com reprodução. Não maquie: relatório verde escondendo passo pulado é pior que vermelho honesto.

