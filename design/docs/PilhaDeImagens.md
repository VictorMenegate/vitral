---
category: Conversa
---
# Pilha de imagens

`<PilhaDeImagens>` no vitral-ui. O que está abaixo vale para o app de verdade (Rust + GPUI); o componente React recria o mesmo desenho.

As imagens que as chamadas escondidas de um grupo recolhido leram ou gravaram, numa linha só: até 4 miniaturas quadradas de 22 sobrepostas 6, como avatares empilhados, e "9 imagens".

## Regras
- Uma imagem só fica na pílula dela (`PilulaDeArquivo`); duas ou mais viram a pilha (dono, 16/11/2026: "tá meio ruim isso aqui, tinha que ser mais organizado": eram pílulas de larguras diferentes quebrando em 8 linhas, com um "+1" solto).
- Altura 28, canto `raio-de-28`, padding 4 e 8, vão `esp-8`; texto `metadado` em `tenue`; hover `hover`.
- Cada miniatura: 22 x 22, canto `raio-controle`, fundo `preto`, borda de 2 em `cartao-opaco` separando uma da outra.
- Recuada para a coluna do texto da trilha (34).
- O clique abre o visor na primeira (as setas passam pelas outras); a dica tem até 10 nomes e "e mais N".

Fonte: `src/ui/mensagens.rs` (`pilha_de_imagens`, `IMAGENS_NA_PILHA`, `dica_da_pilha`).

## Em React

Duas imagens ou mais; recue 34 quando fica embaixo da trilha.

```jsx
<PilhaDeImagens imagens={[a, b, c, d]} total={9} />
```
