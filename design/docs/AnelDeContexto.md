---
category: Caixa
---
# Anel de contexto

`<AnelDeContexto>` no vitral-ui. O que está abaixo vale para o app de verdade (Rust + GPUI); o componente React recria o mesmo desenho.

O anel com fenda mostra quanto da janela de contexto a conversa já usa, com a porcentagem **ao lado**, em mono; o mesmo anel aberto e girando é o "carregando" do app.

## Regras do anel
- Todo anel tem uma fenda de 60 graus embaixo e pontas redondas: vai das 7 às 5 horas, com o trilho `borda-forte` e o arco da fração por cima. Espessura max(13% do diâmetro, 3).
- A cor do arco segue a faixa (`cor_da_faixa`): a base até 69%, `ambar` de 70% a 89%, `vermelho` a partir de 90%.
- **A porcentagem fica sempre ao lado do anel** (dono: "anel de contexto com a porcentagem"), em `etiqueta` (mono 11): `tenue` até 69%, na cor da faixa depois. Sem número dentro do anel: lia como selo de notificação. Também na caixa estreita: só o anel, com a fenda e o arco azul parcial, ficava igual ao anel girando de "rodando".
- O arco desliza até a fração nova em 240 ms.

## O gatilho
Botão fantasma de 32 (28 na estreita), padding `esp-8` (`esp-4`), vão `esp-6`, anel de 20. O clique abre o uso da conversa: o contexto e o custo, o modelo, a conta e o Compactar.

## Anel girando
`AnelGirando`: uma volta a cada 1,2 s, sem trilho, em `destaque`. 14 nas linhas de ferramenta, 12 na linha de estado da caixa. Com o movimento reduzido, parado com a fenda embaixo.

Fonte: `src/ui/componentes/graficos.rs` (`Anel`, `AnelGirando`, `cor_da_faixa`, `pintar_arco`), `src/ui/composer.rs` (`anel_do_contexto`). Nota: a recriação em CSS tem as pontas retas; no app elas são redondas.

## Em React

Na caixa ele já vem pronto (CaixaDeMensagem `contexto`); solto, para popovers de uso e o carregando.

```jsx
<AnelDeContexto fracao={0.74} />
<AnelDeContexto carregando />
<AnelDeContexto fracao={0.38} tamanho={28} porcentagem={false} />
```
