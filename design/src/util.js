/** Junta classes, ignorando as vazias. */
export function cx(...classes) {
  return classes.filter(Boolean).join(' ');
}

/** A cor do arco pela faixa (cor_da_faixa em graficos.rs): a base até 69%, âmbar de 70% a 89%, vermelho a partir de 90%. */
export function corDaFaixa(fracao, base = 'var(--destaque)') {
  if (fracao >= 0.9) return 'var(--vermelho)';
  if (fracao >= 0.7) return 'var(--ambar)';
  return base;
}
