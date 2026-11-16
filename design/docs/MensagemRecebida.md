---
category: Conversa
---
# Mensagem recebida

`<MensagemRecebida>` no vitral-ui. O que está abaixo vale para o app de verdade (Rust + GPUI); o componente React recria o mesmo desenho.

Uma mensagem que outro chat (ou o próprio Vitral) entregou nesta conversa: quem mandou numa linha de 28 e uma prévia; a linha do nome abre e fecha o texto inteiro.

## Anatomia
- Cabeçalho de 28: o avatar num lugar de 24 (o logo do Vitral de 20, ou o círculo do papel do chat), o nome 13/18 em 600 na cor `texto`, e à direita a seta (chevron de 12 em `tenue`) que dá lugar às ações no hover (a hora, "Abrir o chat", Copiar).
- Fechada: a prévia em `corpo-ui` `tenue`, recuada 32 dos dois lados, numa linha (duas no painel largo), com reticências.
- Aberta: o texto em `corpo` na cor `texto`, recuado 32.

## No código Rust (para quem implementa)
Quem mandou (`de`), o texto, a hora; o chat de origem, se ainda existe, para o "Abrir o chat" (Ctrl+clique abre ao lado).

Fonte: `src/ui/mensagens.rs` (`mensagem_recebida`, `lugar_de_acoes`).

## Em React

`de` é quem mandou ("Vitral" põe o logo); `aberta` mostra o texto inteiro.

```jsx
<MensagemRecebida de="Vitral">Antes de encerrar: reveja o que decidiu e anote na memória da equipe.</MensagemRecebida>
<MensagemRecebida de="Pesagem de caixas na expedição" aberta>A balança da bancada 2 voltou a responder.</MensagemRecebida>
```
