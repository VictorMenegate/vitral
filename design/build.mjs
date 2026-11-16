// Build do vitral-ui: gera os tokens, junta o CSS com as fontes e os ícones, empacota o JS com o
// esbuild e tira os .d.ts do JSDoc com o tsc. Tudo determinístico: rodar duas vezes dá o mesmo dist/.
//
//   node build.mjs
//
// Fontes da verdade: tokens/tokens.json (os valores de src/tema.rs), src/estilos/vitral.css (a
// montagem das peças), src/icones/*.svg (Lucide, os mesmos do app), ../assets/fontes e
// ../assets/marcas (os arquivos do próprio app, nunca copiados para cá).

import { build } from 'esbuild';
import { execFileSync } from 'node:child_process';
import { cpSync, existsSync, mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const RAIZ = dirname(fileURLToPath(import.meta.url));
const DIST = join(RAIZ, 'dist');
const FONTES_DO_APP = join(RAIZ, '..', 'assets', 'fontes');

rmSync(DIST, { recursive: true, force: true });
mkdirSync(join(DIST, 'fonts'), { recursive: true });

const semTravessao = (texto, onde) => {
  if (/[\u2013\u2014]/.test(texto)) throw new Error(`${onde}: tem travessão ou meia-risca`);
  return texto;
};

// 1. tokens/tokens.css a partir do tokens.json -----------------------------------------------
const t = JSON.parse(readFileSync(join(RAIZ, 'tokens', 'tokens.json'), 'utf8'));
const comentario = (s) => (s ? ` /* ${String(s).replace(/\*\//g, '* /').replace(/\s+/g, ' ').trim()} */` : '');
const linhas = [
  '/* Vitral: os tokens do app (src/tema.rs) como variáveis CSS. Gerado por build.mjs a partir de',
  '   tokens/tokens.json; não edite à mão. Valores em px a 100% (no app, 1 rem = 15 px e tudo',
  '   cresce com o zoom). Só tema escuro: o app não tem claro. */',
  ':root {',
  '  color-scheme: dark;',
  '  /* Cores */',
];
for (const c of t.color.tokens) linhas.push(`  --${c.name}: ${c.value};${comentario(c.usage)}`);
const familias = [
  ['Sombras e arestas', t.shadow.tokens],
  ['Espaços (a grade é 2, 4, 6, 8, 12, 16, 24 e 32)', t.spacing.tokens],
  ['Cantos', t.radius.tokens],
  ['Alturas', t.altura.tokens],
  ['Larguras', t.largura.tokens],
  ['Opacidades', t.opacidade.tokens],
];
for (const [titulo, lista] of familias) {
  linhas.push(`  /* ${titulo} */`);
  for (const s of lista) linhas.push(`  --${s.name}: ${s.value};${comentario(s.usage)}`);
}
linhas.push('  /* Fontes */');
for (const [k, v] of Object.entries(t.type.families)) linhas.push(`  --font-${k}: ${v};`);
linhas.push('}', '');
linhas.push('/* Tipografia: um papel por classe (vt-tipo-<papel>), os do trait Tipo de src/tema.rs. */');
const TABULAR = new Set(['metadado', 'micro', 'numero', 'etiqueta', 'tecla', 'mono-pequeno']);
for (const g of t.type.groups) {
  for (const s of g.styles) {
    const fam = s.family ?? g.family;
    const extra = TABULAR.has(s.name) ? ' font-variant-numeric: tabular-nums;' : '';
    linhas.push(`.vt-tipo-${s.name} { font-family: var(--font-${fam}); font-size: ${s.fontSize}; line-height: ${s.lineHeight}; font-weight: ${s.fontWeight};${extra} }${comentario(s.usage)}`);
  }
}
const tokensCss = semTravessao(linhas.join('\n') + '\n', 'tokens.css');
writeFileSync(join(RAIZ, 'tokens', 'tokens.css'), tokensCss);

// 2. dist/vitral-ui.css: fontes + peças + ícones ---------------------------------------------
const faces = [];
for (const f of t.type.fonts) {
  const nome = f.file.split('/').pop();
  cpSync(join(FONTES_DO_APP, nome), join(DIST, 'fonts', nome));
  faces.push(`@font-face { font-family: "${f.family}"; src: url(./fonts/${nome}) format("truetype"); font-weight: ${f.weight}; font-style: ${f.style ?? 'normal'}; font-display: block; }`);
}
cpSync(join(FONTES_DO_APP, 'OFL.txt'), join(DIST, 'fonts', 'OFL.txt'));

const uri = (svg) => {
  let s = svg.replace(/\s+/g, ' ').replace(/> </g, '><').trim();
  s = s.replace(/currentColor/g, 'black').replace(/"/g, "'");
  const enc = encodeURIComponent(s).replace(/%20/g, ' ').replace(/%2F/g, '/').replace(/%3D/g, '=')
    .replace(/%3A/g, ':').replace(/%3B/g, ';').replace(/%2C/g, ',').replace(/%27/g, "'")
    .replace(/%28/g, '(').replace(/%29/g, ')');
  return `url("data:image/svg+xml,${enc}")`;
};
const icones = readdirSync(join(RAIZ, 'src', 'icones')).filter((f) => f.endsWith('.svg')).sort();
const regrasIcones = ['', '/* Ícones: Lucide (ISC), os mesmos SVG que o app embute (gpui-kit-assets 0.8.1), como máscara na cor do texto. */'];
for (const f of icones) {
  let svg = readFileSync(join(RAIZ, 'src', 'icones', f), 'utf8');
  if (f === 'escudo-cheio.svg') svg = svg.replace('<path mask', "<path fill='black' mask");
  regrasIcones.push(`.vt-i-${f.slice(0, -4)} { --vt-icone: ${uri(svg)}; }`);
}
const pecas = readFileSync(join(RAIZ, 'src', 'estilos', 'vitral.css'), 'utf8');
const css = semTravessao(
  '/* vitral-ui: as peças do Vitral em CSS. Os valores vêm dos tokens (tokens/tokens.css); aqui só a montagem. */\n' +
    faces.join('\n') + '\n\n' + pecas + regrasIcones.join('\n') + '\n',
  'vitral-ui.css',
);
writeFileSync(join(DIST, 'vitral-ui.css'), css);
writeFileSync(join(RAIZ, 'src', 'icones', 'nomes.js'),
  '// Gerado por build.mjs: os nomes de ícone que a classe vt-i-<nome> conhece.\n' +
  `export const NOMES_DE_ICONE = ${JSON.stringify(icones.map((f) => f.slice(0, -4)))};\n`);

// 3. dist/index.js -----------------------------------------------------------------------------
await build({
  entryPoints: [join(RAIZ, 'src', 'index.js')],
  outfile: join(DIST, 'index.js'),
  bundle: true,
  format: 'esm',
  platform: 'browser',
  target: 'es2020',
  jsx: 'automatic',
  external: ['react', 'react/jsx-runtime', 'react-dom'],
  loader: { '.svg': 'dataurl', '.png': 'dataurl', '.jsx': 'jsx' },
  charset: 'utf8',
  legalComments: 'none',
  logLevel: 'warning',
});

// 4. dist/**/*.d.ts a partir do JSDoc ---------------------------------------------------------
execFileSync(join(RAIZ, 'node_modules', '.bin', 'tsc'), ['-p', join(RAIZ, 'tsconfig.json')], { stdio: 'inherit' });
if (!existsSync(join(DIST, 'index.d.ts'))) throw new Error('tsc não gerou dist/index.d.ts');

console.log(`vitral-ui: ${icones.length} ícones, ${faces.length} fontes, tokens.css ${tokensCss.length} bytes, vitral-ui.css ${css.length} bytes`);
