import { PilhaDeImagens, RespostaDoAgente, TrilhaDeFerramentas } from 'vitral-ui';
import { balao, inicio, modelo, wms } from './_capturas';

const coluna = { display: 'flex', flexDirection: 'column', gap: 12, maxWidth: 640 } as const;

export const UmTurno = () => (
  <div style={coluna}>
    <RespostaDoAgente>
      <p>Vou conferir onde o balão é montado e rodar os testes do tema antes de mexer.</p>
    </RespostaDoAgente>
    <TrilhaDeFerramentas
      resumo="13 comandos · 1 editado · 3 lidos"
      aberta={false}
      abaixoDoResumo={<PilhaDeImagens imagens={[inicio, wms, balao, modelo]} />}
    />
    <RespostaDoAgente acoes hora="12:53">
      <p>
        Pronto. O balão agora usa o raio das peças e a fala ficou um degrau abaixo da resposta. Os caminhos que existem viram link, como <a href="#">src/ui/mensagens.rs</a>.
      </p>
    </RespostaDoAgente>
  </div>
);

export const Markdown = () => (
  <div style={coluna}>
    <RespostaDoAgente>
      <h2>O que mudou</h2>
      <ul>
        <li>
          O balão usa <code>raio-da-peca</code> (16) e a ponta embaixo à direita.
        </li>
        <li>
          A fala passou para <b>14/22</b>, um degrau abaixo da resposta.
        </li>
      </ul>
      <p>Os testes do tema passaram: 48 de 48.</p>
    </RespostaDoAgente>
  </div>
);
