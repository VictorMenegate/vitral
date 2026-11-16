---
category: Conversa
---
# Marco da conversa

`<MarcoDaConversa>` no vitral-ui. O que está abaixo vale para o app de verdade (Rust + GPUI); o componente React recria o mesmo desenho.

Um fato do turno numa linha só, no meio de dois traços: "Turno interrompido", "O Vitral fechou no meio do turno", "Sessão nova", o dia que começa ("Hoje").

## Anatomia
- Dois traços de 1 px em `linha` e, no meio, o ícone de 12 e o texto `metadado` em `tenue`, até 80% da largura (duas linhas no máximo, com reticências).
- O separador de dia usa o mesmo molde, com o nome em 600.
- 16 em volta de um marco (`esp-marco`); 32 antes do separador de dia (começa um turno).
- **Linha de informação** (troca de conta ou de agente, nova tentativa): uma linha de log de 32 com o ícone de 14 e o texto 13/18, tudo em `tenue`. Ícone pelo assunto: Shuffle na troca, Gauge no limite, RotateCw na nova tentativa, Info no resto.

## Regras
- O marco é impessoal e sem instrução: "O Vitral fechou no meio do turno", sem "use Retomar para o Claude continuar" (a faixa logo abaixo já tem o Retomar) e sem nome de agente.
- No fim de um painel parado, o marco que repete a faixa ("Parada · Vitral fechou") não é desenhado.
- A sessão antiga que sumiu ou expirou é marco ("Sessão nova, com um resumo da anterior"), não cartão vermelho: o turno continuou.

Fonte: `src/ui/mensagens.rs` (`aviso_do_chat`, `tipo_aviso`, `separador_de_dia`).

## Em React

Entre os turnos, com 16 em volta.

```jsx
<MarcoDaConversa icone="circle-pause">O Vitral fechou no meio do turno</MarcoDaConversa>
<MarcoDaConversa tipo="dia">Hoje</MarcoDaConversa>
<MarcoDaConversa tipo="info" icone="shuffle">Daqui em diante quem responde é o Codex.</MarcoDaConversa>
```
