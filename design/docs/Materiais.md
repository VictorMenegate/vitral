---
category: Materiais
---
# Materiais

`<Materiais>` no vitral-ui. O que está abaixo vale para o app de verdade (Rust + GPUI); o componente React recria o mesmo desenho.

Os três materiais do Vitral são feitos só de luz e sombra de 1 px por dentro: vidro nas peças, tecla no que está em relevo e escolhido, poço no que é cavado. Nada vaza para fora de uma peça translúcida.

## Vidro (`aresta-de-vidro`)
Nas peças (a conversa, os painéis da direita): o fio claro de 1 px (`aro`) só nos dois lados, a sombra de 1 px na base (`sombra-de-baixo`) e um brilho de 3% (`brilho-de-dentro`) que entra uns 40 px pelo topo. Sem o fio em cima (dono, 13/11/2026: "essa bordinha de cima"). A aresta mora numa camada à parte, o último filho da peça; a peça não leva `border`. As peças levam `sombra-flutuante` no nível Opaco.

## Tecla (`aresta-de-tecla`)
Luz de 1 px em cima a 16% e sombra de 1 px embaixo a 30%: a barra de ações de uma mensagem, a tecla de atalho, a citação na caixa, a pílula do segmentado de cor, o item sob o ponteiro no menu. Nunca em cada linha de uma lista. A ficha de metal usa a `aresta-de-ficha`, com a luz a 22%.

## Poço (`aresta-de-poco`)
A sombra cai de cima para dentro e a quina de baixo pega 1 px de luz: a busca, o trilho do segmentado, o contador, a faixa do pé do menu. Fundo `recuo`.

## Degraus
Cada superfície num nível de claro bem diferente, um quase preto frio e sem cor: a base (`vidro-leitura` sobre o metal), os cartões (`cartao`), o elevado (`cartao-elevado`), o flutuante (`flutuante`, um degrau mais claro que a peça de onde sai) e o preto de dentro (`preto`). O que separa as peças é o degrau de tom, e o que marca o foco é o metal, não a borda.

## Retícula
O fio de pontos (2 aceso, 2 apagado, em `fio`) substitui toda linha cheia: separadores de menu, grupos das Configurações (recuado 12), rodapés de teclas. A trama fina (xadrez de 2 px em `pontilhado-fino`) e a hachura diagonal (o que saiu) são as outras duas texturas.

Fonte: `src/tema.rs` (`aresta_de_vidro`, `aresta_de_tecla`, `aresta_de_ficha`, `aresta_de_poco`), `src/ui/componentes/superficies.rs` (`camada_de_aresta`), `src/ui/componentes/graficos.rs` (`fio_de_pontos`, `trama`, `hachura`), `vendor/gpui-component/src/styled.rs` (`popover_shadow`).

## Em React

`material="vidro"` é a peça da janela (a conversa, o painel da direita); `casca` para um flutuante novo; `fio` no lugar de toda linha.

```jsx
<Materiais material="vidro" style={{ flex: 1, minWidth: 280 }}>
  {/* CabecalhoDoPainel, a conversa e a CaixaDeMensagem */}
</Materiais>
<Materiais material="fio" />
```
