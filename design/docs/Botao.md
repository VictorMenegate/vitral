---
category: Base
---
# Botão

`<Botao>` no vitral-ui. O que está abaixo vale para o app de verdade (Rust + GPUI); o componente React recria o mesmo desenho.

O botão com texto ou só com ícone, sobre o `Button` do kit: fantasma é o padrão, o primário azul é raro e o claro serve ao "Permitir" do pedido de permissão.

## Variantes
- **Fantasma** (`botao_fantasma`): transparente, `hover` e `pressionado`; ícone em `tenue` ao lado do rótulo em `texto`. É o botão de quase tudo (Retomar, Tentar de novo, Detalhes, Compactar).
- **Primário** (`botao_primario`): `destaque` com `sobre-destaque` (4,8:1); o diálogo com ação e o boas-vindas. Um destaque só por tela: nunca primário azul num cartão de erro.
- **Perigo**: rótulo e ícone em `vermelho`, `perigo-hover` no hover.
- **Claro**: `claro` com `sobre-cor`; o "Permitir", para o azul chapado não disputar com a moldura do pedido.
- **Só ícone** (`botao_icone`): quadrado da altura, ícone em `tenue`, sempre com dica.
- **Botão do aviso**: 28, rótulo 13/600 em `destaque-texto`, sem fundo ("Desfazer", "Abrir").

## Medidas
| Tamanho | Altura | Ícone | Padding | Canto |
| --- | --- | --- | --- | --- |
| Linha | 24 | 14 | 8 | `raio-de-24` |
| Barra | 28 | 16 | 12 | `raio-de-28` |
| Controle | 32 | 16 | 12 | `raio-de-32` |
Rótulo em `rotulo` (13/18, 500). Canto pelo padrão `raio::de(altura)`: bem arredondado, nunca redondo.

## Regras
- O ícone vem antes do rótulo; para avançar, o relatório de design (D11) propõe o ícone no fim ("Continuar →").
- Diálogo só informativo: "Fechar" primário. Diálogo com ação: "Cancelar" fantasma mais o primário. "Remover" tira de uma lista, "Apagar" destrói.
- Desabilitado a `opacidade-desabilitado`, com o motivo na dica.

Fonte: `src/ui/componentes/controles.rs` (`botao_fantasma`, `botao_icone`, `botao_primario`, `botao_perigo`, `botao_claro`, `TamanhoBotao`), `src/ui/componentes/superficies.rs` (`botao_do_aviso`).

## Em React

`variante` escolhe o material (fantasma é o padrão), `tamanho` a altura, `icone` o Lucide antes do rótulo. Sem `children`, é só ícone e pede `dica`.

```jsx
<div style={{ display: 'flex', gap: 8 }}>
  <Botao icone="play">Retomar</Botao>
  <Botao tamanho="linha" icone="chevron-down">Detalhes</Botao>
  <Botao variante="claro">Permitir</Botao>
  <Botao tamanho="barra" icone="plus" dica="Novo chat" />
</div>
```
