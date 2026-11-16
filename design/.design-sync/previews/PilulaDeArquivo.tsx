import { BalaoDoUsuario, PilulaDeArquivo } from 'vitral-ui';
import { balao, inicio, wms } from './_capturas';

const fileira = { display: 'flex', gap: 6, flexWrap: 'wrap', alignItems: 'center' } as const;

export const NaConversa = () => (
  <div style={fileira}>
    <PilulaDeArquivo imagem={wms} nome="colado-1c5399bf6e.png" medida="410×165" />
    <PilulaDeArquivo nome="relatorio.pdf" medida="184 KB" />
    <PilulaDeArquivo imagem={inicio} sumiu nome="captura-antiga.png" />
  </div>
);

export const NoBalao = () => (
  <BalaoDoUsuario
    style={{ maxWidth: 560 }}
    anexos={
      <>
        <PilulaDeArquivo noBalao imagem={balao} nome="colado-b51d81ef3e.png" medida="601×263" />
        <PilulaDeArquivo noBalao imagem={inicio} nome="a02-historico.png" medida="1600×1000" />
      </>
    }
  />
);
