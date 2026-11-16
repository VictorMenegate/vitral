---
category: Conversa
---
# Cartão de erro

`<CartaoDeErro>` no vitral-ui. O que está abaixo vale para o app de verdade (Rust + GPUI); o componente React recria o mesmo desenho.

O erro de um turno na conversa: o ícone na cor do assunto, o título em português, a frase do que fazer e as ações; o texto cru do agente fica atrás de "Detalhes". A cor fica no ícone e num véu de 6%, nunca num cartão vermelho cheio.

## Cor pelo assunto
- `vermelho`: o turno não pôde seguir (limite, contexto, processo caiu, pasta sumiu, erro desconhecido). Fundo `tinta-leve-vermelho`, fio `tinta-borda-leve-vermelho`.
- `espera` (o azul-claro de ação): precisa de você (login perdido, conversa automática pausada). Fundo `tinta-leve-espera`.
- `ambar`: o turno seguiu com ressalva (Ultracode não ligou). Fundo `tinta-leve-ambar`.
- Neutro: ninguém errou, é só esperar (sem conexão, serviço sobrecarregado). Fundo `grupo`, fio `borda`, ícone `tenue`.

## Anatomia
- Padding 8 contando o fio, canto `raio-cartao`.
- Cabeçalho: o ícone de 16 num lugar de 24 (fica a 12 da borda), o título 14/20 em 600 na cor `texto` (pode quebrar) e o **Copiar** sempre à vista no canto (leva o texto cru, não a frase do Vitral).
- Corpo alinhado ao título (32 de recuo): a frase 13/18 em `tenue`; as ações (botões fantasma de 24, no máximo duas mais o "Detalhes", que é sempre o último) com o ícone do primeiro na coluna do texto; o cru em mono 12 no `recuo`, com as URLs curtas (só o domínio vira o link).
- Medidas: só título, 40; o limite do Codex com Detalhes fechado, 92.

## Regras
- Ações só no erro vivo (nada depois dele) e com o turno parado; num erro que ficou para trás, só o "Detalhes".
- **"Tentar de novo" nunca entra no cartão**: mora na faixa acima da caixa (`FaixaDoTurno`).
- Sem medalhão colorido atrás do ícone, sem botão primário azul: o Grafite tem um destaque só por tela.
- Dois avisos do mesmo problema viram um cartão só (o aviso do Vitral e o erro cru do login).
- O link de uso nunca aponta para "Upgrade to Pro".

## No código Rust (para quem implementa)
`CartaoDeErro::new(icone, cor: Option<Hsla>, titulo)` com `.frase(..)`, `.canto(BotaoCopiar)`, `.acao(..)` (acumula) e `.original(elemento, antes_das_acoes)`.

Fonte: `src/ui/componentes/superficies.rs` (`CartaoDeErro`), `src/ui/mensagens/erros.rs` (leitura e tabela dos erros), `src/ui/mensagens.rs` (`cartao_de_erro`), especificação `arquivo/balao-erro-spec-2026-11-16.md`.

## Em React

Na conversa, depois do turno que falhou. `acoes` no máximo duas; `detalhes` acrescenta o "Detalhes".

```jsx
<CartaoDeErro
  tom="vermelho"
  icone="gauge"
  titulo="O Codex chegou ao limite de uso"
  frase="Volta às 12:30."
  acoes={[{ rotulo: 'Ver o uso', icone: 'external-link' }, { rotulo: 'Continuar no Claude', icone: 'shuffle' }]}
  detalhes="You've hit your usage limit."
/>
```
