---
category: Conversa
---
# Citação com copiar

`<CitacaoComCopiar>` no vitral-ui. O que está abaixo vale para o app de verdade (Rust + GPUI); o componente React recria o mesmo desenho.

Uma citação em bloco ("> ...") na resposta ganha o botão de copiar no canto, como o bloco de código: é onde o agente escreve a mensagem pronta para mandar a outra pessoa (dono, 16/11/2026: "esse tipo de mensagem aqui tem que ter botão pra copiar").

## Anatomia
- Fio de 2 px à esquerda em `borda-forte`, recuo de `esp-12` e 32 à direita (a coluna do Copiar).
- O texto na cor `texto`, em `corpo`: a citação não é mais o cinza `tenue` (relatório de aparência, V13). O negrito fica em 700 e `forte`.
- O **Copiar** é o botão de linha de 24 com ícone de 14 em `tenue` no canto de cima à direita; copia o texto sem as marcas do markdown. No clique, o ícone vira o visto em `verde` por 2 s.
- O mesmo Copiar vale para o bloco de código (preto, canto `raio-cartao`, 40 à direita guardados para o botão nunca ficar sobre um caractere), com a linguagem na dica.

## No código Rust (para quem implementa)
O trecho citado (a resposta é partida em trechos; cada citação é desenhada à parte com o mesmo estilo) e a dica ("Copiar a citação").

Fonte: `src/ui/mensagens.rs` (`texto_assistente`, `trechos_com_citacoes`, `texto_da_citacao`, `estilo_das_mensagens`), `src/ui/componentes/controles.rs` (`BotaoCopiar`).

## Em React

A mensagem pronta para mandar a alguém; `tipo="codigo"` para o bloco de código.

```jsx
<CitacaoComCopiar>
  Carol, arrumei as duas coisas. Aperta <b>"Só os contados"</b> lá em cima.
</CitacaoComCopiar>
<CitacaoComCopiar tipo="codigo">cargo test tema</CitacaoComCopiar>
```
