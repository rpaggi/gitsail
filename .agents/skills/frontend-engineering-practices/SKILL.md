---
name: frontend-engineering-practices
description: Implementa páginas e componentes de interface com atenção a sanitização de conteúdo não confiável, estados assíncronos visíveis ao usuário, e verificação visual real da tela — não apenas teste automatizado verde.
---

# Engenharia de front-end — implementação e verificação

Seu território real (framework, pastas, biblioteca de componentes) vem da documentação local do repositório — leia-a antes de codar. Esta skill é sobre disciplina de entrega de UI, não sobre uma stack específica.

## Antes de codar

Busque a base de conhecimento do projeto (se houver) pelo desenho de tela e pelos contratos de API que a tela consome, antes de escrever a primeira linha.

## Regras gerais que quase sempre valem

- **Conteúdo de origem não confiável (markdown de usuário, resposta de IA, HTML de terceiro) sempre passa por sanitização antes de renderizar como HTML.** Inserir HTML cru sem sanitizar é uma vulnerabilidade (XSS), não um atalho de produtividade.
- **Idioma da interface segue o padrão do projeto**, mesmo que os valores internos (enums do banco, códigos de status) estejam em outro idioma — traduza na camada de apresentação, nunca envie o rótulo traduzido de volta para a API.
- **Toda operação assíncrona visível ao usuário (upload, processamento em background, indexação) precisa de um estado de "em andamento" na tela.** Uma tela que finge ter terminado enquanto o servidor ainda processa é uma tela mentindo.
- **Mudança que invalida dado existente (troca de configuração que exige reprocessamento, ação destrutiva) avisa antes de executar, não depois.**
- **Nunca peça de volta um segredo que a API só devolve como booleano** (ex.: "está configurado: sim/não") — se o campo é write-only por desenho da API, a tela respeita isso.
- **Trate o envelope de erro da API explicitamente** — não presuma sempre 200/sucesso.

## Como você entrega

Componentes que parecem os componentes ao redor. Não introduza uma biblioteca de UI nova sem alinhar com quem decide arquitetura — a stack existente quase sempre já cobre o caso.

**Teste automatizado não substitui abrir a tela.** Exercite o fluxo de verdade (no ambiente de desenvolvimento documentado do projeto, nunca um processo solto no host se o projeto usa container) antes de dizer que funciona. Evidência visual (captura de tela) vale mais que descrição.

Se a lógica de verdade (parsing, cálculo, validação) está presa dentro de um componente e é arriscada o bastante para merecer teste, extraia para uma função pura testável em vez de deixar intestável dentro do componente.

**Mexeu em rota nova que vira referência, comando de build/teste do front, ou convenção de componente — a documentação resumida do projeto sai atualizada no mesmo commit.**

