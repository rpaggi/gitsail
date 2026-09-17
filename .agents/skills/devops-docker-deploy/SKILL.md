---
name: devops-docker-deploy
description: Mantém o ambiente de desenvolvimento containerizado e o pipeline de deploy — Docker/compose, variáveis de ambiente, segredos, portas — provando cada mudança de infraestrutura com o comando que confirma que funciona, nunca afirmando a partir do arquivo de configuração sozinho.
---

# DevOps — ambiente containerizado e deploy

## Antes de mexer

Busque a base de conhecimento do projeto pela configuração de ambiente já documentada — porta, variável de ambiente, armadilha de permissão de usuário em container, etc. Ambiente de dev tende a acumular pegadinhas medidas uma vez e nunca mais escritas em lugar nenhum; se o projeto já documentou uma, não a redescubra do zero.

## Regras gerais que quase sempre valem

- **O ambiente de desenvolvimento documentado (container/compose) é a fonte de verdade — não suba o processo direto no host "só para conferir uma tela".** Dois processos disputando a mesma porta fazem qualquer teste seguinte falar com o processo errado, silenciosamente.
- **`exec` num container em execução pode não herdar o drop de privilégio que o processo principal faz no boot.** Se o comando principal roda como um usuário não-root mas o `exec` cai em root por padrão, todo arquivo escrito por esse `exec` no bind mount nasce root-owned e quebra o próximo boot. Confirme o usuário efetivo do processo principal e do `exec` separadamente antes de assumir que são o mesmo.
- **Porta publicada em endereço explícito (`127.0.0.1:porta:porta`), não em todas as interfaces, a menos que exposição externa seja intencional** — publicar sem host explícito expõe o serviço de dev em qualquer rede que a máquina alcançar (VPN, LAN), não só localhost.
- **Segredo nunca commitado, nunca logado.** Chave de cifra vem de variável de ambiente/vault, nunca hardcoded; arquivo de configuração com token (`.env`, credencial de MCP/API) é gitignorado.
- **Deploy para produção é ação deliberada** (merge num branch protegido, pipeline explícita), nunca commit direto no branch de produção — mesmo sem proteção técnica de branch configurada, a convenção do time é a trava real.
- **Container de vida longa é obrigatório se o sistema depende de processo em background (worker, cron interno).** Migrar para uma plataforma serverless sem verificar essa dependência quebra o worker silenciosamente.

## Duas instâncias, mesma porta — a armadilha mais cara

Se o projeto tem um ambiente "oficial" (produção/staging real) e um ambiente de dev/bancada que **parecem idênticos** (mesma porta, código parecido), confirme sempre contra qual você está apontando antes de confiar num resultado — trocar o host de um ambiente de teste para apontar "sem querer" para o oficial (ou vice-versa) costuma não dar erro nenhum, só resposta certa vindo do lugar errado.

## Como você entrega

Mudança de infraestrutura vem com o comando que prova que funciona — suba o serviço e mostre-o de pé, não afirme a partir do arquivo YAML/config.

**A documentação resumida do projeto (porta, comando de dev/teste/build, variável de ambiente, passo de deploy) sai atualizada no mesmo commit.** É exatamente o conjunto que, escrito errado, faz a próxima sessão testar contra o processo que não é.

