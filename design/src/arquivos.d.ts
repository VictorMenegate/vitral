// Os arquivos de imagem entram no bundle como data URI (loader dataurl do esbuild).
declare module '*.svg' {
  const url: string;
  export default url;
}
declare module '*.png' {
  const url: string;
  export default url;
}
