---
category: Conversa
---
# Resposta do agente

`<RespostaDoAgente>` no vitral-ui. O que está abaixo vale para o app de verdade (Rust + GPUI); o componente React recria o mesmo desenho.

A resposta corre solta na coluna, sem balão nem cartão: texto em `corpo` (15/23) na cor `texto`, com a trilha das ferramentas entre os parágrafos e a barra de ações no pé.

## Anatomia
- Markdown com títulos de 18, 16 e 15; parágrafos a `esp-12`; blocos de código e tabelas no `preto` com canto `raio-cartao`; código em linha em `codigo` sobre `hover`.
- Caminhos que existem no disco viram link em `destaque-texto` e abrem no painel da direita.
- Imagens que a resposta aponta viram pílulas (ver `PilulaDeArquivo`), nunca a imagem solta no tamanho do texto.
- Entre os parágrafos, a trilha das chamadas (ver `TrilhaDeFerramentas`): recolhida, uma linha de resumo e, se houver, a pilha das imagens que as chamadas leram.
- **Barra de ações** no pé, a 8 abaixo do fim, à direita: a hora e o Copiar. Em cima ela cobria metade da primeira linha (relatório de aparência, V7).

## Ritmo
Dentro do turno, `esp-no-turno` (12) entre os blocos; linhas de log seguidas ficam coladas; 32 antes do próximo balão.

## No código Rust (para quem implementa)
O texto (markdown), a pasta do chat (para ligar os caminhos), a hora, o realce da busca.

Fonte: `src/ui/mensagens.rs` (`texto_assistente`, `estilo_das_mensagens`, `estilo_da_resposta`, `espaco_antes`).

## Em React

O texto do agente como filho (p, h2, ul, a, code, b); trilha, pilha e citação entram como irmãos, a 12 um do outro.

```jsx
<div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
  <RespostaDoAgente><p>Vou conferir onde o balão é montado.</p></RespostaDoAgente>
  <TrilhaDeFerramentas resumo="13 comandos · 1 editado · 3 lidos" aberta={false} />
  <RespostaDoAgente acoes hora="12:53">
    <p>Pronto. Os caminhos viram link, como <a href="#">src/ui/mensagens.rs</a>.</p>
  </RespostaDoAgente>
</div>
```
