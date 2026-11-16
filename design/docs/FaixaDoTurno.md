---
category: Caixa
---
# Faixa do turno

`<FaixaDoTurno>` no vitral-ui. O que está abaixo vale para o app de verdade (Rust + GPUI); o componente React recria o mesmo desenho.

A faixa de 28 logo acima da caixa de mensagem diz o que parou e oferece o botão que resolve: Retomar, Tentar de novo ou Compactar.

## Quando usar
Só para o que pede um botão seu. O turno em andamento não usa a faixa: mora na linha de estado dentro da moldura da caixa. Ordem: o que roda ou parou vale mais; parado, a pasta que sumiu; por último, a fila e a conversa grande que esfriou.

## Situações
| Situação | Ícone e cor | Texto | Ação |
| --- | --- | --- | --- |
| Parada que pede Retomar | Pause em `espera` (em `tenue` se a parada foi sua) | "Parada · Vitral fechou" (o motivo inteiro na dica) | Retomar (Play) |
| Limite | Gauge em `vermelho` | "Limite de uso do Codex · volta 12:30" | Tentar de novo |
| Login | KeyRound em `espera` | "Sem login no Claude Code" | Tentar de novo |
| Rede, sobrecarga | WifiOff, Clock em `tenue` | "Sem conexão", "Serviço sobrecarregado" | Tentar de novo |
| Contexto | Minimize em `vermelho` | "Conversa longa demais para o modelo" | Tentar de novo |
| Pasta sumiu | FolderX em `vermelho` | "A pasta ~/x não foi encontrada" | nenhuma |
| Fria | Clock em `tenue` | "Relê 182 mil tokens · parada há 3 h" | Compactar |

## Regras
- Altura `altura-barra`, vão `esp-8`, texto 13/18 em `tenue`, numa linha só com reticências no fim (o dono viu "Parada · Vitral" / "fechou" em duas linhas). A ação fica sempre inteira; quem corta é o texto.
- Alinhada com o texto da caixa à esquerda e com o enviar à direita.
- O "Tentar de novo" mora só aqui: nunca repetido no cartão de erro (dois botões iguais perto um do outro já foram cortados).
- Mesmo formato da lista e do sino ("Parada · Vitral fechou").

Fonte: `src/ui/mensagens.rs` (`faixa_atividade`), `src/ui/mensagens/erros.rs` (`na_faixa`).

## Em React

Logo acima da CaixaDeMensagem, alinhada com ela.

```jsx
<FaixaDoTurno icone="pause" tom="espera" acao="Retomar" iconeAcao="play">Parada · Vitral fechou</FaixaDoTurno>
<FaixaDoTurno icone="gauge" tom="vermelho" acao="Tentar de novo" iconeAcao="rotate-cw">Limite de uso do Codex · volta 12:30</FaixaDoTurno>
```
