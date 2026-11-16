import { TrilhaDeFerramentas } from 'vitral-ui';

const largura = { maxWidth: 640 } as const;

export const GrupoAberto = () => (
  <TrilhaDeFerramentas
    style={largura}
    resumo="Rodou 2 comandos · editou 1 arquivo · leu 1 arquivo"
    falhas={1}
    chamadas={[
      { icone: 'book-open', verbo: 'Leu', alvo: 'tema.rs' },
      { icone: 'brain', verbo: 'Pensou', meta: 'por 9,8 s' },
      { icone: 'file-pen-line', verbo: 'Editou', alvo: 'superficies.rs', mais: 5, menos: 2 },
      { icone: 'terminal', verbo: 'Rodou', alvo: 'npm run build', falhou: true },
      { verbo: 'Rodando', alvo: 'sleep 40', meta: '00:12', rodando: true },
    ]}
  />
);

export const ChamadasSoltas = () => (
  <TrilhaDeFerramentas
    style={largura}
    chamadas={[
      { icone: 'bot', verbo: 'Subagente', prosa: 'conferir cupons', meta: '14 ferramentas · 01:20' },
      { icone: 'terminal', verbo: 'Rodou', alvo: 'cargo test tema' },
      { icone: 'globe', verbo: 'Buscou', prosa: 'preço do frete da Intelipost' },
    ]}
  />
);

export const Recolhido = () => (
  <TrilhaDeFerramentas style={largura} resumo="13 comandos · 1 editado · 3 lidos" aberta={false} />
);
