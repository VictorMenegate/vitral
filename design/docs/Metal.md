---
category: Materiais
---
# Metal

`<Metal>` no vitral-ui. O que está abaixo vale para o app de verdade (Rust + GPUI); o componente React recria o mesmo desenho.

O metal líquido é a assinatura do Vitral: uma chapa atrás de tudo (a receita "Mesh drift" que o dono montou e colou em 12/11/2026: "tenta focar em vez de glass esse metal líquido"), e os recortes dela que dizem "é aqui", "escolhido" e "ação".

## A receita
Cinco bolhas gaussianas (`metal-1` quase preta e quatro azuis, `metal-2` a `metal-5`) andando em órbitas próprias; a cor de cada ponto é a média das cinco pesada pela distância. Por cima: contraste 1,37, saturação 0,68, brilho -0,02 (a `paleta_acabada`, `metal-acabado-1` a `metal-acabado-5`), vinheta 0,54, grão de filme 0,22 e uma luz que segue o ponteiro (força 0,5, raio 0,8). A cor mais clara que ela entrega é o `metal-teto` (#ACD9FF): todo teste de contraste roda contra ela.

## Os recortes
- **A chapa e a base**: a janela inteira é a chapa debaixo do `vidro-leitura` (11% passa). A lista mora direto na base; os cartões flutuam por cima.
- **A ficha do escolhido**: o miolo do campo (`Encaixe::Miolo`, o recorte aumentado 2,5 vezes em volta do centro, sem a vinheta das pontas), com o grão leve, o `veu-de-placa` (marinho a 64%) e a `aresta-de-ficha`. É o "escolhido" do app inteiro: segmentados, listas e popovers de escolha (dono: "o metal azul como escolhido").
- **A placa** do cabeçalho dos menus: o mesmo miolo e o mesmo véu, 52 de altura, com os cantos de cima do menu.
- **A tecla de metal** (o + da lista, o enviar, o parar): o campo inteiro encolhido (`Encaixe::Cobrir`) com o `destaque` a 60% por cima: o botão continua sendo "o azul" da tela, com o metal vivo por baixo.
- **O aro de metal** ("é aqui"): 2 px em volta de um miolo opaco, na caixa de mensagem com foco, na busca com foco e na aba sob o ponteiro; 4 px na moldura do pedido que espera você.
- **A doca** (a luz azul de baixo): só o painel ativo; o campo encolhido com 20% do destaque, sumindo para o escuro do cartão em cima e nos lados.

## Regras
- Texto nunca vai no metal cru. Sobre o `veu-de-placa`, só `forte` e `texto`; sobre o `veu-de-botao`, só ícone branco.
- Sem halo, sem degradê azul e sem borda azul: o "é aqui" é sempre um pedaço do metal.
- **Quando anda**: em repouso o metal fica parado e nada redesenha; ele anda a 20 quadros por segundo só com um chat à vista trabalhando, e só então a luz segue o ponteiro. O botão do logo anima com o ponteiro em cima. Com a janela sem foco ou o movimento reduzido, parado na fase 66,5.
- Nos níveis de transparência (Leve, Média, Forte) a chapa perde opacidade (0,5, 0,35, 0,3) e o papel de parede aparece atrás; no Opaco (o padrão) ela é opaca.

## Nesta recriação
Os previews pintam o metal com gradientes parados nas cores da `paleta_acabada` e um grão de SVG; no app o campo é calculado (uma grade de 1 ponto a cada 8 px, esticada pela placa de vídeo, e o grão num ladrilho de 256).

Fonte: `src/ui/componentes/metal.rs`, `src/ui/componentes/malha.rs`, `src/tema.rs` (`veu_de_placa`, `veu_de_botao`, `NIVEIS_DE_TRANSPARENCIA`), `src/ui/componentes/superficies.rs` (`ficha_do_escolhido`, `placa_de_metal`), `src/ui/conversa.rs` (a doca).

## Em React

`tipo="base"` é o fundo da janela; os outros recortes dão tamanho e canto pelo `style`.

```jsx
<Metal tipo="base" style={{ display: 'flex', gap: 8, padding: 8, height: '100vh' }}>
  {/* a lista mora aqui; as peças flutuam por cima */}
</Metal>
<Metal tipo="ficha" style={{ height: 40, padding: '0 12px', display: 'flex', alignItems: 'center' }}>Padrão</Metal>
```
