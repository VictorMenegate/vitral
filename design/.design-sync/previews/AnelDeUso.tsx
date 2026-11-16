import { AnelDeUso, Botao, Materiais } from 'vitral-ui';

export const Faixas = () => (
  <div style={{ display: 'flex', gap: 16, alignItems: 'center' }}>
    <AnelDeUso marca="claude" fracao={0.42} inicial="P" />
    <AnelDeUso marca="codex" fracao={0.12} />
    <AnelDeUso marca="claude" fracao={0.78} />
    <AnelDeUso marca="claude" fracao={1} />
    <AnelDeUso marca="codex" apagado dica="Codex não instalado" />
  </div>
);

export const NoCantoDaJanela = () => (
  <Materiais material="vidro" style={{ height: 130, width: 320 }}>
    <div style={{ display: 'flex', justifyContent: 'flex-end', padding: 8 }}>
      <Botao tamanho="barra" icone="x" dica="Fechar o Vitral" />
    </div>
    <div style={{ position: 'absolute', right: 6, top: 52, display: 'flex', flexDirection: 'column', gap: 6 }}>
      <AnelDeUso marca="claude" fracao={0.42} inicial="P" />
      <AnelDeUso marca="codex" fracao={0.12} />
    </div>
  </Materiais>
);
