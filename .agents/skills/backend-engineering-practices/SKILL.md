---
name: backend-engineering-practices
description: Implementa e mantém a camada de servidor de uma aplicação — rotas de API, autenticação, integrações externas, workers em background — com disciplina de verificação real (mutation testing, exercício de ponta a ponta), não apenas "compila" ou "os testes passam".
---

# Engenharia de backend — implementação e verificação

Seu território real (quais pastas, qual framework, quais convenções de rota/erro) vem da documentação local do repositório (README/CLAUDE.md/AGENTS.md) — leia-a antes de codar. Esta skill é sobre **disciplina de entrega**, não sobre uma stack específica.

## Antes de codar

Se o projeto tiver uma base de conhecimento consultável, busque nela antes de escrever a primeira linha — a resposta pode já existir como decisão registrada, e reimplementar por cima dela é retrabalho ou regressão. Documentação resumida (README/CLAUDE.md) pode estar desatualizada; onde divergir da fonte de decisões, esta última vence, e a divergência se reporta.

## Regras gerais que quase sempre valem

- **Nunca faça trabalho pesado (embedding, processamento longo) dentro do ciclo de uma requisição HTTP síncrona.** Enfileire e responda; um worker separado processa. Proxies e load balancers têm timeout curto (segundos, não minutos) — isso não é otimização, é correção.
- **Envelope de erro consistente** em toda a API (mesmo formato de código+mensagem), timestamps num formato padrão (ISO 8601 UTC é uma escolha segura).
- **Valide entrada de query string/corpo com uma biblioteca de schema antes de usar o valor.** Deixar o erro de validação vazar cru vira 500 genérico em vez do envelope de erro esperado pelo cliente.
- **Segredos nunca retornam pela API**, nem parcialmente. Se o cliente precisa saber "está configurado", devolva um booleano, nunca o valor.
- **Toda chamada a serviço externo com cota (IA, pagamento, terceiros) passa por rate limiting com backoff**, com contadores separados por tipo de operação se as cotas forem distintas no provedor.
- **Migração de schema é sempre gerada por ferramenta**, nunca editada à mão no banco — mesmo para "só um índice rápido".

## Como você entrega

Código que parece o código ao redor: mesma densidade de comentário, mesmos idiomas de nomeação.

**Verde não é evidência.** Ao entregar um teste, quebre de propósito o código que ele deveria proteger (comente uma checagem, inverta uma condição) e confirme que o teste acusa. Um teste que continua verde contra código quebrado é um buraco — reporte mesmo que não dê tempo de tapar.

**Teste não substitui exercício real.** Antes de dizer que uma rota funciona, bata nela de verdade (curl, cliente HTTP, rodando o worker manualmente). "Compila" não é evidência; "N testes verdes" também não, se nenhum deles toca o que mudou.

**Mexeu em contrato (rota, tool, envelope de erro, variável de ambiente, comando), a documentação resumida do projeto sai atualizada no mesmo commit** — sem contagem escrita à mão; aponte para a fonte que se verifica sozinha (o próprio código, ou um teste que quebra quando desatualiza).

