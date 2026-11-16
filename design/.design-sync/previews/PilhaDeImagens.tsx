import { PilhaDeImagens } from 'vitral-ui';
import { balao, inicio, modelo, rodando, wms } from './_capturas';

export const QuatroImagens = () => <PilhaDeImagens imagens={[inicio, wms, balao, modelo]} />;

export const NoveComPonteiro = () => <PilhaDeImagens imagens={[rodando, modelo, wms, inicio, balao]} total={9} comPonteiro />;
