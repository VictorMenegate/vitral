import { AbaDePainel, BalaoDoUsuario, CabecalhoDoPainel, CaixaDeMensagem, CartaoDeLista, ChipDaCaixa, Materiais, RaizVitral, RespostaDoAgente, RotuloDeSecao } from 'vitral-ui';

export const JanelaNaBase = () => (
  <RaizVitral fundo="base" style={{ display: 'flex', gap: 8, padding: 8, height: 180, borderRadius: 12 }}>
    <div style={{ width: 220, display: 'flex', flexDirection: 'column', gap: 2 }}>
      <CartaoDeLista titulo="WMS" estado="rodando" selecionado painel={1} painelAtivo />
      <CartaoDeLista titulo="Pesagem de caixas" hora="3 d" />
    </div>
    <Materiais material="vidro" style={{ flex: 1, padding: 16 }}>
      <div className="vt-tipo-titulo-tela" style={{ color: 'var(--forte)' }}>WMS</div>
      <div className="vt-tipo-corpo">A raiz liga a Inter, a cor do texto e o fundo do app.</div>
    </Materiais>
  </RaizVitral>
);

export const SobreOCartao = () => (
  <RaizVitral style={{ padding: 16, borderRadius: 12 }}>
    <div className="vt-tipo-corpo">Texto solto na raiz: Inter 15/23 na cor texto.</div>
    <div className="vt-tipo-metadado" style={{ color: 'var(--tenue)' }}>Metadado 12/16 em tenue, com números tabulares: 00:03 · 52%</div>
  </RaizVitral>
);

export const JanelaCompleta = () => (
  <div style={{ height: 420 }}>
    <RaizVitral fundo="base" style={{ display: 'flex', gap: 'var(--vao-entre-pecas)', padding: 'var(--margem-da-janela)', height: '100%', borderRadius: 12 }}>
      <aside style={{ width: 'var(--largura-lateral)', display: 'flex', flexDirection: 'column', gap: 2 }}>
        <RotuloDeSecao contagem={2} tomContagem="espera">Paradas</RotuloDeSecao>
        <CartaoDeLista titulo="WMS" estado="rodando" selecionado painel={1} painelAtivo />
        <CartaoDeLista titulo="Pesagem de caixas" estado="espera" hora="3 h" />
      </aside>
      <Materiais material="vidro" style={{ flex: 1, minWidth: 280, display: 'flex', flexDirection: 'column' }}>
        <CabecalhoDoPainel>
          <AbaDePainel ativa titulo="WMS" estado="rodando" painel={1} painelAtivo />
        </CabecalhoDoPainel>
        <div style={{ flex: 1, padding: 'var(--esp-16)', display: 'flex', flexDirection: 'column', gap: 'var(--esp-12)' }}>
          <BalaoDoUsuario>Confere o estoque do lote 418</BalaoDoUsuario>
          <RespostaDoAgente>
            <p>Conferi: estão todos no lugar.</p>
          </RespostaDoAgente>
        </div>
        <CaixaDeMensagem ativa contexto={0.12} chips={<ChipDaCaixa marca="claude" seta>Opus 5.5</ChipDaCaixa>} />
      </Materiais>
    </RaizVitral>
  </div>
);
