// As imagens do próprio app (../assets), embutidas como data URI. Nunca redesenhar: usar os arquivos.
import claudeDisco from '../../assets/marcas/claude.svg';
import claudeGlifo from '../../assets/marcas/claude-glifo.svg';
import codexDisco from '../../assets/marcas/codex.svg';
import codexGlifo from '../../assets/marcas/codex-glifo.svg';
import linearDisco from '../../assets/marcas/linear.svg';
import logo128 from '../../assets/vitral-128.png';
import logoV from '../../assets/vitral-v.png';

export const MARCAS = {
  claude: { disco: claudeDisco, glifo: claudeGlifo },
  codex: { disco: codexDisco, glifo: codexGlifo },
  linear: { disco: linearDisco, glifo: linearDisco },
};
export const LOGO = logo128;
export const LOGO_V = logoV;
