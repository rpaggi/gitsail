---
name: ui-video-evidence
description: Grava evidência em vídeo de um fluxo de UI usando Playwright em container — ritmo de gravação humano (devagar, com pausas e digitação gradual, não headless-turbo) — e organiza o resultado numa pasta de evidências (vídeo .mp4 + README com os critérios validados). Use quando o usuário pedir "evidência em vídeo", "grave um vídeo validando X" ou equivalente. Passe o que deve ser testado e em qual tela como argumento.
---

# Gravar evidência em vídeo de um fluxo de UI

Demanda do usuário:

> **$ARGUMENTS**

Se `$ARGUMENTS` não deixar claro **o quê** validar e **em qual tela/aplicação**, pergunte antes de prosseguir.

## Por que existe

Sessões de gravação real encontram defeitos que teste de API não pega — ícone quebrado renderizando em branco, toast de erro escondido atrás de modal, cabeçalho lendo dado desatualizado. Nenhum deles quebra requisição; todos quebram o uso. Vídeo bem feito vale mais que capturas soltas.

## Regras obrigatórias

1. **Rode o Playwright em container, não no host**, a menos que o host já tenha o Chromium/dependências do sistema instalados e validados — instalar via `npx playwright install` costuma faltar bibliotecas do sistema fora de um container preparado para isso. Use a imagem oficial do Playwright:
   ```bash
   docker run --rm --network host --ipc=host \
     -v "$SCRATCH:/work" -w /work \
     -e BASE_URL=http://localhost:<PORTA> -e OUT_DIR=/work/evidencia \
     --user "$(id -u):$(id -g)" \
     mcr.microsoft.com/playwright:v1.62.0-noble node /work/roteiro.js
   ```
   Se o alvo estiver atrás de uma rede de container (compose), `--network host` costuma ser necessário para a página conseguir falar com a API do próprio ambiente — a rede interna do compose normalmente não é alcançável de fora sem isso.

2. **Resolução moderada** (ex.: `{ width: 1280, height: 800 }`). Nunca grave em full-HD/1920 sem necessidade — o vídeo fica mais pesado sem ganho de legibilidade.

3. **Ritmo humano — devagar, não um bot disparando ações instantâneas:**
   - `slowMo: 300–500` no lançamento do navegador.
   - Pausas explícitas entre passos (antes de clicar, depois de navegar, depois de uma resposta assíncrona).
   - Digitação caractere a caractere (com delay) no campo que é o **ponto observável** do teste — o que é secundário pode ser preenchido instantaneamente.
   - `hover()` antes de clicar.
   - Ao abrir dropdown/modal, aguarde o elemento e faça uma pausa visual antes de interagir.
   - Não otimize para vídeo curto — otimize para legível.

4. **Legendas sobrepostas.** Um elemento fixo no rodapé, atualizado a cada passo, explicando o que está acontecendo. O vídeo deve se explicar sozinho, sem depender de um README ao lado.

5. **Usuário de teste.** Verifique primeiro se já existe um usuário de teste utilizável. Se não houver um confiável, o próprio roteiro deve criar um pela UI (dado único, ex. e-mail com timestamp) como primeiro passo — evite criar direto no banco por atalho, a menos que o cadastro via UI não seja viável para o que está sendo testado.

6. **Conversão e destino final.**
   - Grave com a opção de gravação de vídeo do contexto do navegador (sai como `.webm`).
   - Converta para `.mp4` (`ffmpeg -c:v libx264 -crf 26`).
   - Salve numa pasta de evidências com nome descritivo: o `.mp4` + um `README.md` curto — o que foi testado, passo a passo, resultado de cada critério de aceite, e qualquer defeito encontrado no caminho.
   - Não commite a evidência automaticamente — deixe no working tree; commit é decisão separada do usuário.

7. **Limpeza.** Se o roteiro criou dado só para a gravação, remova ao final.

## Passo a passo

1. Confirme que o ambiente-alvo está de pé; suba-o se necessário, sempre pelo mecanismo documentado do projeto.
2. Leia os componentes/telas reais envolvidas para descobrir seletores — não adivinhe.
3. Escreva o roteiro Playwright num diretório de trabalho temporário, seguindo as regras acima.
4. Rode via `docker run`. Itere se algo quebrar.
5. Converta o vídeo, organize a pasta de evidência, escreva o README.
6. Reporte ao usuário: passou/falhou em cada critério, defeitos encontrados, caminho dos arquivos — envie o vídeo.

## Delegação

Se a bateria cobrir múltiplos fluxos, considere um agente por fluxo em paralelo (cada um grava seu próprio vídeo) em vez de um roteiro monolítico tentando cobrir tudo.

