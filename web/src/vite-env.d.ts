/// <reference types="vite/client" />

declare module '*.wasm' {
  const value: WebAssembly.Module;
  export default value;
}
