import { AbaDePainel, AnelDeUso, CabecalhoDoPainel, Materiais, Metal } from 'vitral-ui';

export const PainelComAbas = () => (
  <Metal tipo="base" style={{ padding: 8, borderRadius: 12 }}>
    <Materiais material="vidro">
      <CabecalhoDoPainel>
        <AbaDePainel quadrada titulo="Alertas de cupons" />
        <AbaDePainel quadrada titulo="Eita Gestão" />
        <AbaDePainel ativa titulo="Nova conversa" painel={1} painelAtivo fechar />
      </CabecalhoDoPainel>
      <div style={{ height: 40 }} />
    </Materiais>
  </Metal>
);

export const UltimoPainelComUso = () => (
  <Metal tipo="base" style={{ padding: 8, borderRadius: 12 }}>
    <Materiais material="vidro">
      <CabecalhoDoPainel fecharJanela>
        <AbaDePainel ativa titulo="Projeto Vitral com integração GitHub" estado="espera" painel={3} />
      </CabecalhoDoPainel>
      <div style={{ position: 'relative', height: 96 }}>
        <div style={{ position: 'absolute', right: 6, top: 8, display: 'flex', flexDirection: 'column', gap: 6 }}>
          <AnelDeUso marca="claude" fracao={0.42} inicial="P" />
          <AnelDeUso marca="codex" fracao={0.12} />
        </div>
        <div className="vt-tipo-corpo" style={{ padding: '4px 56px 0 16px', color: 'var(--texto)' }}>
          A conversa passa por baixo dos anéis de uso, que ficam no canto de cima, embaixo do ×.
        </div>
      </div>
    </Materiais>
  </Metal>
);
