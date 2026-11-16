---
category: Caixa
---
# Caixa de mensagem

`<CaixaDeMensagem>` no vitral-ui. O que está abaixo vale para o app de verdade (Rust + GPUI); o componente React recria o mesmo desenho.

Onde se escreve para o agente: uma moldura preta de raio 20 com 2 de folga e, dentro, o campo opaco de raio 18 com o texto e a barra de chips; no painel ativo ela pousa sobre a luz azul de baixo.

## Anatomia
- **Moldura**: fundo `preto`, raio `raio-peca` (20), fio de 1 transparente e `esp-2` de folga (eram 4; dono, 16/11/2026: "diminui as bordas").
- **Miolo**: raio 18 (concêntrico com a moldura), fio `borda`, fundo `cartao-elevado`, a `linha-de-luz` no topo; padding 12 em cima, 8 nos lados e embaixo (8, 6 e 6 na faixa estreita).
- **Campo**: 15/22 em `texto`; o placeholder em `tenue` cai na coluna do ícone do primeiro chip.
- **Barra** (32; 28 na estreita): à esquerda o chip do modo, o do modelo e o do esforço (ver `ChipDaCaixa`); à direita o anel do contexto com a porcentagem, o clipe, o "⋯" e o enviar.
- **Linha de estado** (só com o chat rodando): 22 de altura dentro da moldura, `etiqueta` mono 11 em `tenue`: o anel girando, o relógio "00:03", os agentes e a tecla Esc.
- **Enviar**: tecla de metal redonda de 36 (32 na estreita). Vazio, só o fio `borda` e a seta `apagado`; com texto, o metal acende em 120 ms. Rodando, vira o parar (o quadrado), e com texto aparecem os dois.

## Estados
- **Foco** ("é aqui"): o aro de metal de 2 px acende em volta da moldura em 180 ms e apaga em 120. Sem halo, sem azul chapado, sem cantoneira.
- **Painel ativo**: a luz azul embaixo da caixa (a doca): o metal inteiro encolhido com `opacidade-luz-da-doca` de `destaque` por cima, sumindo para o escuro do cartão em cima e nos lados (dono, 12/11 e 13/11/2026: "menos nos lados pra ficar mais no meio só"). Só o painel selecionado a tem.
- **Painéis de fundo**: o modelo, o esforço, o clipe, o "⋯" e o modo ficam a `opacidade-em-repouso` e acendem no hover da caixa. Sempre acesos: o chip do modo Sem perguntar (aviso de segurança), o anel com a porcentagem, o parar, o campo e a moldura.

## No código Rust (para quem implementa)
O chat (modo, modelo, esforço, contexto), a faixa da caixa (`FaixaCaixa::Estreita` abaixo de 35 rem, `Larga` acima de 46), os anexos e a citação (moram na moldura, em cima do campo) e se o painel é o ativo.

Fonte: `src/ui/composer.rs` (`caixa_mensagem`, `barra_da_caixa`, `botoes_de_envio`, `anel_do_contexto`), `src/ui/conversa.rs` (a doca), `src/ui/componentes/metal.rs` (`aro_de_metal`).

## Em React

No pé da peça de conversa; os chips vão em `chips`, na ordem modo, modelo, esforço. `ativa` só no painel selecionado.

```jsx
<Materiais material="vidro">
  {/* cabeçalho e conversa */}
  <CaixaDeMensagem
    ativa
    foco
    valor="ola painel dois"
    contexto={0.12}
    chips={
      <>
        <ChipDaCaixa modo="sem-perguntar" seta />
        <ChipDaCaixa marca="claude" seta>Opus 5.5</ChipDaCaixa>
        <ChipDaCaixa esforco={4} seta>Auto · Extra</ChipDaCaixa>
      </>
    }
  />
</Materiais>
```
